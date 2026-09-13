param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$TauriArgs
)

$ErrorActionPreference = "Stop"
$workspace = Split-Path -Parent $PSScriptRoot
$desktop = Join-Path $workspace "crates\iota-desktop"

if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    throw "npm is required to build the desktop app"
}

if (-not (Test-Path (Join-Path $desktop "node_modules"))) {
    Push-Location $desktop
    npm ci
    Pop-Location
}

Push-Location $desktop
& npm run tauri -- build @TauriArgs
$exitCode = $LASTEXITCODE
Pop-Location
if ($exitCode -ne 0) { exit $exitCode }
Write-Host "desktop bundles: $(Join-Path $workspace 'target\release\bundle')"
