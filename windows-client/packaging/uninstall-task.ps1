# Removes the WorkDashCalendarPush scheduled task installed by install-task.ps1.
$taskName = "WorkDashCalendarPush"
$existing = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
if ($existing) {
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
    Write-Host "unregistered '$taskName'."
} else {
    Write-Host "nothing registered."
}
