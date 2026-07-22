# Registers a Scheduled Task that runs outlook_calendar_push.ps1 at logon and
# restarts it on crash. This is the real calendar sync path — Outlook COM
# automation, no Graph/Azure consent needed (the Graph-based
# work-dash-windows-client.exe calendar thread is dead: this org's tenant
# blocks admin consent for the Graph CLI public client).
#
# Outlook COM automation requires an interactive desktop session, so this
# runs as LogonType Interactive tied to your actual logon (not "run whether
# logged on or not" / session 0, which the old exe task used and which
# can't drive Outlook.exe). No stored password needed for this logon type.
#
# WORK_DASH_SERVER_URL / WORK_DASH_API_KEY must already be set as
# persistent env vars (setx), since the script defaults to reading them
# from the environment.
#
# Run:  powershell -ExecutionPolicy Bypass -File install-task.ps1

$ErrorActionPreference = "Stop"

$taskName = "WorkDashCalendarPush"
$scriptPath = "C:\dev\windows-client\outlook_calendar_push.ps1"

if (-not (Test-Path $scriptPath)) {
    Write-Error "$scriptPath not found."
}

$existing = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "removing previous task registration..."
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
}

$action = New-ScheduledTaskAction `
    -Execute "powershell.exe" `
    -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$scriptPath`"" `
    -WorkingDirectory (Split-Path $scriptPath -Parent)
$trigger = New-ScheduledTaskTrigger -AtLogOn
$principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet `
    -RestartCount 999 `
    -RestartInterval (New-TimeSpan -Minutes 1) `
    -ExecutionTimeLimit (New-TimeSpan -Hours 0) `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries

Register-ScheduledTask `
    -TaskName $taskName `
    -Action $action `
    -Trigger $trigger `
    -Principal $principal `
    -Settings $settings | Out-Null

Write-Host ""
Write-Host "registered '$taskName', starts at next logon, restarts on crash (up to 999x, 1 min apart)."
Write-Host "start it now without logging out:  Start-ScheduledTask -TaskName $taskName"
Write-Host "check status:                      Get-ScheduledTask -TaskName $taskName | Get-ScheduledTaskInfo"
