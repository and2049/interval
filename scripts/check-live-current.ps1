<#
.SYNOPSIS
Checks and optionally starts the current OpenF1 live race or sprint session.

.DESCRIPTION
Calls the backend live endpoints, prints availability/session/runtime status, and can validate that
the live dashboard contract is healthy enough for a real race-window check.

.PARAMETER Strict
Enables the recommended real-session assertions:
failed/stale/missing critical-channel failure, max runtime update age of 15 seconds, at least 10
timing rows, at least 10 track positions, and live snapshot clock sync within 30 seconds of the
OpenF1 session clock. Add -MinGeometryPoints 20 once cached, historical, or accumulated live
location geometry is expected, and add -MinEvents 1 when validating after race-control, pit,
weather, or derived events are expected.

.PARAMETER AllowedBadChannels
Live endpoints that may be missing, stale, or failed without failing strict validation. Defaults to
optional/degradable feeds that are useful when present but should not block a live race check.

.PARAMETER ExpectedMeetingName
Fails if the active OpenF1 live meeting name does not exactly match this value.

.PARAMETER ExpectedSessionType
Fails if the active OpenF1 live session type does not match this value, for example race or sprint.

.PARAMETER ExpectedNextMeetingName
Fails if the next OpenF1 live meeting name does not exactly match this value when no live session is active.

.PARAMETER ExpectedNextSessionType
Fails if the next OpenF1 live session type does not match this value when no live session is active.

.PARAMETER MaxSnapshotClockLagSeconds
Fails if the live snapshot cursor is not synced to the OpenF1 session clock within this many seconds.
Strict mode defaults this to 30 seconds.

.PARAMETER WaitForActiveSeconds
Polls /api/live/current until a live race or sprint becomes active, then runs the requested checks.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File scripts/check-live-current.ps1 -Start -RequireActive -Strict

Starts the active OpenF1 live session if needed and runs the standard live dashboard health checks.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File scripts/check-live-current.ps1 -Start -RequireActive -Strict -ExpectedMeetingName "British Grand Prix" -ExpectedSessionType sprint

Starts/checks live mode and fails fast if OpenF1 reports a different meeting or session type.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File scripts/check-live-current.ps1 -WatchSeconds 900 -Strict

Watches an already-started live session for 15 minutes and fails if the live dashboard becomes unhealthy.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File scripts/check-live-current.ps1 -ExpectedNextMeetingName "British Grand Prix" -ExpectedNextSessionType sprint

Checks that OpenF1 reports the expected next live race or sprint before the session window opens.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File scripts/check-live-current.ps1 -WaitForActiveSeconds 28800 -Start -RequireActive -Strict

Waits up to eight hours for the next live race or sprint, starts it, and runs strict validation.
#>

param(
    [string]$BackendUrl = "http://127.0.0.1:4000",
    [switch]$Start,
    [switch]$RequireActive,
    [switch]$Strict,
    [switch]$FailOnBadChannels,
    [int]$MaxUpdateAgeSeconds = 0,
    [int]$MinTimingRows = 0,
    [int]$MinTrackPositions = 0,
    [int]$MinEvents = 0,
    [int]$MinGeometryPoints = 0,
    [int]$MaxSnapshotClockLagSeconds = 0,
    [string[]]$AllowedBadChannels = @("intervals", "pit", "race_control", "stints", "weather", "session_result"),
    [long]$ExpectedSessionKey = 0,
    [string]$ExpectedMeetingName = "",
    [string]$ExpectedSessionType = "",
    [long]$ExpectedNextSessionKey = 0,
    [string]$ExpectedNextMeetingName = "",
    [string]$ExpectedNextSessionType = "",
    [int]$WatchSeconds = 0,
    [int]$WaitForActiveSeconds = 0,
    [int]$IntervalSeconds = 10
)

$ErrorActionPreference = "Stop"

trap {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}

if ($Strict) {
    $FailOnBadChannels = $true
    if ($MaxUpdateAgeSeconds -le 0) { $MaxUpdateAgeSeconds = 15 }
    if ($MinTimingRows -le 0) { $MinTimingRows = 10 }
    if ($MinTrackPositions -le 0) { $MinTrackPositions = 10 }
    if ($MaxSnapshotClockLagSeconds -le 0) { $MaxSnapshotClockLagSeconds = 30 }
    Write-Output "strict: enabled max_update_age=${MaxUpdateAgeSeconds}s max_snapshot_clock_lag=${MaxSnapshotClockLagSeconds}s min_timing_rows=$MinTimingRows min_track_positions=$MinTrackPositions min_events=$MinEvents min_geometry_points=$MinGeometryPoints"
}

function Request-Json($method, $path) {
    $uri = "$($BackendUrl.TrimEnd('/'))$path"
    if ($method -eq "POST") {
        return Invoke-RestMethod -Method Post -Uri $uri
    }
    Invoke-RestMethod -Uri $uri
}

