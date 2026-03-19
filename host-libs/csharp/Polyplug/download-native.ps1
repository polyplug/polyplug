# Download native libraries from GitHub Releases
# Usage: pwsh -File download-native.ps1 -Version 0.1.0

param(
    [Parameter(Mandatory=$true)]
    [string]$Version
)

$ErrorActionPreference = "Stop"

# GitHub Releases base URL
$BaseUrl = "https://github.com/polyplug/polyplug/releases/download/v$Version"

# Script directory (where this .ps1 file lives)
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

# Define native library mappings
$Libraries = @(
    @{
        Url = "$BaseUrl/libpolyplug-linux-x64.so"
        Path = "$ScriptDir\runtimes\linux-x64\native\libpolyplug.so"
        Name = "Linux x64"
    },
    @{
        Url = "$BaseUrl/libpolyplug-macos-x64.dylib"
        Path = "$ScriptDir\runtimes\osx-x64\native\libpolyplug.dylib"
        Name = "macOS x64"
    },
    @{
        Url = "$BaseUrl/libpolyplug-macos-arm64.dylib"
        Path = "$ScriptDir\runtimes\osx-arm64\native\libpolyplug.dylib"
        Name = "macOS ARM64"
    },
    @{
        Url = "$BaseUrl/polyplug-windows-x64.dll"
        Path = "$ScriptDir\runtimes\win-x64\native\polyplug.dll"
        Name = "Windows x64"
    }
)

Write-Host "Downloading native libraries for polyplug v$Version..."
Write-Host ""

foreach ($Lib in $Libraries) {
    $LibName = $Lib.Name
    $Url = $Lib.Url
    $Path = $Lib.Path
    
    Write-Host "Downloading $LibName..."
    Write-Host "  URL: $Url"
    Write-Host "  Path: $Path"
    
    # Ensure directory exists
    $Dir = Split-Path -Parent $Path
    if (!(Test-Path $Dir)) {
        New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    }
    
    # Remove placeholder README if it exists
    $PlaceholderPath = Join-Path $Dir "README.txt"
    if (Test-Path $PlaceholderPath) {
        Remove-Item -Path $PlaceholderPath -Force
    }
    
    # Download the file
    try {
        Invoke-WebRequest -Uri $Url -OutFile $Path -UseBasicParsing
        Write-Host "  ✓ Success" -ForegroundColor Green
    } catch {
        Write-Host "  ✗ Failed: $_" -ForegroundColor Red
        Write-Host "  Continuing with other platforms..."
    }
    
    Write-Host ""
}

Write-Host "Native library download complete."
Write-Host ""
Write-Host "Note: Some downloads may have failed if the release doesn't include all platforms."
Write-Host "This is expected for development builds. CI should ensure all platforms are available."
