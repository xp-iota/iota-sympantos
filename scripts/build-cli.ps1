$ErrorActionPreference = "Stop"
$workspace = Split-Path -Parent $PSScriptRoot
Set-Location $workspace

& cargo build --release -p iota-cli --bin iota
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host "CLI binary: $(Join-Path $workspace 'target\release\iota.exe')"
