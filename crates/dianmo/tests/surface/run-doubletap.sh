#!/bin/bash
# 单击 / 双击打开键盘 on the Surface (doubletap.ps1, ~90 s) through the current device entry
# (`ssh surface`, TokenSSH desktop job; see ~/.agents/test-devices.md), not the old 15570 tunnel.
#   crates/dianmo/tests/surface/run-doubletap.sh <dianmo.exe> <out_dir>
# Copies the exe into C:\Users\wecode\dianmo-dbltap-test, runs the job in the desktop session,
# prints its output, fetches the screenshots, then deletes the folder on the Surface.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); repo=$(cd "$here/../../../.." && pwd)
exe=${1:?usage: run-doubletap.sh <dianmo.exe> <out_dir>}; out=${2:?usage: run-doubletap.sh <dianmo.exe> <out_dir>}
job_py=~/.agents/test-device-configs/surface/surface-job.py
win='C:\Users\wecode\dianmo-dbltap-test'
# The tunnel occasionally refuses a session ("exec request failed on channel 0"): retry.
retry() { local i; for i in 1 2 3 4; do "$@" && return 0; sleep 3; done; return 1; }
retry ssh surface "New-Item -ItemType Directory -Force '$win' | Out-Null" >/dev/null
# The script goes as a file (UTF-8 with BOM for Windows PowerShell): inline it would exceed the
# 32K command line once encoded for the job.
tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
{ printf '\xef\xbb\xbf'; cat "$repo/crates/dianmo-win/tests/surface/touch.ps1" "$here/doubletap.ps1"; } > "$tmp"
retry scp -q "$exe" "$here/doubletap-test.html" "surface:C:/Users/wecode/dianmo-dbltap-test/"
retry scp -q "$tmp" "surface:C:/Users/wecode/dianmo-dbltap-test/doubletap-run.ps1"
# Submitting registers a task: not retried.
submit=$(printf '$ErrorActionPreference = "Continue"\n[Console]::OutputEncoding = [Text.Encoding]::UTF8\n$RunDir = %s\n. (Join-Path $RunDir "doubletap-run.ps1")\n' "'$win'" | python3 "$job_py")
job=$(printf '%s' "$submit" | python3 -c 'import json,sys,re; t=sys.stdin.read(); print(json.loads(t[t.index("{"):])["JobId"])')
echo "job $job"
mkdir -p "$out"
for _ in $(seq 1 60); do
    sleep 5
    status=$(python3 "$job_py" --status "$job" 2>&1) || continue
    if printf '%s' "$status" | grep -q '"ExitCode"'; then break; fi
done
printf '%s\n' "$status" | head -12
# The whole output (--status shows only its tail).
retry scp -q "surface:C:/Users/wecode/AppData/Local/TokenSSH/Jobs/$job/stdout.log" "$out/doubletap-output.txt" && cat "$out/doubletap-output.txt"
retry scp -q 'surface:C:/Users/wecode/dianmo-dbltap-test/dbl-*.png' "$out/" || true
retry ssh surface "Remove-Item -Recurse -Force '$win' -ErrorAction SilentlyContinue" >/dev/null || true
ls "$out"
