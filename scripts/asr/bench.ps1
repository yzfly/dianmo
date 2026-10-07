<#
  Run asr_wav.exe for each model at below-normal priority (no window, files only).

    powershell -ExecutionPolicy Bypass -File bench.ps1 [-Root C:\dev\dianmo-asr] [-List wavs\list.tsv]
       [-Models a,b] [-Threads 1] [-Realtime] [-TailMs 0] [-Priority belownormal]

  Expects <Root>\bin\asr_wav.exe (scripts/asr/build.sh copies it there) and the DLLs from fetch.ps1.
  Output: <Root>\results\<model>-t<threads>[-rt].txt, also echoed.
#>
param(
  [string]$Root = 'C:\dev\dianmo-asr',
  [string]$List = 'wavs\list.tsv',
  [string[]]$Models = @(),
  [int]$Threads = 1,
  [switch]$Realtime,
  [int]$TailMs = 0,
  [int]$PadMs = 500,
  [int]$LeadMs = 300,
  [string]$Priority = 'belownormal',
  [string]$Tag = ''
)
$ErrorActionPreference = 'Stop'
Set-Location $Root
$Models = @($Models | % { $_ -split ',' } | ? { $_ })   # run.sh passes "a,b" as one string
if (-not $Models) { $Models = Get-ChildItem models -Directory -Filter 'sherpa-onnx-*streaming*' | % Name }
New-Item -ItemType Directory -Force results | Out-Null
foreach ($m in $Models) {
  $suffix = "-t$Threads-p$PadMs" + $(if ($Realtime) { "-rt$TailMs" } else { "" }) + $Tag
  $out = "results\$m$suffix.txt"
  $a = @("models\$m", "--list", $List, "--threads", $Threads, "--pad-ms", $PadMs, "--lead-ms", $LeadMs, '--dll-dir', "$Root\sherpa\lib")
  if ($Realtime) { $a += @('--realtime', '--tail-ms', $TailMs) }
  $cmd = "start `"`" /$Priority /b /wait `"$Root\bin\asr_wav.exe`" $($a -join ' ') > `"$out`" 2>&1"
  cmd /c $cmd
  "=== $m$suffix"
  Get-Content $out -Encoding UTF8
}
