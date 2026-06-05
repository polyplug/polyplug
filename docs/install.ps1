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

    $asset = "${BinaryName}-${platform}.exe"
    $baseUrl = "https://github.com/$Repo/releases/download/v$version"
    $url = "$baseUrl/$asset"
    $tmpFile = Join-Path $env:TEMP $asset

    Write-Info "Downloading polyplugc v$version for $platform..."
    Write-Info "URL: $url"

    try {
        Invoke-WebRequest -Uri $url -OutFile $tmpFile -UseBasicParsing
    } catch {
        Write-Error-Exit "Failed to download binary from $url"
    }

    Verify-Checksum -file $tmpFile -asset $asset -baseUrl $baseUrl

    return $tmpFile
}

# Verify the downloaded binary against the release SHA256SUMS manifest.
# Aborts the install on mismatch or missing entry so a tampered or truncated
# download is never executed.
function Verify-Checksum {
    param($file, $asset, $baseUrl)

    $sumsFile = Join-Path $env:TEMP "${BinaryName}-SHA256SUMS"
    try {
        Invoke-WebRequest -Uri "$baseUrl/SHA256SUMS" -OutFile $sumsFile -UseBasicParsing
    } catch {
        Write-Error-Exit "Failed to download SHA256SUMS from $baseUrl/SHA256SUMS - cannot verify integrity"
    }

    $expected = $null
    foreach ($line in Get-Content $sumsFile) {
        # SHA256SUMS lines are "<hash>  <filename>".
        $parts = $line -split '\s+', 2
        if ($parts.Count -eq 2 -and $parts[1].Trim() -eq $asset) {
            $expected = $parts[0].Trim().ToLower()
            break
        }
    }
    if ([string]::IsNullOrEmpty($expected)) {
        Write-Error-Exit "No checksum entry for '$asset' in SHA256SUMS - refusing to install"
    }

    $actual = (Get-FileHash -Path $file -Algorithm SHA256).Hash.ToLower()
    if ($expected -ne $actual) {
        Remove-Item -Path $file -Force -ErrorAction SilentlyContinue
        Write-Error-Exit "Checksum mismatch for $asset (expected $expected, got $actual) - download may be corrupted or tampered"
    }

    Write-Info "Checksum verified for $asset"
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