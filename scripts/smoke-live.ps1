param(
    [string]$DatabaseUrl = "sqlite://tmp/live-smoke.db",
    [string]$BackendBind = "127.0.0.1:4000",
    [int]$FrontendPort = 5174,
    [string]$OpenF1MockBaseUrl = "http://127.0.0.1:45101/v1/",
    [int]$OpenF1MockPort = 45101,
    [int]$TimeoutSeconds = 20
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
$mockScript = Join-Path $PSScriptRoot "openf1_live_mock.py"
$liveCheckScript = Join-Path $PSScriptRoot "check-live-current.ps1"
$mockOutLog = Join-Path $tmpDir "smoke-live-openf1.out.log"
$mockErrLog = Join-Path $tmpDir "smoke-live-openf1.err.log"
$backendOutLog = Join-Path $tmpDir "smoke-live-backend.out.log"
$backendErrLog = Join-Path $tmpDir "smoke-live-backend.err.log"
$frontendOutLog = Join-Path $tmpDir "smoke-live-frontend.out.log"
$frontendErrLog = Join-Path $tmpDir "smoke-live-frontend.err.log"
$backendUrl = "http://$BackendBind"
$frontendUrl = "http://127.0.0.1:$FrontendPort"
$mockStarted = $null
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

function Assert-GreaterThan($actual, $minimum, $label) {
    if ([double]$actual -le [double]$minimum) {
        throw "$label expected greater than $minimum but got $actual"
    }
}

function Assert-HttpStatus($method, $url, $expectedStatus, $label) {
    $client = [System.Net.Http.HttpClient]::new()
    try {
        $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::new($method), $url)
        $response = $client.SendAsync($request).GetAwaiter().GetResult()
        $status = [int]$response.StatusCode
        if ($status -ne $expectedStatus) {
            $body = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            throw "$label expected HTTP $expectedStatus but got HTTP $status. Body: $body"
        }
    } finally {
        if ($request) {
            $request.Dispose()
        }
        if ($response) {
            $response.Dispose()
        }
        $client.Dispose()
    }
}

function Stop-SmokeProcess([ref]$processRef) {
    $process = $processRef.Value
    if ($process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $process.WaitForExit(5000) | Out-Null
    }
    $processRef.Value = $null
}

function Assert-LiveStream($url, $seconds, $sessionKey) {
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
            throw "Live stream returned HTTP $([int]$response.StatusCode)"
        }

        $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
        $reader = [System.IO.StreamReader]::new($stream)
        $deadline = (Get-Date).AddSeconds($seconds)
        $currentEvent = $null
        $seenMetadata = $false
        $seenSnapshot = $false
        $seenEvent = $false
        $lineTask = $null

        while ((Get-Date) -lt $deadline -and -not ($seenMetadata -and $seenSnapshot -and $seenEvent)) {
            if ($null -eq $lineTask) {
                $lineTask = $reader.ReadLineAsync()
            }
            if (-not $lineTask.Wait(500)) {
                continue
            }

            $line = $lineTask.Result
            $lineTask = $null
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
                Assert-Equal $payload.contract_version "replay.v1" "live stream metadata contract"
                Assert-Equal $payload.session.session_key $sessionKey "live stream metadata session"
                Assert-Equal $payload.data_sources[0].name "openf1_live" "live stream metadata source"
                $seenMetadata = $true
            } elseif ($currentEvent -eq "snapshot") {
                Assert-Equal $payload.contract_version "replay.v1" "live stream snapshot contract"
                Assert-Equal $payload.cursor.session_key $sessionKey "live stream snapshot session"
                Assert-CountAtLeast $payload.timing.rows 2 "live stream timing rows"
                Assert-CountAtLeast $payload.track.positions 2 "live stream track positions"
                $seenSnapshot = $true
            } elseif ($currentEvent -eq "event") {
                Assert-Truthy $payload.id "live stream event id"
                Assert-Truthy $payload.kind "live stream event kind"
                Assert-Truthy $payload.source "live stream event source"
                $seenEvent = $true
            }
        }

        Assert-Truthy $seenMetadata "live stream metadata event"
        Assert-Truthy $seenSnapshot "live stream snapshot event"
        Assert-Truthy $seenEvent "live stream typed event"
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

