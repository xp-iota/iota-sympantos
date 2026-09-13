param(
    [switch]$Cli,
    [string]$App,
    [switch]$All
)

$ErrorActionPreference = "Stop"
$workspace = Split-Path -Parent $PSScriptRoot
$binDir = if ($env:IOTA_INSTALL_DIR) { $env:IOTA_INSTALL_DIR } else { Join-Path $HOME ".local\bin" }
$cliSource = if ($env:IOTA_CLI_SOURCE) { $env:IOTA_CLI_SOURCE } else { Join-Path $workspace "target\release\iota.exe" }

if (-not $Cli -and -not $App -and -not $All) {
    $Cli = $true
}
if ($All) {
    $Cli = $true
}

if ($Cli) {
    if (-not (Test-Path $cliSource)) {
        throw "CLI binary not found: $cliSource. Run .\scripts\build-cli.ps1 first."
    }
    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    Copy-Item $cliSource (Join-Path $binDir "iota.exe") -Force
    Write-Host "installed CLI: $(Join-Path $binDir 'iota.exe')"
}

if ($All -and -not $App) {
    $App = Get-ChildItem (Join-Path $workspace "target\release\bundle") -Recurse -File -Include *.msi, *.exe |
        Select-Object -First 1 -ExpandProperty FullName
}

if ($App) {
    if (-not (Test-Path $App)) {
        throw "desktop installer not found: $App. Run .\scripts\build-app.ps1 first."
    }
    $extension = [IO.Path]::GetExtension($App).ToLowerInvariant()
    if ($extension -eq ".msi") {
        Start-Process msiexec.exe -ArgumentList "/i", $App -Wait
    } elseif ($extension -eq ".exe") {
        Start-Process $App -Wait
    } else {
        throw "unsupported desktop installer: $App (expected .msi or .exe)"
    }
}
