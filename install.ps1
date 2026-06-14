$ErrorActionPreference = "Stop"
$AIKD_VERSION = "2.0.0"
$INSTALL_DIR = "$env:LOCALAPPDATA\aikd\bin"
$URL = "https://github.com/your-org/aikd/releases/download/v$AIKD_VERSION/aikd-x86_64-pc-windows-msvc.exe"

New-Item -ItemType Directory -Force -Path $INSTALL_DIR | Out-Null
Invoke-WebRequest -Uri $URL -OutFile "$INSTALL_DIR\aikd.exe"

$currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($currentPath -notlike "*$INSTALL_DIR*") {
    [Environment]::SetEnvironmentVariable("PATH", "$currentPath;$INSTALL_DIR", "User")
    Write-Host "Added $INSTALL_DIR to PATH (restart terminal)"
}

Write-Host "AIKD v$AIKD_VERSION installed to $INSTALL_DIR\aikd.exe"
Write-Host "Run: aikd init"