function Write-ChannelSummary($channels) {
    if (-not $channels) {
        Write-Output "channels: <none>"
        return
    }
    $channels |
        Select-Object endpoint, state, rows, age_seconds, last_error |
        Format-Table -AutoSize |
        Out-String |
        Write-Output
}

function Assert-LiveChannelsHealthy($channels, $allowedBadChannels) {
    if (-not $channels) {
        throw "expected live channel status but none was reported"
    }

    $allowed = @{}
    foreach ($channel in $allowedBadChannels) {
        if ($channel) {
            $allowed[$channel.ToLowerInvariant()] = $true
        }
    }

    $bad = @($channels | Where-Object {
        @("failed", "stale", "missing") -contains $_.state `
            -and -not $allowed.ContainsKey(([string]$_.endpoint).ToLowerInvariant())
    })
    $ignored = @($channels | Where-Object {
        @("failed", "stale", "missing") -contains $_.state `
            -and $allowed.ContainsKey(([string]$_.endpoint).ToLowerInvariant())
    })
    if ($bad.Count -eq 0) {
        if ($ignored.Count -gt 0) {
            $ignoredSummary = ($ignored | ForEach-Object { "$($_.endpoint):$($_.state)" }) -join "; "
            Write-Output "optional channel degradation ignored: $ignoredSummary"
        }
        return
    }

    $summary = ($bad | ForEach-Object {
        $text = "$($_.endpoint):$($_.state)"
        if ($_.last_error) {
            $text = "$text ($($_.last_error))"
        }
        $text
    }) -join "; "
    throw "live channel health degraded: $summary"
}

function Assert-LiveRuntimeFresh($status, $maxAgeSeconds) {
    if ($maxAgeSeconds -le 0) {
        return
    }
    if (-not $status.updated_at) {
        throw "expected live status updated_at for runtime freshness check"
    }

    try {
        $updatedAt = [DateTimeOffset]::Parse([string]$status.updated_at).ToUniversalTime()
    } catch {
        throw "could not parse live status updated_at: $($status.updated_at)"
    }

    $ageSeconds = ([DateTimeOffset]::UtcNow - $updatedAt).TotalSeconds
    if ($ageSeconds -gt $maxAgeSeconds) {
        throw ("live runtime update is stale: {0:N1}s old, max {1}s" -f $ageSeconds, $maxAgeSeconds)
    }
    Write-Output ("runtime freshness: ok ({0:N1}s old)" -f ([Math]::Max(0, $ageSeconds)))
}

function Assert-LiveSnapshotClockSynced($snapshot, $metadata, $sessionStartTime, $maxLagSeconds) {
    if ($maxLagSeconds -le 0) {
        return
    }
    if (-not $sessionStartTime) {
        throw "expected live session start_time for snapshot clock sync check"
    }

    try {
        $sessionStart = [DateTimeOffset]::Parse([string]$sessionStartTime).ToUniversalTime()
    } catch {
        throw "could not parse live session start_time: $sessionStartTime"
    }

    $expectedT = ([DateTimeOffset]::UtcNow - $sessionStart).TotalSeconds
    $expectedT = [Math]::Max(0, $expectedT)
    if ($metadata.max_t -gt 0) {
        $expectedT = [Math]::Min([double]$metadata.max_t, $expectedT)
    }
    $lagSeconds = [Math]::Abs(([double]$snapshot.cursor.t) - $expectedT)
    if ($lagSeconds -gt $maxLagSeconds) {
        throw ("live snapshot clock lag is {0:N1}s, max {1}s" -f $lagSeconds, $maxLagSeconds)
    }
    Write-Output ("snapshot clock: ok ({0:N1}s lag)" -f $lagSeconds)
}

function Assert-LiveSnapshotContent($snapshot, $minTimingRows, $minTrackPositions) {
    $timingRows = $snapshot.timing.rows.Count
    $trackPositions = $snapshot.track.positions.Count
    if ($minTimingRows -gt 0 -and $timingRows -lt $minTimingRows) {
        throw "live snapshot has $timingRows timing rows, expected at least $minTimingRows"
    }
    if ($minTrackPositions -gt 0 -and $trackPositions -lt $minTrackPositions) {
        throw "live snapshot has $trackPositions track positions, expected at least $minTrackPositions"
    }
    if ($minTimingRows -gt 0 -or $minTrackPositions -gt 0) {
        Write-Output "snapshot content: ok"
    }
}

function Assert-LiveEventFeed($events, $minEvents) {
    if ($minEvents -le 0) {
        return
    }
    $eventCount = $events.events.Count
    if ($eventCount -lt $minEvents) {
        throw "live event feed has $eventCount events, expected at least $minEvents"
    }
    Write-Output "event feed: ok"
}

