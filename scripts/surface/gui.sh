#!/bin/bash
# Run a PowerShell script (stdin) inside the user's desktop session on the Surface, so it can
# start GUI apps, inject input and see the real screen. Prints the script's output.
# Queued by a lock: only one GUI job at a time. The user is using this machine: keep jobs short
# and close whatever you open.
#   scripts/surface/gui.sh [timeout_seconds] <<'PS' ... PS
set -euo pipefail
timeout=${1:-60}
here=$(cd "$(dirname "$0")" && pwd)
job=$(printf '%s\n' '$ProgressPreference="SilentlyContinue"' '& {' "$(cat)" '} *>&1 | ForEach-Object { "$_" } | Out-File C:\Users\wecode\claude\gui-job.out -Encoding utf8' | base64 -w0)
exec 9>/tmp/dianmo-surface-gui.lock; flock 9
"$here/ps.sh" <<PS
\$dir = 'C:\Users\wecode\claude'; New-Item -ItemType Directory -Force \$dir | Out-Null
[IO.File]::WriteAllText("\$dir\gui-job.ps1", [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('$job')), (New-Object Text.UTF8Encoding \$true))
Remove-Item "\$dir\gui-job.out" -ErrorAction SilentlyContinue
\$a = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File \$dir\gui-job.ps1"
\$p = New-ScheduledTaskPrincipal -UserId 'wecode' -LogonType Interactive -RunLevel Highest
\$s = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds $timeout) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName 'DianmoGui' -Action \$a -Principal \$p -Settings \$s -Force | Out-Null
Start-ScheduledTask DianmoGui
Start-Sleep -Milliseconds 300
\$t0 = Get-Date
while ((Get-ScheduledTask DianmoGui).State -eq 'Running' -and ((Get-Date) - \$t0).TotalSeconds -lt $timeout + 5) { Start-Sleep -Milliseconds 200 }
if (Test-Path "\$dir\gui-job.out") { Get-Content "\$dir\gui-job.out" -Encoding UTF8 } else { "(no output; task result \$((Get-ScheduledTaskInfo DianmoGui).LastTaskResult))" }
PS
