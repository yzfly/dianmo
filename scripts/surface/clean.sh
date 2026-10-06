#!/bin/bash
# Remove a Surface work dir (or just its target\ with --target).
#   scripts/surface/clean.sh <name> [--target]
set -euo pipefail
name=${1:?usage: clean.sh <name> [--target]}
sub=$([ "${2:-}" = --target ] && echo '\target' || true)
"$(dirname "$0")/ps.sh" <<PS
Remove-Item 'C:\dev\dianmo-$name$sub' -Recurse -Force -ErrorAction SilentlyContinue
Get-PSDrive C | % { "C: free \$([math]::Round(\$_.Free/1GB))G" }
PS