function Assert-LiveGeometry($geometry, $minGeometryPoints) {
    if ($minGeometryPoints -le 0) {
        return
    }
    $pointCount = $geometry.centerline.Count
    if ($pointCount -lt $minGeometryPoints) {
        throw "live geometry has $pointCount centerline points, expected at least $minGeometryPoints"
    }
    Write-Output "geometry: ok"
}

function Assert-LiveContract($metadata, $snapshot, $events, $geometry) {
    if ($metadata.contract_version -ne "replay.v1") {
        throw "live metadata contract_version was '$($metadata.contract_version)', expected replay.v1"
    }
    if ($metadata.data_sources[0].name -ne "openf1_live") {
        throw "live metadata source was '$($metadata.data_sources[0].name)', expected openf1_live"
    }
    if ($snapshot.contract_version -ne "replay.v1") {
        throw "live snapshot contract_version was '$($snapshot.contract_version)', expected replay.v1"
    }
    if ($events -and $events.contract_version -ne "replay.v1") {
        throw "live events contract_version was '$($events.contract_version)', expected replay.v1"
    }
    if ($geometry -and $geometry.contract_version -ne "replay.v1") {
        throw "live geometry contract_version was '$($geometry.contract_version)', expected replay.v1"
    }
    Write-Output "contract: ok"
}

function Write-LiveSnapshotSummary {
    param(
        [long]$SessionKey,
        [switch]$FailOnBadChannels,
        [int]$MaxUpdateAgeSeconds = 0,
        [int]$MinTimingRows = 0,
        [int]$MinTrackPositions = 0,
        [int]$MinEvents = 0,
        [int]$MinGeometryPoints = 0,
        [int]$MaxSnapshotClockLagSeconds = 0,
        [string]$SessionStartTime = "",
        [string[]]$AllowedBadChannels = @()
    )

    $metadata = Request-Json "GET" "/api/sessions/$sessionKey/live/metadata"
    $snapshot = Request-Json "GET" "/api/sessions/$sessionKey/live/snapshot"
    $status = Request-Json "GET" "/api/sessions/$sessionKey/live/status"
    $events = if ($MinEvents -gt 0) { Request-Json "GET" "/api/sessions/$sessionKey/live/events" } else { $null }
    $geometry = if ($MinGeometryPoints -gt 0) { Request-Json "GET" "/api/sessions/$sessionKey/live/track/geometry" } else { $null }

    Assert-LiveContract $metadata $snapshot $events $geometry
    Write-Output "metadata: source=$($metadata.data_sources[0].name) frame_step=$($metadata.frame_step_seconds) max_t=$($metadata.max_t)"
    Write-Output "snapshot: t=$($snapshot.cursor.t) lap=$($snapshot.race_state.lap) timing_rows=$($snapshot.timing.rows.Count) track_positions=$($snapshot.track.positions.Count)"
    if ($events) {
        Write-Output "events: count=$($events.events.Count)"
    }
    if ($geometry) {
        Write-Output "geometry: centerline_points=$($geometry.centerline.Count) source=$($geometry.source) quality=$($geometry.quality)"
    }
    Write-Output "status: active=$($status.active) current_t=$($status.current_t) updated_at=$($status.updated_at)"
    Write-ChannelSummary $status.channels
    Assert-LiveSnapshotContent $snapshot $MinTimingRows $MinTrackPositions
    Assert-LiveEventFeed $events $MinEvents
    Assert-LiveGeometry $geometry $MinGeometryPoints
    Assert-LiveSnapshotClockSynced $snapshot $metadata $SessionStartTime $MaxSnapshotClockLagSeconds
    Assert-LiveRuntimeFresh $status $MaxUpdateAgeSeconds
    if ($FailOnBadChannels) {
        Assert-LiveChannelsHealthy $status.channels $AllowedBadChannels
        Write-Output "channel health: ok"
    }
}

$current = Request-Json "GET" "/api/live/current"
Write-Output "availability: $($current.availability)"
Write-Output "active: $($current.active)"

if ($current.session) {
    Write-Output "session: $($current.session.year) $($current.session.name) [$($current.session.session_type)] key=$($current.session.session_key)"
}
if ($current.meeting) {
    Write-Output "meeting: $($current.meeting.name) - $($current.meeting.location), $($current.meeting.country)"
}
if ($current.next_session) {
    Write-Output "next: $($current.next_session.year) $($current.next_session.name) [$($current.next_session.session_type)] key=$($current.next_session.session_key) start=$($current.next_session.start_time)"
}
if ($current.message) {
    Write-Output "message: $($current.message)"
}
if ($current.status) {
    Write-Output "status current_t=$($current.status.current_t) updated_at=$($current.status.updated_at)"
    Write-ChannelSummary $current.status.channels
}

