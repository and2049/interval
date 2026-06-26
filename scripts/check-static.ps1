param()

$ErrorActionPreference = "Stop"

trap {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}

$scriptDir = $PSScriptRoot
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

Write-Output "Static checks passed."
