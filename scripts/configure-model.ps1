param(
    [switch]$Init,
    [switch]$Open
)

$ErrorActionPreference = "Stop"
$workspace = Split-Path -Parent $PSScriptRoot
$configDir = if ($env:IOTA_CONFIG_DIR) { $env:IOTA_CONFIG_DIR } else { Join-Path $HOME ".i6" }
$configPath = Join-Path $configDir "nimia.yaml"

if (-not (Test-Path $configPath)) {
    $Init = $true
}

if ($Init) {
    New-Item -ItemType Directory -Force -Path $configDir | Out-Null
    if (-not (Test-Path $configPath)) {
        Copy-Item (Join-Path $workspace "nimia.yaml.template") $configPath
        Write-Host "created $configPath"
    } else {
        Write-Host "kept existing $configPath"
    }
}

if ($Open) {
    if ($env:VISUAL) {
        & $env:VISUAL $configPath
    } elseif ($env:EDITOR) {
        & $env:EDITOR $configPath
    } else {
        Start-Process notepad.exe -ArgumentList $configPath -Wait
    }
} else {
    Write-Host "edit $configPath and replace placeholder API keys before running iota"
}
