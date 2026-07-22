# Pushes today's + next N days' Outlook calendar (via local Outlook COM,
# reads the already-open Outlook app) to work-dash-server.
#
# No Azure app registration or Graph consent needed - talks to the running
# Outlook.exe on this machine through the classic COM automation interface.
#
# Usage: powershell -ExecutionPolicy Bypass -File outlook_calendar_push.ps1
#   -ServerUrl http://localhost:8280 -ApiKey devkey123 -DaysAhead 14

param(
    [string]$ServerUrl = $env:WORK_DASH_SERVER_URL,
    [string]$ApiKey = $env:WORK_DASH_API_KEY,
    [int]$DaysAhead = 14,
    [int]$PollSecs = 300
)

if (-not $ServerUrl) { throw "ServerUrl (or WORK_DASH_SERVER_URL) required" }
if (-not $ApiKey) { throw "ApiKey (or WORK_DASH_API_KEY) required" }

function Clean-Str($s) {
    if (-not $s) { return $s }
    $sb = New-Object System.Text.StringBuilder
    foreach ($ch in $s.ToCharArray()) {
        if ([char]::IsSurrogate($ch)) { continue }
        [void]$sb.Append($ch)
    }
    return $sb.ToString()
}

function Clean-Title($s) {
    $cleaned = (Clean-Str $s)
    if ([string]::IsNullOrWhiteSpace($cleaned)) { return "No title" }
    return $cleaned
}

function Push-Calendar {
    $outlook = New-Object -ComObject Outlook.Application
    $ns = $outlook.GetNamespace("MAPI")
    $calendar = $ns.GetDefaultFolder(9)  # olFolderCalendar
    $items = $calendar.Items
    $items.IncludeRecurrences = $true
    $items.Sort("[Start]")

    $rangeStart = (Get-Date).Date
    $rangeEnd = $rangeStart.AddDays($DaysAhead)
    # Same zzz-offset format as event start/end below - the server compares
    # range bounds against start_at as plain strings, so format must match.
    $rangeStartStr = $rangeStart.ToString("yyyy-MM-ddTHH:mm:sszzz")
    $rangeEndStr = $rangeEnd.ToString("yyyy-MM-ddTHH:mm:sszzz")
    $filter = "[Start] >= '$($rangeStart.ToString('g'))' AND [Start] <= '$($rangeEnd.ToString('g'))'"
    $restricted = $items.Restrict($filter)

    $seen = New-Object System.Collections.Generic.HashSet[string]
    $events = @()
    foreach ($item in $restricted) {
        $start = $item.Start.ToString("yyyy-MM-ddTHH:mm:sszzz")
        $end = $item.End.ToString("yyyy-MM-ddTHH:mm:sszzz")
        $title = Clean-Title $item.Subject

        # Restrict+IncludeRecurrences occasionally yields the same occurrence
        # twice (Outlook COM quirk) - dedupe on the visible identity instead
        # of trusting EntryID/Start uniqueness from the enumerator.
        $dedupeKey = "$title|$start|$end"
        if (-not $seen.Add($dedupeKey)) { continue }

        $events += [PSCustomObject]@{
            external_id  = "outlook-" + $item.EntryID + "-" + $item.Start.ToString("yyyyMMddTHHmm")
            title        = $title
            start        = $start
            end          = $end
            place        = Clean-Str $item.Location
            is_cancelled = [bool]$item.IsCancelled
        }
    }

    $bodyJson = @{ events = $events; range_start = $rangeStartStr; range_end = $rangeEndStr } | ConvertTo-Json -Depth 5
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($bodyJson)
    Write-Host "$(Get-Date -Format o) pushing $($events.Count) events"

    Invoke-RestMethod -Method Put -Uri "$ServerUrl/api/calendar" `
        -Headers @{ Authorization = "Bearer $ApiKey" } `
        -ContentType "application/json; charset=utf-8" `
        -Body $bodyBytes
}

while ($true) {
    try {
        Push-Calendar
    } catch {
        Write-Warning "push failed: $_"
    }
    Start-Sleep -Seconds $PollSecs
}
