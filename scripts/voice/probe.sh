#!/bin/bash
# Run hotkey-probe.ps1 in the user's desktop session on the Surface and fetch its screenshots.
#   scripts/voice/probe.sh <local-shot-dir> [hotkey-probe.ps1 args...]
#   e.g. scripts/voice/probe.sh /tmp/shots -Keys LCtrl+LWin -HoldMs 3000 -Tag wetype
set -euo pipefail
out=${1:?usage: probe.sh <local-shot-dir> [args...]}; shift
here=$(cd "$(dirname "$0")" && pwd); surface="$here/../surface"
"$surface/ps.sh" <<'PS' >/dev/null
New-Item -ItemType Directory -Force C:\dev\dianmo-voice\shots | Out-Null
Remove-Item C:\dev\dianmo-voice\shots\* -ErrorAction SilentlyContinue
PS
# PowerShell 5.1 reads BOM-less scripts as ANSI: add a UTF-8 BOM so the Chinese test text survives.
tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
printf '\xef\xbb\xbf' >"$tmp"; cat "$here/hotkey-probe.ps1" >>"$tmp"
scp -q -P 15570 "$tmp" wecode@127.0.0.1:C:/dev/dianmo-voice/hotkey-probe.ps1
"$surface/gui.sh" 60 <<PS
& C:\dev\dianmo-voice\hotkey-probe.ps1 $*
PS
mkdir -p "$out"
scp -q -P 15570 'wecode@127.0.0.1:C:/dev/dianmo-voice/shots/*.png' "$out/" 2>/dev/null || true
for f in "$out"/*.png; do
  case "$f" in *-s.png) continue;; esac
  uv run -q --with pillow python3 -c "import sys; from PIL import Image; im=Image.open(sys.argv[1]); im.thumbnail((1440,1440)); im.save(sys.argv[2])" "$f" "${f%.png}-s.png"
done
ls "$out"
