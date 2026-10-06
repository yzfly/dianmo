#!/bin/bash
# Run engine-test.ps1 (voice_probe end to end with a test Notepad) in the user's desktop session on
# the Surface and fetch its screenshots. Build voice_probe first:
#   scripts/surface/build.sh voicebuild build --release -p dianmo --no-default-features --example voice_probe
#   scripts/voice/engine-test.sh <local-shot-dir> [engine-test.ps1 args...]
#   e.g. scripts/voice/engine-test.sh /tmp/v -Engine wetype -ImeCycles 1 -Speak '今天天气不错' -Keyword 天气
set -euo pipefail
out=${1:?usage: engine-test.sh <local-shot-dir> [args...]}; shift
here=$(cd "$(dirname "$0")" && pwd); surface="$here/../surface"
dir='C:/dev/dianmo-voicebuild/target'
"$surface/ps.sh" <<'PS' >/dev/null
New-Item -ItemType Directory -Force C:\dev\dianmo-voicebuild\target\shots | Out-Null
Remove-Item C:\dev\dianmo-voicebuild\target\shots\* -ErrorAction SilentlyContinue
PS
# PowerShell 5.1 reads BOM-less scripts as ANSI: add a UTF-8 BOM so Chinese arguments survive.
tmp=$(mktemp); trap 'rm -f "$tmp"' EXIT
printf '\xef\xbb\xbf' >"$tmp"; cat "$here/engine-test.ps1" >>"$tmp"
scp -q -P 15570 "$tmp" "wecode@127.0.0.1:$dir/engine-test.ps1"
"$surface/gui.sh" 90 <<PS
& C:\dev\dianmo-voicebuild\target\engine-test.ps1 $*
PS
mkdir -p "$out"
scp -q -P 15570 "wecode@127.0.0.1:$dir/shots/*.png" "$out/" 2>/dev/null || true
ls "$out"
