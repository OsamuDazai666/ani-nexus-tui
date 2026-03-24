$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$repo = "OsamuDazai666/nexus-tui"
$installerUrl = "https://github.com/$repo/releases/latest/download/ani-nexus-tui-installer.ps1"
$tempFile = Join-Path $env:TEMP "ani-nexus-tui-installer.ps1"

Write-Host "Fetching latest ani-nexus installer from GitHub Releases..."
Invoke-WebRequest -Uri $installerUrl -OutFile $tempFile

try {
    & $tempFile @args
} finally {
    Remove-Item $tempFile -Force -ErrorAction SilentlyContinue
}
