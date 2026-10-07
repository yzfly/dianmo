<#
  Fetch sherpa-onnx (prebuilt Windows x64, MSVC /MT, no TTS) + small Chinese ASR models into C:\dev\dianmo-asr.

    powershell -ExecutionPolicy Bypass -File fetch.ps1 [-Root C:\dev\dianmo-asr] [-Models a,b,...]

  Layout:
    <Root>\downloads\*.tar.bz2         cached archives
    <Root>\sherpa\bin\*.exe, lib\*.dll  sherpa-onnx-c-api.dll + onnxruntime.dll + CLI tools
    <Root>\models\<model-name>\...     extracted model dirs
    <Root>\models\silero_vad.onnx

  Source: GitHub releases of k2-fsa/sherpa-onnx (tags asr-models and v<version>); from the Surface
  this ran at 4-5 MB/s, so no mirror was needed. Mirrors with the same files unpacked:
  hf-mirror.com/csukuangfj/<model-name> (most models), modelscope.cn (some).
#>
param(
  [string]$Root = 'C:\dev\dianmo-asr',
  [string[]]$Models = @(
    'sherpa-onnx-streaming-zipformer-small-ctc-zh-int8-2025-04-01',
    'sherpa-onnx-streaming-zipformer-zh-14M-2023-02-23',
    'sherpa-onnx-streaming-zipformer-ctc-multi-zh-hans-int8-2023-12-13',
    'sherpa-onnx-x-asr-480ms-streaming-zipformer-transducer-zh-en-punct-int8-2026-06-05',
    'sherpa-onnx-zipformer-ctc-small-zh-int8-2025-07-16'   # non-streaming, for offline.ps1
  )
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$SherpaVer = '1.13.8'
$SherpaPkg = "sherpa-onnx-v$SherpaVer-win-x64-shared-MT-Release-no-tts"
$Gh = 'https://github.com/k2-fsa/sherpa-onnx/releases/download'

function Get-File([string]$Url, [string]$Out) {
  if (Test-Path $Out) { return }
  New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
  $sw = [Diagnostics.Stopwatch]::StartNew()
  for ($i = 1; $i -le 3; $i++) {
    & curl.exe -sSL --fail --retry 2 --connect-timeout 20 -o "$Out.part" $Url
    if ($LASTEXITCODE -eq 0) { Move-Item -Force "$Out.part" $Out; break }
    if ($i -eq 3) { throw "download failed: $Url" }
    Start-Sleep 3
  }
  $mb = (Get-Item $Out).Length / 1MB
  '{0,-90} {1,7:N1} MB {2,6:N1} s  <- {3}' -f (Split-Path $Out -Leaf), $mb, $sw.Elapsed.TotalSeconds, $Url
}

# Win10's bsdtar has no bzip2 filter: unpack .bz2 with 7-Zip (installed on the Surface), then the .tar with tar.exe.
$7z = 'C:\Program Files\7-Zip\7z.exe'
function Expand-To([string]$Archive, [string]$Dest) {
  New-Item -ItemType Directory -Force $Dest | Out-Null
  $tmp = Join-Path $dl 'tmp'
  if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp }
  & $7z x -y -bso0 -bsp0 "-o$tmp" $Archive
  if ($LASTEXITCODE -ne 0) { throw "7z failed: $Archive" }
  $tar = Get-ChildItem $tmp -Filter *.tar | Select -First 1
  & tar.exe -xf $tar.FullName -C $Dest
  if ($LASTEXITCODE -ne 0) { throw "tar failed: $Archive" }
  Remove-Item -Recurse -Force $tmp
}

$dl = Join-Path $Root 'downloads'
# sherpa-onnx runtime
$a = Join-Path $dl "$SherpaPkg.tar.bz2"
Get-File "$Gh/v$SherpaVer/$SherpaPkg.tar.bz2" $a
if (-not (Test-Path (Join-Path $Root 'sherpa\lib\sherpa-onnx-c-api.dll'))) {
  Expand-To $a $dl
  if (Test-Path (Join-Path $Root 'sherpa')) { Remove-Item -Recurse -Force (Join-Path $Root 'sherpa') }
  Move-Item (Join-Path $dl $SherpaPkg) (Join-Path $Root 'sherpa')
}
# VAD
Get-File "$Gh/asr-models/silero_vad.onnx" (Join-Path $Root 'models\silero_vad.onnx')
# models
foreach ($m in $Models) {
  $a = Join-Path $dl "$m.tar.bz2"
  Get-File "$Gh/asr-models/$m.tar.bz2" $a
  if (-not (Test-Path (Join-Path $Root "models\$m"))) { Expand-To $a (Join-Path $Root 'models') }
}
Get-ChildItem (Join-Path $Root 'sherpa') -Recurse -File | ? Extension -in '.dll', '.exe' |
  % { '{0,-50} {1,8:N2} MB' -f $_.Name, ($_.Length / 1MB) }
foreach ($m in $Models) {
  "== $m"
  Get-ChildItem (Join-Path $Root "models\$m") -Recurse -File | % { '  {0,-60} {1,8:N2} MB' -f $_.FullName.Substring($Root.Length + 8 + $m.Length), ($_.Length / 1MB) }
}
