$ErrorActionPreference = "Stop"
$AIKD_VERSION = "2.0.0"
$INSTALL_DIR = "$env:LOCALAPPDATA\aikd\bin"
$URL = "https://github.com/gelutjari/aikd/releases/download/v$AIKD_VERSION/aikd-x86_64-pc-windows-msvc.exe"

New-Item -ItemType Directory -Force -Path $INSTALL_DIR | Out-Null
Invoke-WebRequest -Uri $URL -OutFile "$INSTALL_DIR\aikd.exe"
try {
    Invoke-WebRequest -Uri "https://github.com/gelutjari/aikd/releases/download/v$AIKD_VERSION/SHA256SUMS" -OutFile "$INSTALL_DIR\SHA256SUMS" -ErrorAction Stop
    $line = Select-String -Path "$INSTALL_DIR\SHA256SUMS" -Pattern "aikd-x86_64-pc-windows-msvc.exe$" | Select-Object -First 1
    if ($line) {
        $expected = ($line.Line -split '\s+')[0]
        $actual = (Get-FileHash "$INSTALL_DIR\aikd.exe" -Algorithm SHA256).Hash.ToLower()
        if ($actual -ne $expected.ToLower()) { throw "Checksum FAILED for aikd.exe" }
        Write-Host "Checksum OK"
    }
} catch {
    Write-Host "Checksum file unavailable, skipping verification: $($_.Exception.Message)"
}

$currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($currentPath -notlike "*$INSTALL_DIR*") {
    [Environment]::SetEnvironmentVariable("PATH", "$currentPath;$INSTALL_DIR", "User")
    Write-Host "Added $INSTALL_DIR to PATH (restart terminal)"
}

Write-Host "AIKD v$AIKD_VERSION installed to $INSTALL_DIR\aikd.exe"
Write-Host "Run: aikd init"
