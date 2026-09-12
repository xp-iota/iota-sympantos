param(
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$workspace = Split-Path -Parent $PSScriptRoot
Set-Location $workspace

$packages = @("iota-sympantos-kanban", "iota-sympantos-core")
foreach ($package in $packages) {
    if ($DryRun) {
        $arguments = @("package", "-p", $package, "--allow-dirty")
        Write-Host "Packaging $package"
    }
    else {
        $arguments = @("publish", "-p", $package)
        Write-Host "Publishing $package"
    }
    & cargo @arguments
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}
