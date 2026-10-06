#!/bin/bash
# Run a PowerShell script (from stdin) on the Surface via the reverse tunnel.
# All output streams come back as plain UTF-8 text on stdout.
#   scripts/surface/ps.sh <<'PS'
#   Get-Process | Select -First 3
#   PS
body=$(cat)
wrapped=$(printf '%s\n' \
  '[Console]::OutputEncoding=[Text.Encoding]::UTF8; $ProgressPreference="SilentlyContinue"' \
  '& {' "$body" '} *>&1 | ForEach-Object { "$_" }')
b64=$(printf '%s' "$wrapped" | iconv -f UTF-8 -t UTF-16LE | base64 -w0)
exec ssh -p 15570 -o ConnectTimeout=15 -o ServerAliveInterval=30 wecode@127.0.0.1 \
  "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $b64"
