# Removes the dev-mode package identity registration.
$existing = Get-AppxPackage -Name "WorkDash.WindowsClient" -ErrorAction SilentlyContinue
if ($existing) {
    Remove-AppxPackage $existing
    Write-Host "unregistered."
} else {
    Write-Host "nothing registered."
}
