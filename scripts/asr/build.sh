#!/bin/bash
# Build the dianmo-asr example on the Surface (C:\dev\dianmo-asr-build, NOT C:\dev\dianmo-asr: sync.sh
# wipes its target dir) and install it + the scripts into C:\dev\dianmo-asr\{bin,scripts}.
#   scripts/asr/build.sh
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd); repo=$(cd "$here/../.." && pwd)
"$repo/scripts/surface/build.sh" asr-build build --release -p dianmo-asr --example asr_wav
"$repo/scripts/surface/ps.sh" <<'PS'
New-Item -ItemType Directory -Force C:\dev\dianmo-asr\bin, C:\dev\dianmo-asr\scripts | Out-Null
Copy-Item C:\dev\dianmo-asr-build\target\release\examples\asr_wav.exe C:\dev\dianmo-asr\bin\ -Force
Copy-Item C:\dev\dianmo-asr-build\scripts\asr\* C:\dev\dianmo-asr\scripts\ -Force
"installed: $((Get-Item C:\dev\dianmo-asr\bin\asr_wav.exe).Length / 1KB) KB"
PS
