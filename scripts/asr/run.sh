#!/bin/bash
# Run bench.ps1 on the Surface with the given arguments, e.g.
#   scripts/asr/run.sh -Models sherpa-onnx-streaming-zipformer-small-ctc-zh-int8-2025-04-01 -Realtime
here=$(cd "$(dirname "$0")" && pwd)
"$here/../surface/ps.sh" <<PS
powershell -NoProfile -ExecutionPolicy Bypass -File C:\dev\dianmo-asr\scripts\bench.ps1 $*
PS
