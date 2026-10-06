#!/bin/bash
# Focus watcher / auto show-hide / tray menu / full-screen test of the demo on the Surface.
# Build first:  scripts/surface/build.sh win build --release -p dianmo-win --example demo
# Opens Notepad, an Explorer window, a private Edge instance (own profile) and the taskbar search,
# injects real touch, and closes everything again. Screenshots: C:\Users\wecode\claude\focus-*.png
#   run-focus.sh [sections]   sections: all (default) or a comma list of
#                             notepad,explorer,edge,vscode,search,fullscreen,tray
set -euo pipefail
sections=${1:-all}
here=$(cd "$(dirname "$0")" && pwd); repo=$(cd "$here/../../../.." && pwd)
tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
{ printf '\xef\xbb\xbf'; echo "\$sections = '$sections'"; cat "$here/touch.ps1" "$here/focus.ps1"; } > "$tmp"
scp -q -P 15570 "$tmp" wecode@127.0.0.1:C:/Users/wecode/claude/win-focus.ps1
scp -q -P 15570 "$here/focus-test.html" wecode@127.0.0.1:C:/Users/wecode/claude/focus-test.html
"$repo/scripts/surface/gui.sh" 180 <<'PS'
. C:\Users\wecode\claude\win-focus.ps1
PS
