#!/bin/bash
# App-window test on the Surface: window_demo opened from its keyboard strip by injected touch;
# drag/fling, tap, mouse hover and wheel, keys, dark title bar, memory across open/close cycles.
# Screenshots land in /tmp/appwin-{light,dark}.png.
# Build first:  scripts/surface/build.sh appwin build --release -p dianmo-win --example window_demo
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); repo=$(cd "$here/../../../.." && pwd)
tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
{ printf '\xef\xbb\xbf'; cat "$here/touch.ps1" "$here/window.ps1"; } > "$tmp"
scp -q -P 15570 "$tmp" wecode@127.0.0.1:C:/Users/wecode/claude/win-window.ps1
"$repo/scripts/surface/gui.sh" 90 <<'PS'
. C:\Users\wecode\claude\win-window.ps1
PS
for n in light dark; do scp -q -P 15570 "wecode@127.0.0.1:C:/Users/wecode/claude/appwin-$n.png" "${OUT:-/tmp}/appwin-$n.png" || true; done
