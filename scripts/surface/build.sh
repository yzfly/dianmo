#!/bin/bash
# Sync, then run cargo on the Surface at below-normal priority with 2 jobs (queued by a lock).
#   scripts/surface/build.sh <name> [cargo args...]      default: build --release
set -euo pipefail
name=${1:?usage: build.sh <name> [cargo args...]}; shift
args=${*:-build --release}
here=$(cd "$(dirname "$0")" && pwd)
exec 9>/tmp/dianmo-surface-build.lock; flock 9
"$here/sync.sh" "$name"
"$here/ps.sh" <<PS
\$env:Path = "\$HOME\.cargo\bin;C:\dev\tools\llvm-mingw\bin;\$env:Path"
\$env:CARGO_TERM_COLOR = 'never'
Set-Location 'C:\dev\dianmo-$name'
cmd /c start '""' /belownormal /b /wait cargo $args -j 2 2>&1
"cargo exit: \$LASTEXITCODE"
PS
