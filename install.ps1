#Requires -Version 5.0
[CmdletBinding()]
param(
    [Alias("v")]
    [string]$Version,
    [switch]$NoModifyPath,
    [switch]$Help
)

$ErrorActionPreference = "Stop"
$App = "interval-desktop"
$Repo = "and2049/interval"
$InstallDir = if ($env:INTERVAL_INSTALL_DIR) { $env:INTERVAL_INSTALL_DIR } else { Join-Path $env:USERPROFILE ".interval\bin" }

function Write-Info { param([string]$msg) Write-Host $msg -ForegroundColor Gray }

# Runs as a function so early exits use `return`: `exit` would close the caller's
# terminal when the script is piped into `iex`.
function Install-Interval {
    if ($Help) {
        Write-Host @"
Interval Installer

Usage: install.ps1 [options]

Options:
    -Version <version>   Install a specific version (e.g. 26-9-6.0)
    -NoModifyPath        Don't add interval to the user PATH
    -Help                Display this help message

Examples:
    irm https://github.com/$Repo/releases/latest/download/install.ps1 | iex
    & ([scriptblock]::Create((irm https://github.com/$Repo/releases/latest/download/install.ps1))) -Version 26-9-6.0
"@
        return
    }

    $arch = $env:PROCESSOR_ARCHITECTURE
    if ($arch -ne "AMD64") {
        throw "No prebuilt binary for windows-$arch. Build from source: cargo build --release -p $App"
    }
    $filename = "$App-windows-x64.zip"

    if ($Version) {
        $Version = $Version -replace '^v', ''
        $url = "https://github.com/$Repo/releases/download/v$Version/$filename"
    } else {
        try {
            $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -UseBasicParsing
            $Version = $release.tag_name -replace '^v', ''
        } catch {
            throw "Failed to fetch version information"
        }
        $url = "https://github.com/$Repo/releases/latest/download/$filename"
    }

    $destBinary = Join-Path $InstallDir "$App.exe"
    $versionFile = Join-Path $InstallDir "version"
    if ((Test-Path $destBinary) -and (Test-Path $versionFile) -and ((Get-Content $versionFile -Raw).Trim() -eq $Version)) {
        Write-Info "Version $Version already installed"
        return
    }

    Write-Info "Installing interval version: $Version"
    $tmpDir = Join-Path $env:TEMP "interval_install_$PID"
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null
    try {
        $archivePath = Join-Path $tmpDir $filename
        try {
            Invoke-WebRequest -Uri $url -OutFile $archivePath -UseBasicParsing
        } catch {
            throw "Failed to download $url`nAvailable releases: https://github.com/$Repo/releases"
        }
        Expand-Archive -Path $archivePath -DestinationPath $tmpDir -Force
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

        $oldBinary = "$destBinary.old"
        Remove-Item -Path $oldBinary -Force -ErrorAction SilentlyContinue
        if (Test-Path $destBinary) {
            Move-Item -Path $destBinary -Destination $oldBinary -Force
        }
        Move-Item -Path (Join-Path $tmpDir "$App.exe") -Destination $destBinary -Force
        Set-Content -Path $versionFile -Value $Version -NoNewline
    } finally {
        Remove-Item -Path $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
    }

    $shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut(
        (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Interval.lnk"))
    $shortcut.TargetPath = $destBinary
    $shortcut.WorkingDirectory = $InstallDir
    $shortcut.Save()

    if (-not $NoModifyPath) {
        $userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
        if (($userPath -split ';') -notcontains $InstallDir) {
            $newPath = if ($userPath) { "$InstallDir;$userPath" } else { $InstallDir }
            [Environment]::SetEnvironmentVariable("PATH", $newPath, "User")
            Write-Info "Added $InstallDir to the user PATH. Restart your terminal for the change to take effect."
        }
    }

    if ($env:GITHUB_ACTIONS -eq "true" -and $env:GITHUB_PATH) {
        Add-Content -Path $env:GITHUB_PATH -Value $InstallDir
    }

    Write-Host ""
    Write-Info "interval installed successfully!"
    Write-Host ""
    Write-Info "$App  # Open the dashboard"
    Write-Host ""
    Write-Info "For more information visit https://github.com/$Repo"
    Write-Host ""
}

Install-Interval
