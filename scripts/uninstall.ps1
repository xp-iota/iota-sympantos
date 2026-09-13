param(
    [string]$AppPath
)

$ErrorActionPreference = "Stop"
$binDir = if ($env:IOTA_INSTALL_DIR) { $env:IOTA_INSTALL_DIR } else { Join-Path $HOME ".local\bin" }

Remove-Item (Join-Path $binDir "iota.exe") -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $binDir "iota-desktop.exe") -Force -ErrorAction SilentlyContinue

if ($AppPath) {
    if (Test-Path $AppPath) {
        $extension = [IO.Path]::GetExtension($AppPath).ToLowerInvariant()
        if ($extension -eq ".msi") {
            Start-Process msiexec.exe -ArgumentList "/x", $AppPath -Wait
        } else {
            Write-Host "remove the installed desktop app through Windows Settings: $AppPath"
        }
    }
}

Write-Host "removed user-local iota CLI artifacts"
Write-Host "configuration and data under $HOME\.i6 were kept"
