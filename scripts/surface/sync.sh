#!/bin/bash
# Mirror the repo (tracked + untracked-not-ignored files) to C:\dev\dianmo-<name> on the Surface.
# Keeps the remote target\ dir so builds stay incremental.
#   scripts/surface/sync.sh <name>
set -euo pipefail
name=${1:?usage: sync.sh <name>}
here=$(cd "$(dirname "$0")" && pwd); repo=$(cd "$here/../.." && pwd)
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
(cd "$repo" && git ls-files -co --exclude-standard -z | xargs -0 tar -czf "$tmp/src.tgz" --)
scp -q -P 15570 "$tmp/src.tgz" "wecode@127.0.0.1:C:/dev/dianmo-$name.tgz"
"$here/ps.sh" <<PS
\$d = 'C:\dev\dianmo-$name'
New-Item -ItemType Directory -Force \$d | Out-Null
Get-ChildItem \$d -Force | ? Name -ne 'target' | Remove-Item -Recurse -Force
tar -xzf "\$d.tgz" -C \$d
Remove-Item "\$d.tgz"
PS