if ((-not $current.active -or -not $current.session) -and $WaitForActiveSeconds -gt 0) {
    $deadline = (Get-Date).AddSeconds($WaitForActiveSeconds)
    Write-Output "waiting for live session: up to ${WaitForActiveSeconds}s"
    while ((Get-Date) -lt $deadline -and (-not $current.active -or -not $current.session)) {
        Start-Sleep -Seconds ([Math]::Max(1, $IntervalSeconds))
        $current = Request-Json "GET" "/api/live/current"
        Write-Output "availability: $($current.availability) active=$($current.active)"
        if ($current.next_session) {
            Write-Output "next: $($current.next_session.year) $($current.next_session.name) [$($current.next_session.session_type)] key=$($current.next_session.session_key) start=$($current.next_session.start_time)"
        }
    }
    if (-not $current.active -or -not $current.session) {
        throw "timed out waiting ${WaitForActiveSeconds}s for an active OpenF1 live session"
    }
    Write-Output "session: $($current.session.year) $($current.session.name) [$($current.session.session_type)] key=$($current.session.session_key)"
    if ($current.meeting) {
        Write-Output "meeting: $($current.meeting.name) - $($current.meeting.location), $($current.meeting.country)"
    }
}

if (-not $current.active -or -not $current.session) {
    if ($ExpectedNextSessionKey -gt 0 -and (-not $current.next_session -or [long]$current.next_session.session_key -ne $ExpectedNextSessionKey)) {
        throw "expected next live session $ExpectedNextSessionKey but got $($current.next_session.session_key)"
    }
    if ($ExpectedNextMeetingName.Trim() -and (-not $current.next_meeting -or $current.next_meeting.name -ne $ExpectedNextMeetingName)) {
        throw "expected next live meeting '$ExpectedNextMeetingName' but got '$($current.next_meeting.name)'"
    }
    if ($ExpectedNextSessionType.Trim() -and (-not $current.next_session -or $current.next_session.session_type -ne $ExpectedNextSessionType)) {
        throw "expected next live session type '$ExpectedNextSessionType' but got '$($current.next_session.session_type)'"
    }
    if ($RequireActive) {
        throw "expected an active OpenF1 live session from /api/live/current"
    }
    exit 0
}

$sessionKey = [long]$current.session.session_key
if ($ExpectedSessionKey -gt 0 -and $sessionKey -ne $ExpectedSessionKey) {
    throw "expected active live session $ExpectedSessionKey but got $sessionKey"
}
if ($ExpectedMeetingName.Trim() -and (-not $current.meeting -or $current.meeting.name -ne $ExpectedMeetingName)) {
    throw "expected active live meeting '$ExpectedMeetingName' but got '$($current.meeting.name)'"
}
if ($ExpectedSessionType.Trim() -and $current.session.session_type -ne $ExpectedSessionType) {
    throw "expected active live session type '$ExpectedSessionType' but got '$($current.session.session_type)'"
}

if ($Start) {
    $startResponse = Request-Json "POST" "/api/sessions/$sessionKey/live/start"
    if ($RequireActive -and -not $startResponse.active) {
        throw "expected /api/sessions/$sessionKey/live/start to return active=true"
    }
    Write-Output "start: active=$($startResponse.active) source=$($startResponse.source) current_t=$($startResponse.current_t)"
} elseif (-not $current.status) {
    Write-Output "runtime: not started. Re-run with -Start to create live metadata, snapshot, and channel status."
    exit 0
}

Write-LiveSnapshotSummary -SessionKey $sessionKey -FailOnBadChannels:$FailOnBadChannels -MaxUpdateAgeSeconds $MaxUpdateAgeSeconds -MinTimingRows $MinTimingRows -MinTrackPositions $MinTrackPositions -MinEvents $MinEvents -MinGeometryPoints $MinGeometryPoints -MaxSnapshotClockLagSeconds $MaxSnapshotClockLagSeconds -SessionStartTime $current.session.start_time -AllowedBadChannels $AllowedBadChannels

if ($WatchSeconds -gt 0) {
    $deadline = (Get-Date).AddSeconds($WatchSeconds)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Seconds ([Math]::Max(1, $IntervalSeconds))
        Write-Output "---"
        Write-LiveSnapshotSummary -SessionKey $sessionKey -FailOnBadChannels:$FailOnBadChannels -MaxUpdateAgeSeconds $MaxUpdateAgeSeconds -MinTimingRows $MinTimingRows -MinTrackPositions $MinTrackPositions -MinEvents $MinEvents -MinGeometryPoints $MinGeometryPoints -MaxSnapshotClockLagSeconds $MaxSnapshotClockLagSeconds -SessionStartTime $current.session.start_time -AllowedBadChannels $AllowedBadChannels
    }
}
