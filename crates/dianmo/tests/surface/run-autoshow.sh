#!/bin/bash
# Auto show / hide of a test copy of 点墨 in real apps on the Surface (autoshow.ps1, ~90 s).
# Opens Notepad, an Explorer window, a private Edge, a private VS Code and an administrator
# PowerShell, injects real touch, closes everything again. Screenshots: C:\Users\wecode\claude\auto-*.png
#   crates/dianmo/tests/surface/run-autoshow.sh <run_dir> [sections]
# sections: all (default) or a comma list of notepad,explorer,edge,vscode,admin
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); repo=$(cd "$here/../../../.." && pwd)
run_dir=${1:?usage: run-autoshow.sh <run_dir> [sections]}; sections=${2:-all}
tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
{ printf '\xef\xbb\xbf'; cat "$repo/crates/dianmo-win/tests/surface/touch.ps1" "$here/autoshow.ps1"; } > "$tmp"
scp -q -P 15570 "$tmp" wecode@127.0.0.1:C:/Users/wecode/claude/dianmo-autoshow.ps1
scp -q -P 15570 "$repo/crates/dianmo-win/tests/surface/focus-test.html" wecode@127.0.0.1:C:/Users/wecode/claude/focus-test.html
"$repo/scripts/surface/gui.sh" 240 <<PS
Remove-Item C:\Users\wecode\claude\auto-*.png -ErrorAction SilentlyContinue
\$RunDir = '$run_dir'; \$Sections = '$sections'
. C:\Users\wecode\claude\dianmo-autoshow.ps1
PS
