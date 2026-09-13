param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CliArgs
)

$ErrorActionPreference = "Stop"
$workspace = Split-Path -Parent $PSScriptRoot
$configPath = Join-Path $HOME ".i6\nimia.yaml"

if (-not (Test-Path $configPath)) {
    Write-Error "model config is missing: $configPath. Run .\scripts\configure-model.ps1 -Init first."
}

Set-Location $workspace
& cargo run -p iota-cli --quiet -- @CliArgs
exit $LASTEXITCODE
