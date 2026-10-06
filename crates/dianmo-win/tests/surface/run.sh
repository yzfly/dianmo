#!/bin/bash
# End-to-end test of the demo on the Surface (real touch injection into classic Notepad).
# Build first:  scripts/surface/build.sh win build --release -p dianmo-win --example demo
# The script is too long for gui.sh's command line, so it is uploaded and dot-sourced.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); repo=$(cd "$here/../../../.." && pwd)
tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
{ printf '\xef\xbb\xbf'; cat "$here/touch.ps1" "$here/e2e.ps1"; } > "$tmp"
scp -q -P 15570 "$tmp" wecode@127.0.0.1:C:/Users/wecode/claude/win-e2e.ps1
"$repo/scripts/surface/gui.sh" 120 <<'PS'
. C:\Users\wecode\claude\win-e2e.ps1
PS
