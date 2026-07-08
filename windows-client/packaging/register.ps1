# Registers dev-mode package identity for the cargo-built exes so
# UserNotificationListener's NotificationChanged event works (it fails with
# 0x80070490 for unpackaged processes).
#
# Loose registration makes the folder containing AppxManifest.xml the
# package root, and the exes listed in the manifest must sit next to it —
# so this script copies the manifest + assets into target\debug and
# registers there. Requires Windows Developer Mode
# (Settings > System > For developers > Developer Mode).
#
# Run:  powershell -ExecutionPolicy Bypass -File register.ps1
# Then launch the exe directly, e.g.:  ..\target\debug\toast_logger.exe

$ErrorActionPreference = "Stop"

$packagingDir = $PSScriptRoot
$targetDir = Join-Path (Split-Path $packagingDir -Parent) "target\debug"

if (-not (Test-Path (Join-Path $targetDir "toast_logger.exe"))) {
    Write-Error "toast_logger.exe not found in $targetDir — run 'cargo build' first."
}

# Manifest validation requires every listed Executable to exist. The main
# client exe may not be built yet in a toast_logger-only workflow.
if (-not (Test-Path (Join-Path $targetDir "work-dash-windows-client.exe"))) {
    Write-Warning "work-dash-windows-client.exe not built yet — registration will fail unless it exists. Running 'cargo build' builds both."
}

Copy-Item (Join-Path $packagingDir "AppxManifest.xml") $targetDir -Force
New-Item -ItemType Directory -Force (Join-Path $targetDir "Assets") | Out-Null
Copy-Item (Join-Path $packagingDir "Assets\logo.png") (Join-Path $targetDir "Assets") -Force

# Re-registering the same version over an existing install errors; drop any
# previous registration first.
$existing = Get-AppxPackage -Name "WorkDash.WindowsClient" -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "removing previous registration..."
    Remove-AppxPackage $existing
}

Add-AppxPackage -Register (Join-Path $targetDir "AppxManifest.xml")

Write-Host ""
Write-Host "registered. Launch the exe directly from the package root:"
Write-Host "  $targetDir\toast_logger.exe"
Write-Host ""
Write-Host "note: after 'cargo build' rewrites the exes, identity keeps working"
Write-Host "(loose registration doesn't hash files), but if Windows misbehaves"
Write-Host "re-run this script."
