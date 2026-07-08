# Launches an exe from the registered WorkDash.WindowsClient package WITH
# package identity attached. Directly running target\debug\*.exe does NOT
# get identity even after Add-AppxPackage -Register — loose registration
# only makes Windows aware of the package; identity is attached at
# activation time, and normal double-click/shell exec of the raw exe is not
# a package activation. Invoke-CommandInDesktopPackage is the documented
# bridge for classic (non-UWP) exes that need it (e.g. for
# UserNotificationListener).
#
# Usage: powershell -ExecutionPolicy Bypass -File run_with_identity.ps1 <AppId> [args...]
#   AppId is "ToastLogger" or "WorkDashClient" (see AppxManifest.xml).

param(
    [Parameter(Mandatory=$true)][string]$AppId,
    [Parameter(ValueFromRemainingArguments=$true)][string[]]$Rest
)

$ErrorActionPreference = "Stop"

$pkg = Get-AppxPackage -Name "WorkDash.WindowsClient" -ErrorAction SilentlyContinue
if (-not $pkg) {
    Write-Error "package not registered — run register.ps1 first."
}

$exeName = if ($AppId -eq "ToastLogger") { "toast_logger.exe" } else { "work-dash-windows-client.exe" }
$targetDir = Join-Path (Split-Path $PSScriptRoot -Parent) "target\debug"
$exePath = Join-Path $targetDir $exeName

if (-not (Test-Path $exePath)) {
    Write-Error "$exePath not found — run 'cargo build' first."
}

Write-Host "package family name: $($pkg.PackageFamilyName)"
Write-Host "launching $exePath with package identity (AppId=$AppId)..."

Invoke-CommandInDesktopPackage `
    -PackageFamilyName $pkg.PackageFamilyName `
    -AppId $AppId `
    -PreventBreakaway `
    -Command $exePath `
    -Args ($Rest -join " ")
