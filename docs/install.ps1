# polyplugc installer for Windows
# Usage: powershell -c "irm https://polyplug.github.io/install.ps1 | iex"

param(
    [string]$InstallDir = "$env:USERPROFILE\.local\bin",
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$Repo = "polyplug/polyplug"
$BinaryName = "polyplugc"

function Write-Info($message) {
    Write-Host "[INFO] " -ForegroundColor Green -NoNewline
    Write-Host $message
}

function Write-Error-Exit($message) {
    Write-Host "[ERROR] " -ForegroundColor Red -NoNewline
    Write-Host $message
    exit 1
}

function Get-Platform {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture
    
    if ($arch -eq [System.Runtime.InteropServices.Architecture]::X64) {
        return "windows-x64"
    }
    
    Write-Error-Exit "Unsupported architecture: $arch"
}

function Get-LatestVersion {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    return $release.tag_name -replace '^v', ''
}

function Download-Binary {
    param($version, $platform)
    
    $url = "https://github.com/$Repo/releases/download/v$version/${BinaryName}-${platform}.exe"
    $tmpFile = Join-Path $env:TEMP "${BinaryName}-${platform}.exe"
    
    Write-Info "Downloading polyplugc v$version for $platform..."
    Write-Info "URL: $url"
    
    try {
        Invoke-WebRequest -Uri $url -OutFile $tmpFile -UseBasicParsing
    } catch {
        Write-Error-Exit "Failed to download binary from $url"
    }
    
    return $tmpFile
}

function Install-Binary {
    param($tmpFile)
    
    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }
    
    $destFile = Join-Path $InstallDir "${BinaryName}.exe"
    Move-Item -Path $tmpFile -Destination $destFile -Force
    
    Write-Info "Installed $BinaryName to $InstallDir"
}

function Add-ToPath {
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    
    if ($currentPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable("Path", "$currentPath;$InstallDir", "User")
        Write-Info "Added $InstallDir to PATH"
        Write-Info "Restart your terminal or run: `$env:Path += `";$InstallDir`""
    }
}

# Main
Write-Info "Installing polyplugc..."

$platform = Get-Platform
Write-Info "Detected platform: $platform"

if ([string]::IsNullOrEmpty($Version)) {
    $Version = Get-LatestVersion
}
Write-Info "Version: $Version"

$tmpFile = Download-Binary -version $Version -platform $platform
Install-Binary -tmpFile $tmpFile
Add-ToPath

Write-Host ""
Write-Info "Installation complete!"
Write-Host ""
Write-Info "Run 'polyplugc --help' to get started."
Write-Host ""