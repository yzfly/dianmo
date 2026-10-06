#!/bin/bash
# Short GUI pass over the settings window and onboarding (settings-gui.ps1, ~40 s) on the Surface,
# then fetches the window screenshots into <out_dir>.
#   crates/dianmo/tests/surface/run-settings.sh <run_dir> <out_dir>
# run_dir: Windows folder with dianmo.exe (+ rime.dll, data\rime), e.g. C:\dev\dianmo-dist\<name>\Dianmo
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); repo=$(cd "$here/../../../.." && pwd)
run_dir=${1:?usage: run-settings.sh <run_dir> <out_dir>}; out=${2:?usage: run-settings.sh <run_dir> <out_dir>}
tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
{ printf '\xef\xbb\xbf'; cat "$repo/crates/dianmo-win/tests/surface/touch.ps1" "$here/settings-gui.ps1"; } > "$tmp"
scp -q -P 15570 "$tmp" wecode@127.0.0.1:C:/Users/wecode/claude/dianmo-settings-gui.ps1
"$repo/scripts/surface/gui.sh" 90 <<PS
Remove-Item C:\Users\wecode\claude\set-*.png -ErrorAction SilentlyContinue
\$RunDir = '$run_dir'
. C:\Users\wecode\claude\dianmo-settings-gui.ps1
PS
mkdir -p "$out"
scp -q -P 15570 'wecode@127.0.0.1:C:/Users/wecode/claude/set-*.png' "$out/" || true
ls "$out"
