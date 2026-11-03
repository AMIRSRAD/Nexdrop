# Idle cost of a running Nexdrop (the app and its WebView2 processes): CPU time over a
# window and memory, with the window shown and hidden in the tray.
#   powershell -ExecutionPolicy Bypass -File scripts/measure_idle.ps1 [-Seconds 30]
param([int]$Seconds = 30)

function Tree($rootId) {
    $all = Get-CimInstance Win32_Process
    $ids = @($rootId)
    do {
        $new = $all | Where-Object { $ids -contains $_.ParentProcessId -and -not ($ids -contains $_.ProcessId) } | ForEach-Object { $_.ProcessId }
        $ids += $new
    } while ($new)
    $ids
}

$main = Get-Process nexdrop -ErrorAction Stop | Select-Object -First 1
$ids = Tree $main.Id
$before = @{}
Get-Process -Id $ids | ForEach-Object { $before[$_.Id] = $_.TotalProcessorTime.TotalMilliseconds }
Start-Sleep $Seconds
$rows = Get-Process -Id $ids -ErrorAction SilentlyContinue | ForEach-Object {
    [pscustomobject]@{ Name = $_.ProcessName; CpuMs = [math]::Round($_.TotalProcessorTime.TotalMilliseconds - $before[$_.Id]); MemMB = [math]::Round($_.WorkingSet64 / 1MB) }
}
$app = ($rows | Where-Object Name -eq 'nexdrop')
$cpu = ($rows | Measure-Object CpuMs -Sum).Sum
$mem = ($rows | Measure-Object MemMB -Sum).Sum
"backend cpu: $($app.CpuMs) ms / ${Seconds}s   total cpu: $cpu ms / ${Seconds}s   memory: $mem MB ($($rows.Count) processes)"
if ($env:IDLE_DETAIL) { $rows | Sort-Object CpuMs -Descending | Format-Table -AutoSize | Out-String }
