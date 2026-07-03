param(
    [switch]$WithSmoke,
    [switch]$WithLiveSmoke,
    [switch]$SkipLiveSmoke
)

$ErrorActionPreference = "Stop"

trap {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}

$root = Split-Path -Parent $PSScriptRoot
$frontendDir = Join-Path $root "frontend"
$staticCheckScript = Join-Path $PSScriptRoot "check-static.ps1"
$smokeScript = Join-Path $PSScriptRoot "smoke-mvp.ps1"
$liveSmokeScript = Join-Path $PSScriptRoot "smoke-live.ps1"
$runLiveSmoke = $WithLiveSmoke -or -not $SkipLiveSmoke

function Assert-CommandAvailable($command, $installHint) {
    if (-not (Get-Command $command -ErrorAction SilentlyContinue)) {
        throw "Required command '$command' was not found. $installHint"
    }
}

function Assert-PathExists($path, $label) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "$label not found at $path"
    }
}

function Invoke-Step($label, $scriptBlock) {
    Write-Output ""
    Write-Output "==> $label"
    & $scriptBlock
}

function Invoke-CheckedCommand($command, $arguments) {
    & $command @arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$command $($arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

Assert-PathExists $frontendDir "Frontend directory"
Assert-PathExists $staticCheckScript "Static check script"
Assert-CommandAvailable "cargo" "Install Rust/Cargo and ensure it is on PATH."
Assert-CommandAvailable "bun" "Install Bun and ensure it is on PATH."
if ($WithSmoke) {
    Assert-PathExists $smokeScript "Smoke script"
}
if ($runLiveSmoke) {
    Assert-PathExists $liveSmokeScript "Live smoke script"
}

Invoke-Step "Script static checks" {
    Invoke-CheckedCommand "powershell" @("-ExecutionPolicy", "Bypass", "-File", $staticCheckScript)
}

Invoke-Step "Backend tests" {
    Push-Location $root
    try {
        Invoke-CheckedCommand "cargo" @("test")
    } finally {
        Pop-Location
    }
}

Invoke-Step "Frontend tests" {
    Push-Location $frontendDir
    try {
        Invoke-CheckedCommand "bun" @("run", "test")
    } finally {
        Pop-Location
    }
}

Invoke-Step "Frontend build" {
    Push-Location $frontendDir
    try {
        Invoke-CheckedCommand "bun" @("run", "build")
    } finally {
        Pop-Location
    }
}

if ($WithSmoke) {
    Invoke-Step "Cached MVP smoke" {
        Invoke-CheckedCommand "powershell" @("-ExecutionPolicy", "Bypass", "-File", $smokeScript)
    }
} else {
    Write-Output ""
    Write-Output "Skipping cached MVP smoke. Run with -WithSmoke after Bahrain 9472 is cached in interval.db."
}

if ($runLiveSmoke) {
    Invoke-Step "OpenF1 live smoke" {
        Invoke-CheckedCommand "powershell" @("-ExecutionPolicy", "Bypass", "-File", $liveSmokeScript)
    }
} else {
    Write-Output ""
    Write-Output "Skipping OpenF1 live smoke because -SkipLiveSmoke was provided."
}

Write-Output ""
Write-Output "Verification passed."
