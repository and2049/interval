param()

$ErrorActionPreference = "Stop"

trap {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}

$scriptDir = $PSScriptRoot
$root = Split-Path -Parent $scriptDir
$scripts = Get-ChildItem -LiteralPath $scriptDir -Filter "*.ps1" | Sort-Object Name

foreach ($script in $scripts) {
    $errors = $null
    [System.Management.Automation.Language.Parser]::ParseFile(
        $script.FullName,
        [ref]$null,
        [ref]$errors
    ) | Out-Null

    if ($errors.Count -gt 0) {
        Write-Output "$($script.Name) parse failed"
        foreach ($error in $errors) {
            Write-Output "  $($error.Extent.StartLineNumber):$($error.Extent.StartColumnNumber) $($error.Message)"
        }
        exit 1
    }

    Write-Output "$($script.Name) parse OK"
}

$liveCheckScript = Join-Path $scriptDir "check-live-current.ps1"
if (Test-Path -LiteralPath $liveCheckScript) {
    $help = Get-Help $liveCheckScript -Detailed | Out-String -Width 200
    if (
        -not $help.Contains("-Strict") `
            -or -not $help.Contains("-WatchSeconds 900 -Strict") `
            -or -not $help.Contains("-ExpectedMeetingName") `
            -or -not $help.Contains("-ExpectedSessionType") `
            -or -not $help.Contains("-AllowedBadChannels") `
            -or -not $help.Contains("-MinEvents 1")
    ) {
        throw "check-live-current.ps1 help must document strict live validation"
    }
    Write-Output "check-live-current.ps1 help OK"
}

$docsToCheck = @(
    (Get-Item -LiteralPath (Join-Path $root "README.md")),
    (Get-ChildItem -LiteralPath (Join-Path $root "docs") -Filter "*.md" -Recurse),
    (Get-ChildItem -LiteralPath (Join-Path $root "shared\contracts") -Filter "*.md" -Recurse)
)
$forbiddenLivePhrases = @(
    "live mode is deferred",
    "future live data",
    "experimental feature"
)
foreach ($phrase in $forbiddenLivePhrases) {
    $matches = $docsToCheck | Select-String -Pattern $phrase -SimpleMatch -ErrorAction SilentlyContinue
    if ($matches) {
        throw "stale live-mode wording found: '$phrase'"
    }
}
Write-Output "Live wording check OK"

Write-Output "Static checks passed."
