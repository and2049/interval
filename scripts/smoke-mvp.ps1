param(
    [string]$DatabaseUrl = "sqlite://interval.db",
    [string]$BackendBind = "127.0.0.1:4000",
    [int]$FrontendPort = 5173,
    [int]$TimeoutSeconds = 20,
    [long]$SessionKey = 9472,
    [long]$MeetingKey = 1229,
    [string]$ExpectedMeetingName = "Bahrain Grand Prix",
    [double]$SnapshotT = 600.0,
    [string]$ExpectedMapMode = "projected",
    [string]$ExpectedTrackQuality = "projected",
    [string]$ExpectedGeometrySource = "curated_static",
    [int]$MinimumTimingRows = 20
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Net.Http

trap {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}

$root = Split-Path -Parent $PSScriptRoot
$frontendDir = Join-Path $root "frontend"
$tmpDir = Join-Path $root "tmp"
$backendExe = Join-Path $root "target\debug\interval-backend.exe"
$viteEntry = Join-Path $frontendDir "node_modules\vite\bin\vite.js"
$backendOutLog = Join-Path $tmpDir "smoke-backend.out.log"
$backendErrLog = Join-Path $tmpDir "smoke-backend.err.log"
$frontendOutLog = Join-Path $tmpDir "smoke-frontend.out.log"
$frontendErrLog = Join-Path $tmpDir "smoke-frontend.err.log"
$backendUrl = "http://$BackendBind"
$frontendUrl = "http://127.0.0.1:$FrontendPort"
$backendStarted = $null
$frontendStarted = $null

function Ensure-Directory($path) {
    if (-not (Test-Path -LiteralPath $path)) {
        New-Item -ItemType Directory -Path $path | Out-Null
    }
}

function Read-LogTail($path) {
    if (-not (Test-Path -LiteralPath $path)) {
        return "<log not found: $path>"
    }
    $lines = Get-Content -LiteralPath $path -Tail 40 -ErrorAction SilentlyContinue
    if (-not $lines) {
        return "<log is empty: $path>"
    }
    return ($lines -join [Environment]::NewLine)
}

function Wait-ForProcessHttp($url, $seconds, $process, $errorLog, $label) {
    $deadline = (Get-Date).AddSeconds($seconds)
    do {
        if ($process -and $process.HasExited) {
            $tail = Read-LogTail $errorLog
            throw "$label exited before $url became healthy. Last error log lines:$([Environment]::NewLine)$tail"
        }

        try {
            return Invoke-RestMethod -Uri $url
        } catch {
            Start-Sleep -Milliseconds 300
        }
    } while ((Get-Date) -lt $deadline)

    throw "Timed out waiting for $url"
}

function Assert-Equal($actual, $expected, $label) {
    if ($actual -ne $expected) {
        throw "$label expected '$expected' but got '$actual'"
    }
}

function Assert-Truthy($actual, $label) {
    if (-not $actual) {
        throw "$label was not truthy"
    }
}

function Assert-CountAtLeast($actual, $minimum, $label) {
    if ($actual.Count -lt $minimum) {
        throw "$label expected at least $minimum items but got $($actual.Count)"
    }
}

function Assert-ReplayEvents($payload, $label) {
    Assert-Equal $payload.contract_version "replay.v1" "$label contract"
    Assert-CountAtLeast $payload.events 5 "$label events"

    $previous = [double]::NegativeInfinity
    $kinds = @{}
    foreach ($event in $payload.events) {
        if ([double]$event.t -lt $previous) {
            throw "$label events were not ordered by replay time"
        }
        $previous = [double]$event.t
        $kinds[$event.kind] = $true
        Assert-Truthy $event.id "$label event id"
        Assert-Truthy $event.message "$label event message"
        Assert-Truthy $event.source "$label event source"
    }

    Assert-Truthy ($kinds.ContainsKey("race_control")) "$label race-control events"
    $hasRaceTimelineCoverage =
        $kinds.ContainsKey("pit_stop") -or
        $kinds.ContainsKey("stint_change") -or
        $kinds.ContainsKey("leader_change") -or
        $kinds.ContainsKey("weather_change")
    Assert-Truthy $hasRaceTimelineCoverage "$label derived or race event coverage"
}

function Assert-HttpJsonError($url, $expectedStatus, $messageFragment, $label) {
    $client = [System.Net.Http.HttpClient]::new()
    $response = $null
    try {
        $response = $client.GetAsync($url).GetAwaiter().GetResult()
        $status = [int]$response.StatusCode
        if ($status -ne $expectedStatus) {
            throw "$label expected HTTP $expectedStatus but got HTTP $status"
        }

        $body = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult() | ConvertFrom-Json
        if (-not ($body.error -like "*$messageFragment*")) {
            throw "$label expected error containing '$messageFragment' but got '$($body.error)'"
        }
    } finally {
        if ($response) {
            $response.Dispose()
        }
        $client.Dispose()
    }
}

function Assert-ReplayReady($backendUrl, $meetingKey, $sessionKey) {
    $sessions = Invoke-RestMethod -Uri "$backendUrl/api/sessions?meeting_key=$meetingKey"
    $session = $sessions | Where-Object { $_.session.session_key -eq $sessionKey } | Select-Object -First 1
    if (-not $session) {
        throw "Session $sessionKey was not available for meeting $meetingKey. Select it in the app once so discovery can cache meeting/session metadata."
    }
    if (-not $session.replay_ready) {
        throw "Replay $sessionKey is not cached yet. Run the app and choose INGEST + OPEN for session $sessionKey before running this cached smoke."
    }
}

function Assert-ReplayStream($url, $seconds, $sessionKey, $minimumTimingRows, $expectedTrackQuality) {
    $client = [System.Net.Http.HttpClient]::new()
    $client.Timeout = [TimeSpan]::FromSeconds($seconds)
    $response = $null
    $stream = $null
    $reader = $null
    try {
        $response = $client.GetAsync(
            $url,
            [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead
        ).GetAwaiter().GetResult()

        if (-not $response.IsSuccessStatusCode) {
            throw "Replay stream returned HTTP $([int]$response.StatusCode)"
        }

        $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
        $reader = [System.IO.StreamReader]::new($stream)
        $deadline = (Get-Date).AddSeconds($seconds)
        $currentEvent = $null
        $seenMetadata = $false
        $seenSnapshot = $false

        while ((Get-Date) -lt $deadline -and -not ($seenMetadata -and $seenSnapshot)) {
            $lineTask = $reader.ReadLineAsync()
            if (-not $lineTask.Wait(500)) {
                continue
            }

            $line = $lineTask.Result
            if ($null -eq $line) {
                break
            }

            if ($line.StartsWith("event:")) {
                $currentEvent = $line.Substring(6).Trim()
                continue
            }

            if (-not $line.StartsWith("data:")) {
                continue
            }

            $payload = $line.Substring(5).TrimStart() | ConvertFrom-Json
            if ($currentEvent -eq "metadata") {
                Assert-Equal $payload.contract_version "replay.v1" "stream metadata contract"
                Assert-Equal $payload.session.session_key $sessionKey "stream metadata session"
                $seenMetadata = $true
            } elseif ($currentEvent -eq "snapshot") {
                Assert-Equal $payload.contract_version "replay.v1" "stream snapshot contract"
                Assert-Equal $payload.cursor.session_key $sessionKey "stream snapshot session"
                Assert-Truthy ($payload.timing.rows.Count -ge $minimumTimingRows) "stream timing rows"
                Assert-Equal $payload.timing.quality "ready" "stream timing quality"
                Assert-Equal $payload.track.quality $expectedTrackQuality "stream track quality"
                $seenSnapshot = $true
            }
        }

        Assert-Truthy $seenMetadata "stream metadata event"
        Assert-Truthy $seenSnapshot "stream snapshot event"
    } finally {
        if ($reader) {
            $reader.Dispose()
        }
        if ($stream) {
            $stream.Dispose()
        }
        if ($response) {
            $response.Dispose()
        }
        $client.Dispose()
    }
}

function Assert-ReplayStreamCadence($url, $seconds, $frameStep, $expectedStartT) {
    $client = [System.Net.Http.HttpClient]::new()
    $client.Timeout = [TimeSpan]::FromSeconds($seconds)
    $response = $null
    $stream = $null
    $reader = $null
    try {
        $response = $client.GetAsync(
            $url,
            [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead
        ).GetAwaiter().GetResult()

        if (-not $response.IsSuccessStatusCode) {
            throw "Replay cadence stream returned HTTP $([int]$response.StatusCode)"
        }

        $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
        $reader = [System.IO.StreamReader]::new($stream)
        $deadline = (Get-Date).AddSeconds($seconds)
        $currentEvent = $null
        $snapshots = @()
        $started = [System.Diagnostics.Stopwatch]::StartNew()

        while ((Get-Date) -lt $deadline -and $snapshots.Count -lt 3) {
            $lineTask = $reader.ReadLineAsync()
            if (-not $lineTask.Wait(500)) {
                continue
            }

            $line = $lineTask.Result
            if ($null -eq $line) {
                break
            }

            if ($line.StartsWith("event:")) {
                $currentEvent = $line.Substring(6).Trim()
                continue
            }

            if ($currentEvent -ne "snapshot" -or -not $line.StartsWith("data:")) {
                continue
            }

            $payload = $line.Substring(5).TrimStart() | ConvertFrom-Json
            $snapshots += [pscustomobject]@{
                t = [double]$payload.cursor.t
                wallMs = $started.ElapsedMilliseconds
            }
        }

        Assert-CountAtLeast $snapshots 3 "cadence stream snapshots"
        Assert-Equal $snapshots[0].t $expectedStartT "cadence stream start frame"
        Assert-Equal $snapshots[1].t ([math]::Round($expectedStartT + $frameStep, 3)) "cadence stream second frame"
        Assert-Equal $snapshots[2].t ([math]::Round($expectedStartT + (2.0 * $frameStep), 3)) "cadence stream third frame"
        if ($snapshots[2].wallMs -gt 3000) {
            throw "cadence stream delivered three frames too slowly: $($snapshots[2].wallMs)ms"
        }
    } finally {
        if ($reader) {
            $reader.Dispose()
        }
        if ($stream) {
            $stream.Dispose()
        }
        if ($response) {
            $response.Dispose()
        }
        $client.Dispose()
    }
}

Ensure-Directory $tmpDir

cargo build -p interval-backend
if ($LASTEXITCODE -ne 0) {
    throw "cargo build -p interval-backend failed with exit code $LASTEXITCODE"
}

if (-not (Test-Path -LiteralPath $viteEntry)) {
    throw "Vite is not installed. Run 'bun install' in frontend first."
}

try {
    $env:DATABASE_URL = $DatabaseUrl
    $env:INTERVAL_BIND = $BackendBind
    $env:INTERVAL_REBUILD_SESSION_ON_START = "$SessionKey"
    $backendStarted = Start-Process `
        -FilePath $backendExe `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $backendOutLog `
        -RedirectStandardError $backendErrLog `
        -PassThru

    Wait-ForProcessHttp "$backendUrl/healthz" $TimeoutSeconds $backendStarted $backendErrLog "Backend smoke process" | Out-Null
    Assert-ReplayReady $backendUrl $MeetingKey $SessionKey

    $metadata = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$SessionKey/replay/metadata"
    Assert-Equal $metadata.contract_version "replay.v1" "metadata contract"
    Assert-Equal $metadata.session.session_key $SessionKey "metadata session"
    Assert-Equal $metadata.meeting.meeting_key $MeetingKey "metadata meeting"
    Assert-Equal $metadata.meeting.name $ExpectedMeetingName "metadata meeting name"
    Assert-Truthy $metadata.available_channels.timing "timing channel"
    Assert-Truthy $metadata.available_channels.track_geometry "track geometry channel"

    $snapshot = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$SessionKey/replay/snapshot?t=$SnapshotT"
    Assert-Equal $snapshot.contract_version "replay.v1" "snapshot contract"
    Assert-Equal $snapshot.cursor.session_key $SessionKey "snapshot session"
    Assert-Truthy ($snapshot.timing.rows.Count -ge $MinimumTimingRows) "timing rows"
    Assert-Equal $snapshot.timing.quality "ready" "timing quality"
    Assert-Equal $snapshot.track.map_mode $ExpectedMapMode "map mode"
    Assert-Equal $snapshot.track.quality $ExpectedTrackQuality "track quality"
    Assert-Equal $snapshot.weather.quality "ready" "weather quality"
    Assert-Equal $snapshot.race_control.quality "ready" "race-control quality"
    Assert-CountAtLeast $snapshot.derived_metrics $MinimumTimingRows "derived metrics"
    Assert-Equal $snapshot.derived_metrics[0].label "3-lap avg" "derived metric label"
    Assert-CountAtLeast $snapshot.track.positions $MinimumTimingRows "track positions"
    $expectedQualityPositions = $snapshot.track.positions | Where-Object { $_.quality -eq $ExpectedTrackQuality }
    Assert-CountAtLeast $expectedQualityPositions $MinimumTimingRows "expected-quality driver positions"
    Assert-HttpJsonError "$backendUrl/api/sessions/$SessionKey/replay/snapshot?t=NaN" 400 "finite number" "invalid snapshot cursor"

    $geometry = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$SessionKey/track/geometry"
    Assert-Equal $geometry.contract_version "replay.v1" "geometry contract"
    Assert-Equal $geometry.source $ExpectedGeometrySource "geometry source"
    Assert-Equal $geometry.quality "ready" "geometry quality"
    Assert-Equal $geometry.map_mode $ExpectedMapMode "geometry map mode"
    Assert-CountAtLeast $geometry.centerline 20 "geometry centerline"
    Assert-CountAtLeast $geometry.inner_edge 20 "geometry inner edge"
    Assert-CountAtLeast $geometry.outer_edge 20 "geometry outer edge"
    Assert-Truthy ($geometry.circuit_length -gt 1000) "geometry circuit length"

    $events = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$SessionKey/replay/events"
    Assert-ReplayEvents $events "events"

    Assert-ReplayStream "$backendUrl/api/sessions/$SessionKey/replay/stream" $TimeoutSeconds $SessionKey $MinimumTimingRows $ExpectedTrackQuality
    Assert-ReplayStreamCadence "$backendUrl/api/sessions/$SessionKey/replay/stream?from=$SnapshotT&speed=4" $TimeoutSeconds ([double]$metadata.frame_step_seconds) $SnapshotT

    $frontendStarted = Start-Process `
        -FilePath "node" `
        -ArgumentList @($viteEntry, "--host", "127.0.0.1", "--port", "$FrontendPort", "--strictPort") `
        -WorkingDirectory $frontendDir `
        -WindowStyle Hidden `
        -RedirectStandardOutput $frontendOutLog `
        -RedirectStandardError $frontendErrLog `
        -PassThru

    Wait-ForProcessHttp "$frontendUrl/healthz" $TimeoutSeconds $frontendStarted $frontendErrLog "Frontend smoke process" | Out-Null
    $html = (New-Object System.Net.WebClient).DownloadString($frontendUrl)
    Assert-Truthy ($html -like "*<div id=`"root`"></div>*") "frontend app shell"

    $proxiedMetadata = Invoke-RestMethod -Uri "$frontendUrl/api/sessions/$SessionKey/replay/metadata"
    Assert-Equal $proxiedMetadata.session.session_key $SessionKey "proxied metadata session"
    Assert-Equal $proxiedMetadata.meeting.name $ExpectedMeetingName "proxied metadata meeting"

    $proxiedEvents = Invoke-RestMethod -Uri "$frontendUrl/api/sessions/$SessionKey/replay/events"
    Assert-ReplayEvents $proxiedEvents "proxied events"

    Write-Output "MVP smoke passed: replay $SessionKey metadata, meeting context, snapshot, events, geometry, stream, and Vite proxy are healthy."
} finally {
    Remove-Item Env:\INTERVAL_REBUILD_SESSION_ON_START -ErrorAction SilentlyContinue
    if ($frontendStarted -and -not $frontendStarted.HasExited) {
        Stop-Process -Id $frontendStarted.Id -Force -ErrorAction SilentlyContinue
    }
    if ($backendStarted -and -not $backendStarted.HasExited) {
        Stop-Process -Id $backendStarted.Id -Force -ErrorAction SilentlyContinue
    }
}
