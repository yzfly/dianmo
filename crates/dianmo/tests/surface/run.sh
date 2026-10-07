#!/bin/bash
# End-to-end test of dianmo.exe on the Surface: real touch injection into classic Notepad.
#   crates/dianmo/tests/surface/run.sh [run_dir] [keys]
# run_dir: Windows folder with dianmo.exe (+ rime.dll, data\rime), default C:\dev\dianmo-app\target\run
# keys:    space-separated key names to tap after the keyboard shows (default: n i h a o space);
#          also SHOT, WAIT<ms>, @x,y (screen px) - see e2e.ps1
# GUI_TIMEOUT=<s> (env) for long runs (default 150).
# Screenshots are left in C:\Users\wecode\claude\dm-*.png (fetch with scp).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); repo=$(cd "$here/../../../.." && pwd)
run_dir=${1:-'C:\dev\dianmo-app\target\run'}
keys=${2:-'n i h a o space'}
tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
{ printf '\xef\xbb\xbf'; cat "$repo/crates/dianmo-win/tests/surface/touch.ps1" "$here/e2e.ps1"; } > "$tmp"
scp -q -P 15570 "$tmp" wecode@127.0.0.1:C:/Users/wecode/claude/dianmo-e2e.ps1
"$repo/scripts/surface/gui.sh" "${GUI_TIMEOUT:-150}" <<PS
\$RunDir = '$run_dir'; \$Keys = '$keys' -split ' '
. C:\Users\wecode\claude\dianmo-e2e.ps1
PS
