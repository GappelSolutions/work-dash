# Privacy-safe all-day Teams event capture.
#
# Tails the active main Teams log (MSTeams_*.log, handling rotation) and
# appends ONE line per whitelisted event to the capture file. It NEVER writes
# message text, caller names, chat content, or any free text from the log —
# only a fixed event tag and, at most, a numeric count. This is deliberate:
# the goal is to learn which log signals reliably mark an incoming call vs a
# chat message, without recording anything sensitive.
#
# Output line format:  <ISO-local-timestamp> | <TAG> [| n=<number>]
# Tags (all name-free):
#   CALL_HFP_REPORT   HfpVoipCallCoordinatorImpl: reportIncomingCall (headset only)
#   CALL_STATE        CallingCrossCloudModule cloud state changed
#   RINGING           any line mentioning ringing/ringtone
#   NOTIF_PANEL       NotificationPanelManager showed a popup (call OR chat toast)
#   TOAST_SHOWN       ToastsModule displayed a toast
#   UNREAD            unread notification count changed (n = new count)
#   MISSED_CHANNEL    channel notification action
#
# Usage:  powershell -ExecutionPolicy Bypass -File teams_day_capture.ps1
# Stop:   Ctrl+C (or Stop-Process on its PID)

$logDir = Join-Path $env:LOCALAPPDATA 'Packages\MSTeams_8wekyb3d8bbwe\LocalCache\Microsoft\MSTeams\Logs'
$outFile = Join-Path $PSScriptRoot 'teams-events.txt'

function Get-ActiveMainLog {
    Get-ChildItem $logDir -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like 'MSTeams_*.log' } |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
}

# Whitelist: pattern -> tag. Only these ever produce output. Anything not
# matched is ignored, so no arbitrary log content can leak.
$rules = @(
    @{ pat = 'HfpVoipCallCoordinatorImpl: reportIncomingCall for callId'; tag = 'CALL_HFP_REPORT' },
    @{ pat = 'CallingCrossCloudModule.*CloudStateChanged';                tag = 'CALL_STATE' },
    @{ pat = '(?i)ringtone|isringing|startringing';                       tag = 'RINGING' },
    @{ pat = 'NotificationPanelManager: Layout';                          tag = 'NOTIF_PANEL' },
    @{ pat = 'ToastsModule: .*(Showing|Display|Present)';                 tag = 'TOAST_SHOWN' },
    @{ pat = 'MissedChannelNotifications';                                tag = 'MISSED_CHANNEL' }
)

$header = "=== capture start $(Get-Date -Format o) ==="
Add-Content -Path $outFile -Value $header
Write-Host $header
Write-Host "watching $logDir"
Write-Host "writing name-free event tags to $outFile"

$currentPath = $null
$offset = 0
$lastUnread = $null

while ($true) {
    $active = Get-ActiveMainLog
    if ($active) {
        if ($currentPath -ne $active.FullName) {
            if ($null -eq $currentPath) {
                # First attach: skip prior-session history.
                $offset = $active.Length
            } else {
                # Rotation: new log starts empty, read from the beginning.
                $offset = 0
            }
            $currentPath = $active.FullName
            Add-Content -Path $outFile -Value "$(Get-Date -Format o) | LOG_SWITCH"
        }

        try {
            $fs = [System.IO.File]::Open($currentPath, 'Open', 'Read', 'ReadWrite')
            if ($offset -le $fs.Length) {
                $fs.Seek($offset, 'Begin') | Out-Null
                $sr = New-Object System.IO.StreamReader($fs)
                $chunk = $sr.ReadToEnd()
                $offset = $fs.Position
                $sr.Close()
                $fs.Close()

                foreach ($line in ($chunk -split "`n")) {
                    if ($line.Length -eq 0) { continue }
                    $ts = Get-Date -Format o

                    # UNREAD: record only the numeric count, and only on change.
                    if ($line -match 'unread notification count: (\d+)') {
                        $n = [int]$matches[1]
                        if ($n -ne $lastUnread) {
                            $lastUnread = $n
                            Add-Content -Path $outFile -Value "$ts | UNREAD | n=$n"
                        }
                        continue
                    }

                    foreach ($rule in $rules) {
                        if ($line -match $rule.pat) {
                            Add-Content -Path $outFile -Value "$ts | $($rule.tag)"
                            break
                        }
                    }
                }
            } else {
                # File shrank (unexpected) — reset.
                $offset = 0
                $fs.Close()
            }
        } catch {
            # Log momentarily locked; retry next tick.
        }
    }
    Start-Sleep -Seconds 2
}