if (-not (Test-Path -LiteralPath $mockScript)) {
    throw "OpenF1 live mock script not found at $mockScript"
}

if (-not (Test-Path -LiteralPath $liveCheckScript)) {
    throw "OpenF1 live diagnostic script not found at $liveCheckScript"
}

if (-not (Test-Path -LiteralPath $viteEntry)) {
    throw "Vite is not installed. Run 'bun install' in frontend first."
}

cargo build -p interval-backend
if ($LASTEXITCODE -ne 0) {
    throw "cargo build -p interval-backend failed with exit code $LASTEXITCODE"
}

try {
    $preSessionMockPort = $OpenF1MockPort + 1
    $preSessionMockBaseUrl = "http://127.0.0.1:$preSessionMockPort/v1/"
    $mockStarted = Start-Process `
        -FilePath "python" `
        -ArgumentList @($mockScript, "--host", "127.0.0.1", "--port", "$preSessionMockPort", "--start-offset-minutes", "15") `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $mockOutLog `
        -RedirectStandardError $mockErrLog `
        -PassThru

    Wait-ForProcessHttp "$preSessionMockBaseUrl/meetings" $TimeoutSeconds $mockStarted $mockErrLog "OpenF1 pre-session mock process" | Out-Null

    $env:DATABASE_URL = $DatabaseUrl
    $env:INTERVAL_BIND = $BackendBind
    $env:INTERVAL_OPENF1_LIVE_ENABLED = "true"
    $env:INTERVAL_OPENF1_LIVE_BASE_URL = $preSessionMockBaseUrl

    $backendStarted = Start-Process `
        -FilePath $backendExe `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $backendOutLog `
        -RedirectStandardError $backendErrLog `
        -PassThru

    Wait-ForProcessHttp "$backendUrl/healthz" $TimeoutSeconds $backendStarted $backendErrLog "Backend pre-session live smoke process" | Out-Null

    $preSessionCurrent = Invoke-RestMethod -Uri "$backendUrl/api/live/current"
    Assert-Equal $preSessionCurrent.availability "inactive" "pre-session live current availability"
    Assert-Equal $preSessionCurrent.active $false "pre-session live current active"
    Assert-Truthy $preSessionCurrent.next_session "pre-session next live session"
    if ($null -ne $preSessionCurrent.session) {
        throw "pre-session live current session expected null but got '$($preSessionCurrent.session)'"
    }

    Stop-SmokeProcess ([ref]$backendStarted)
    Stop-SmokeProcess ([ref]$mockStarted)

    $postSessionMockPort = $OpenF1MockPort + 2
    $postSessionMockBaseUrl = "http://127.0.0.1:$postSessionMockPort/v1/"
    $mockStarted = Start-Process `
        -FilePath "python" `
        -ArgumentList @($mockScript, "--host", "127.0.0.1", "--port", "$postSessionMockPort", "--start-offset-minutes", "-70", "--duration-minutes", "60") `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $mockOutLog `
        -RedirectStandardError $mockErrLog `
        -PassThru

    Wait-ForProcessHttp "$postSessionMockBaseUrl/meetings" $TimeoutSeconds $mockStarted $mockErrLog "OpenF1 post-session mock process" | Out-Null

    $env:DATABASE_URL = $DatabaseUrl
    $env:INTERVAL_BIND = $BackendBind
    $env:INTERVAL_OPENF1_LIVE_ENABLED = "true"
    $env:INTERVAL_OPENF1_LIVE_BASE_URL = $postSessionMockBaseUrl

    $backendStarted = Start-Process `
        -FilePath $backendExe `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $backendOutLog `
        -RedirectStandardError $backendErrLog `
        -PassThru

    Wait-ForProcessHttp "$backendUrl/healthz" $TimeoutSeconds $backendStarted $backendErrLog "Backend post-session live smoke process" | Out-Null

    $postSessionCurrent = Invoke-RestMethod -Uri "$backendUrl/api/live/current"
    Assert-Equal $postSessionCurrent.availability "active" "post-session live current availability"
    Assert-Truthy $postSessionCurrent.active "post-session live current active"
    $postSessionKey = [long]$postSessionCurrent.session.session_key

    Invoke-RestMethod -Method Post -Uri "$backendUrl/api/sessions/$postSessionKey/live/start" | Out-Null
    $postSessionMetadata = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$postSessionKey/live/metadata"
    $postSessionSnapshot = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$postSessionKey/live/snapshot"
    Assert-Equal $postSessionMetadata.max_t 3600 "post-session live metadata max_t"
    Assert-Equal $postSessionSnapshot.cursor.t $postSessionMetadata.max_t "post-session live snapshot clamp"

    Stop-SmokeProcess ([ref]$backendStarted)
    Stop-SmokeProcess ([ref]$mockStarted)

    $sprintMockPort = $OpenF1MockPort + 7
    $sprintMockBaseUrl = "http://127.0.0.1:$sprintMockPort/v1/"
    $mockStarted = Start-Process `
        -FilePath "python" `
        -ArgumentList @($mockScript, "--host", "127.0.0.1", "--port", "$sprintMockPort", "--session-name", "Sprint", "--session-type", "Sprint") `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $mockOutLog `
        -RedirectStandardError $mockErrLog `
        -PassThru

    Wait-ForProcessHttp "$sprintMockBaseUrl/meetings" $TimeoutSeconds $mockStarted $mockErrLog "OpenF1 sprint mock process" | Out-Null

    $env:DATABASE_URL = $DatabaseUrl
    $env:INTERVAL_BIND = $BackendBind
    $env:INTERVAL_OPENF1_LIVE_ENABLED = "true"
    $env:INTERVAL_OPENF1_LIVE_BASE_URL = $sprintMockBaseUrl

    $backendStarted = Start-Process `
        -FilePath $backendExe `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $backendOutLog `
        -RedirectStandardError $backendErrLog `
        -PassThru

    Wait-ForProcessHttp "$backendUrl/healthz" $TimeoutSeconds $backendStarted $backendErrLog "Backend sprint live smoke process" | Out-Null

    $sprintCurrent = Invoke-RestMethod -Uri "$backendUrl/api/live/current"
    Assert-Equal $sprintCurrent.availability "active" "sprint live current availability"
    Assert-Truthy $sprintCurrent.active "sprint live current active"
    Assert-Equal $sprintCurrent.session.session_type "sprint" "sprint live current session type"
    $sprintSessionKey = [long]$sprintCurrent.session.session_key

    Invoke-RestMethod -Method Post -Uri "$backendUrl/api/sessions/$sprintSessionKey/live/start" | Out-Null
    $sprintMetadata = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$sprintSessionKey/live/metadata"
    $sprintSnapshot = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$sprintSessionKey/live/snapshot"
    Assert-Equal $sprintMetadata.session.session_type "sprint" "sprint live metadata session type"
    Assert-CountAtLeast $sprintSnapshot.timing.rows 2 "sprint live timing rows"

    Stop-SmokeProcess ([ref]$backendStarted)
    Stop-SmokeProcess ([ref]$mockStarted)

    $warmupMockPort = $OpenF1MockPort + 3
    $warmupMockBaseUrl = "http://127.0.0.1:$warmupMockPort/v1/"
    $mockStarted = Start-Process `
        -FilePath "python" `
        -ArgumentList @($mockScript, "--host", "127.0.0.1", "--port", "$warmupMockPort", "--missing-endpoints", "drivers") `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $mockOutLog `
        -RedirectStandardError $mockErrLog `
        -PassThru

    Wait-ForProcessHttp "$warmupMockBaseUrl/meetings" $TimeoutSeconds $mockStarted $mockErrLog "OpenF1 warmup mock process" | Out-Null

    $env:DATABASE_URL = $DatabaseUrl
    $env:INTERVAL_BIND = $BackendBind
    $env:INTERVAL_OPENF1_LIVE_ENABLED = "true"
    $env:INTERVAL_OPENF1_LIVE_BASE_URL = $warmupMockBaseUrl

    $backendStarted = Start-Process `
        -FilePath $backendExe `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $backendOutLog `
        -RedirectStandardError $backendErrLog `
        -PassThru

    Wait-ForProcessHttp "$backendUrl/healthz" $TimeoutSeconds $backendStarted $backendErrLog "Backend warmup live smoke process" | Out-Null

    $warmupCurrent = Invoke-RestMethod -Uri "$backendUrl/api/live/current"
    Assert-Equal $warmupCurrent.availability "active" "warmup live current availability"
    Assert-Truthy $warmupCurrent.active "warmup live current active"
    $warmupSessionKey = [long]$warmupCurrent.session.session_key
    Assert-HttpStatus "POST" "$backendUrl/api/sessions/$warmupSessionKey/live/start" 503 "warmup live start"

    Stop-SmokeProcess ([ref]$backendStarted)
    Stop-SmokeProcess ([ref]$mockStarted)

    $missingEndMockPort = $OpenF1MockPort + 5
    $missingEndMockBaseUrl = "http://127.0.0.1:$missingEndMockPort/v1/"
    $mockStarted = Start-Process `
        -FilePath "python" `
        -ArgumentList @($mockScript, "--host", "127.0.0.1", "--port", "$missingEndMockPort", "--omit-session-end") `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $mockOutLog `
        -RedirectStandardError $mockErrLog `
        -PassThru

    Wait-ForProcessHttp "$missingEndMockBaseUrl/meetings" $TimeoutSeconds $mockStarted $mockErrLog "OpenF1 missing-end mock process" | Out-Null

    $env:DATABASE_URL = $DatabaseUrl
    $env:INTERVAL_BIND = $BackendBind
    $env:INTERVAL_OPENF1_LIVE_ENABLED = "true"
    $env:INTERVAL_OPENF1_LIVE_BASE_URL = $missingEndMockBaseUrl

    $backendStarted = Start-Process `
        -FilePath $backendExe `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $backendOutLog `
        -RedirectStandardError $backendErrLog `
        -PassThru

    Wait-ForProcessHttp "$backendUrl/healthz" $TimeoutSeconds $backendStarted $backendErrLog "Backend missing-end live smoke process" | Out-Null

    $missingEndCurrent = Invoke-RestMethod -Uri "$backendUrl/api/live/current"
    Assert-Equal $missingEndCurrent.availability "active" "missing-end live current availability"
    Assert-Truthy $missingEndCurrent.active "missing-end live current active"
    $missingEndSessionKey = [long]$missingEndCurrent.session.session_key

    Invoke-RestMethod -Method Post -Uri "$backendUrl/api/sessions/$missingEndSessionKey/live/start" | Out-Null
    $missingEndMetadata = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$missingEndSessionKey/live/metadata"
    Assert-Equal $missingEndMetadata.max_t 10800 "missing-end live metadata default max_t"

    Stop-SmokeProcess ([ref]$backendStarted)
    Stop-SmokeProcess ([ref]$mockStarted)

    $failureMockPort = $OpenF1MockPort + 4
    $failureMockBaseUrl = "http://127.0.0.1:$failureMockPort/v1/"
    $mockStarted = Start-Process `
        -FilePath "python" `
        -ArgumentList @($mockScript, "--host", "127.0.0.1", "--port", "$failureMockPort", "--failed-endpoints", "drivers") `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $mockOutLog `
        -RedirectStandardError $mockErrLog `
        -PassThru

    Wait-ForProcessHttp "$failureMockBaseUrl/meetings" $TimeoutSeconds $mockStarted $mockErrLog "OpenF1 required-endpoint failure mock process" | Out-Null

    $env:DATABASE_URL = $DatabaseUrl
    $env:INTERVAL_BIND = $BackendBind
    $env:INTERVAL_OPENF1_LIVE_ENABLED = "true"
    $env:INTERVAL_OPENF1_LIVE_BASE_URL = $failureMockBaseUrl

    $backendStarted = Start-Process `
        -FilePath $backendExe `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $backendOutLog `
        -RedirectStandardError $backendErrLog `
        -PassThru

    Wait-ForProcessHttp "$backendUrl/healthz" $TimeoutSeconds $backendStarted $backendErrLog "Backend required-endpoint failure live smoke process" | Out-Null

    $failureCurrent = Invoke-RestMethod -Uri "$backendUrl/api/live/current"
    Assert-Equal $failureCurrent.availability "active" "required-endpoint failure live current availability"
    Assert-Truthy $failureCurrent.active "required-endpoint failure live current active"
    $failureSessionKey = [long]$failureCurrent.session.session_key
    Assert-HttpStatus "POST" "$backendUrl/api/sessions/$failureSessionKey/live/start" 502 "required-endpoint failure live start"

    Stop-SmokeProcess ([ref]$backendStarted)
    Stop-SmokeProcess ([ref]$mockStarted)

    $malformedRefreshMockPort = $OpenF1MockPort + 6
    $malformedRefreshMockBaseUrl = "http://127.0.0.1:$malformedRefreshMockPort/v1/"
    $mockStarted = Start-Process `
        -FilePath "python" `
        -ArgumentList @($mockScript, "--host", "127.0.0.1", "--port", "$malformedRefreshMockPort", "--malformed-after-first-endpoints", "location") `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $mockOutLog `
        -RedirectStandardError $mockErrLog `
        -PassThru

    Wait-ForProcessHttp "$malformedRefreshMockBaseUrl/meetings" $TimeoutSeconds $mockStarted $mockErrLog "OpenF1 malformed-refresh mock process" | Out-Null

    $env:DATABASE_URL = $DatabaseUrl
    $env:INTERVAL_BIND = $BackendBind
    $env:INTERVAL_OPENF1_LIVE_ENABLED = "true"
    $env:INTERVAL_OPENF1_LIVE_BASE_URL = $malformedRefreshMockBaseUrl

    $backendStarted = Start-Process `
        -FilePath $backendExe `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $backendOutLog `
        -RedirectStandardError $backendErrLog `
        -PassThru

    Wait-ForProcessHttp "$backendUrl/healthz" $TimeoutSeconds $backendStarted $backendErrLog "Backend malformed-refresh live smoke process" | Out-Null

    $malformedRefreshCurrent = Invoke-RestMethod -Uri "$backendUrl/api/live/current"
    Assert-Equal $malformedRefreshCurrent.availability "active" "malformed-refresh live current availability"
    Assert-Truthy $malformedRefreshCurrent.active "malformed-refresh live current active"
    $malformedRefreshSessionKey = [long]$malformedRefreshCurrent.session.session_key

    Invoke-RestMethod -Method Post -Uri "$backendUrl/api/sessions/$malformedRefreshSessionKey/live/start" | Out-Null
    Start-Sleep -Milliseconds 650
    $malformedRefreshSnapshot = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$malformedRefreshSessionKey/live/snapshot"
    Assert-Equal $malformedRefreshSnapshot.contract_version "replay.v1" "malformed-refresh live snapshot contract"
    Assert-Equal $malformedRefreshSnapshot.cursor.session_key $malformedRefreshSessionKey "malformed-refresh live snapshot session"
    Assert-CountAtLeast $malformedRefreshSnapshot.track.positions 1 "malformed-refresh live preserved track positions"
    $malformedRefreshStatus = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$malformedRefreshSessionKey/live/status"
    $refreshChannel = $malformedRefreshStatus.channels | Where-Object { $_.endpoint -eq "refresh" } | Select-Object -First 1
    Assert-Truthy $refreshChannel "malformed-refresh synthetic channel"
    Assert-Equal $refreshChannel.state "failed" "malformed-refresh synthetic channel state"

    Stop-SmokeProcess ([ref]$backendStarted)
    Stop-SmokeProcess ([ref]$mockStarted)

    $mockStarted = Start-Process `
        -FilePath "python" `
        -ArgumentList @($mockScript, "--host", "127.0.0.1", "--port", "$OpenF1MockPort") `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $mockOutLog `
        -RedirectStandardError $mockErrLog `
        -PassThru

    Wait-ForProcessHttp "$OpenF1MockBaseUrl/meetings" $TimeoutSeconds $mockStarted $mockErrLog "OpenF1 live mock process" | Out-Null

    $env:DATABASE_URL = $DatabaseUrl
    $env:INTERVAL_BIND = $BackendBind
    $env:INTERVAL_OPENF1_LIVE_ENABLED = "true"
    $env:INTERVAL_OPENF1_LIVE_BASE_URL = $OpenF1MockBaseUrl

    $backendStarted = Start-Process `
        -FilePath $backendExe `
        -WorkingDirectory $root `
        -WindowStyle Hidden `
        -RedirectStandardOutput $backendOutLog `
        -RedirectStandardError $backendErrLog `
        -PassThru

    Wait-ForProcessHttp "$backendUrl/healthz" $TimeoutSeconds $backendStarted $backendErrLog "Backend live smoke process" | Out-Null

    $current = Invoke-RestMethod -Uri "$backendUrl/api/live/current"
    Assert-Equal $current.availability "active" "live current availability"
    Assert-Truthy $current.active "live current active"
    $sessionKey = [long]$current.session.session_key
    Assert-Truthy ($sessionKey -gt 0) "live current session key"

    $notStartedOutput = & powershell -ExecutionPolicy Bypass -File $liveCheckScript -BackendUrl $backendUrl -RequireActive -ExpectedSessionKey $sessionKey
    if ($LASTEXITCODE -ne 0) {
        throw "check-live-current.ps1 active-not-started diagnostic failed with exit code $LASTEXITCODE"
    }
    if (-not (($notStartedOutput -join [Environment]::NewLine).Contains("runtime: not started"))) {
        throw "check-live-current.ps1 active-not-started diagnostic did not explain missing runtime state"
    }

    $start = Invoke-RestMethod -Method Post -Uri "$backendUrl/api/sessions/$sessionKey/live/start"
    Assert-Truthy $start.active "live start active"
    Assert-Equal $start.source "openf1_live" "live start source"
    Assert-CountAtLeast $start.channels 8 "live start channels"
    Assert-GreaterThan $start.current_t 60 "live start synced current time"

    $metadata = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$sessionKey/live/metadata"
    Assert-Equal $metadata.contract_version "replay.v1" "live metadata contract"
    Assert-Equal $metadata.session.session_key $sessionKey "live metadata session"
    Assert-Equal $metadata.data_sources[0].name "openf1_live" "live metadata source"
    Assert-Equal $metadata.frame_step_seconds 0.5 "live metadata frame step"
    Assert-Equal $metadata.endpoints.snapshot_endpoint "/api/sessions/$sessionKey/live/snapshot" "live metadata snapshot endpoint"
    Assert-Equal $metadata.endpoints.stream_endpoint "/api/sessions/$sessionKey/live/stream" "live metadata stream endpoint"
    Assert-Equal $metadata.endpoints.events_endpoint "/api/sessions/$sessionKey/live/events" "live metadata events endpoint"
    Assert-Equal $metadata.endpoints.track_geometry_endpoint "/api/sessions/$sessionKey/live/track/geometry" "live metadata track geometry endpoint"

    $snapshot = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$sessionKey/live/snapshot"
    Assert-Equal $snapshot.contract_version "replay.v1" "live snapshot contract"
    Assert-Equal $snapshot.cursor.session_key $sessionKey "live snapshot session"
    Assert-CountAtLeast $snapshot.timing.rows 2 "live timing rows"
    Assert-CountAtLeast $snapshot.track.positions 2 "live track positions"
    Assert-GreaterThan $snapshot.cursor.t 60 "live snapshot synced current time"
    Assert-Equal $snapshot.timing.rows[0].position 1 "live leader position"
    Assert-Equal $snapshot.timing.rows[1].gap_to_leader "+2.431" "live follower gap"

    $status = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$sessionKey/live/status"
    Assert-Truthy $status.active "live status active"
    Assert-Equal $status.source "openf1_live" "live status source"
    Assert-Truthy $status.updated_at "live status updated_at"
    Assert-GreaterThan $status.current_t 60 "live status synced current time"
    $freshChannels = $status.channels | Where-Object { $_.state -eq "fresh" -or $_.state -eq "cached" }
    Assert-CountAtLeast $freshChannels 8 "live fresh or cached channels"
    $pitChannel = $status.channels | Where-Object { $_.endpoint -eq "pit" } | Select-Object -First 1
    Assert-Truthy $pitChannel "live pit channel health"
    Assert-Equal $pitChannel.state "fresh" "live empty optional pit channel state"
    Assert-Equal $pitChannel.rows 0 "live empty optional pit channel rows"

    $geometry = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$sessionKey/live/track/geometry"
    Assert-Equal $geometry.contract_version "replay.v1" "live geometry contract"
    Assert-Equal $geometry.session_key $sessionKey "live geometry session"
    Assert-CountAtLeast $geometry.centerline 20 "live geometry centerline"

    $events = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$sessionKey/live/events"
    Assert-Equal $events.contract_version "replay.v1" "live events contract"
    Assert-CountAtLeast $events.events 1 "live events"

    & powershell -ExecutionPolicy Bypass -File $liveCheckScript -BackendUrl $backendUrl -Start -RequireActive -Strict -MaxUpdateAgeSeconds 30 -MinTimingRows 2 -MinTrackPositions 2 -MinEvents 1 -MinGeometryPoints 20 -ExpectedSessionKey $sessionKey -ExpectedMeetingName "Test Live Grand Prix" -ExpectedSessionType race -WatchSeconds 1 -IntervalSeconds 1 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "check-live-current.ps1 failed with exit code $LASTEXITCODE"
    }

    Assert-LiveStream "$backendUrl/api/sessions/$sessionKey/live/stream" $TimeoutSeconds $sessionKey

    $frontendStarted = Start-Process `
        -FilePath "node" `
        -ArgumentList @($viteEntry, "--host", "127.0.0.1", "--port", "$FrontendPort", "--strictPort") `
        -WorkingDirectory $frontendDir `
        -WindowStyle Hidden `
        -RedirectStandardOutput $frontendOutLog `
        -RedirectStandardError $frontendErrLog `
        -PassThru

    Wait-ForProcessHttp "$frontendUrl/healthz" $TimeoutSeconds $frontendStarted $frontendErrLog "Frontend live smoke process" | Out-Null

    $proxiedCurrent = Invoke-RestMethod -Uri "$frontendUrl/api/live/current"
    Assert-Equal $proxiedCurrent.availability "active" "proxied live current availability"
    Assert-Equal $proxiedCurrent.session.session_key $sessionKey "proxied live current session"

    $proxiedStatus = Invoke-RestMethod -Uri "$frontendUrl/api/sessions/$sessionKey/live/status"
    Assert-Truthy $proxiedStatus.active "proxied live status active"

    $proxiedMetadata = Invoke-RestMethod -Uri "$frontendUrl/api/sessions/$sessionKey/live/metadata"
    Assert-Equal $proxiedMetadata.session.session_key $sessionKey "proxied live metadata session"

    $proxiedStop = Invoke-RestMethod -Method Post -Uri "$frontendUrl/api/sessions/$sessionKey/live/stop"
    Assert-Equal $proxiedStop.active $false "proxied live stop inactive"

    $stoppedStatus = Invoke-RestMethod -Uri "$backendUrl/api/sessions/$sessionKey/live/status"
    Assert-Equal $stoppedStatus.active $false "backend live status inactive after proxied stop"

    Write-Output "Live smoke passed: OpenF1 mock, pre-session wait, post-session clamp, Sprint current/start/metadata/snapshot, missing date_end default duration, warmup 503 retry state, required endpoint 502 failure state, malformed refresh fallback, live diagnostic, backend live current/start/metadata/snapshot/status/events/geometry/stream/stop, and Vite proxy are healthy."
} finally {
    Remove-Item Env:\INTERVAL_OPENF1_LIVE_ENABLED -ErrorAction SilentlyContinue
    Remove-Item Env:\INTERVAL_OPENF1_LIVE_BASE_URL -ErrorAction SilentlyContinue
    Stop-SmokeProcess ([ref]$frontendStarted)
    Stop-SmokeProcess ([ref]$backendStarted)
    Stop-SmokeProcess ([ref]$mockStarted)
}
