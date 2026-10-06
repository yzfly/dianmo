<#
  Assemble what Dianmo needs at runtime for the Rime engine into <Out>:

    powershell -ExecutionPolicy Bypass -File stage.ps1 -Out <dir> [-Data C:\dev\dianmo-data] [-Probe <probe.exe>] [-Verify]

  Result (only runtime files; schema/dictionary sources are not shipped):
    <Out>\rime.dll
    <Out>\data\rime\build\        precompiled schemas + dictionaries (*.schema.yaml, *.bin, default.yaml)
    <Out>\data\rime\lua\          rime-ice Lua scripts + lunar.db
    <Out>\data\rime\opencc\       emoji + Simplified->Traditional
    <Out>\data\rime\custom_phrase.txt, en_dicts\cn_en.txt, en_dicts\cn_en_flypy.txt
                                  (plain-text dictionaries librime reads directly from the shared dir)
    <Out>\data\rime\RIME_ICE_COMMIT

  <Data> is what scripts\rime\fetch.ps1 produced. The precompiled set lives in <Data>\rime\build:
  - with -Probe, it is rebuilt when missing or older than any source file (probe.exe deploy <Data>\rime);
  - without -Probe, an existing up-to-date build\ is required.
  -Verify (needs -Probe) runs `probe run` against the staged copy.
  <Out>\data\rime is replaced; nothing else in <Out> is touched.
#>
param(
  [Parameter(Mandatory = $true)][string]$Out,
  [string]$Data = 'C:\dev\dianmo-data',
  [string]$Probe = '',
  [switch]$Verify
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$dll = Join-Path $Data 'librime\rime.dll'
$shared = Join-Path $Data 'rime'
$build = Join-Path $shared 'build'
foreach ($p in $dll, (Join-Path $shared 'default.yaml'), (Join-Path $shared 'default.custom.yaml')) {
  if (-not (Test-Path $p)) { throw "missing $p (run scripts\rime\fetch.ps1 -RepoData <repo>\data\rime first)" }
}
if ($Probe -and -not (Test-Path $Probe)) { throw "probe not found: $Probe" }

# Runs a console exe at below-normal priority, output to this console; throws on failure.
function Invoke-Low([string]$Exe, [string[]]$Arguments) {
  $quoted = ($Arguments | ForEach-Object { '"' + $_ + '"' }) -join ' '
  $ErrorActionPreference = 'Continue'
  cmd /c "start `"`" /belownormal /b /wait `"$Exe`" $quoted 2>&1"
  $code = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($code -ne 0) { throw "$Exe failed (exit $code)" }
}

# One stager at a time (deploy rewrites <Data>\rime\build in place).
$mutex = New-Object Threading.Mutex($false, 'Local\DianmoRimeStage')
[void]$mutex.WaitOne()
try {
  # ---- precompiled build\ ---------------------------------------------------------------------
  $stamp = Join-Path $build 'default.yaml'
  $sources = Get-ChildItem $shared -Recurse -File -Include *.yaml, *.txt |
    Where-Object { $_.FullName -notlike "$build\*" }
  $stale = -not (Test-Path $stamp)
  if (-not $stale) {
    $built = (Get-Item $stamp).LastWriteTimeUtc
    $newer = $sources | Where-Object { $_.LastWriteTimeUtc -gt $built } | Select-Object -First 1
    if ($newer) { $stale = $true; "build\ is older than $($newer.Name)" }
  }
  if ($stale) {
    if (-not $Probe) { throw "$build is missing or stale; pass -Probe <probe.exe> to rebuild it" }
    "deploying $shared ..."
    Invoke-Low $Probe @('deploy', $shared, '--dll', $dll)
  } else {
    "build\ is up to date"
  }

  # ---- copy the runtime set --------------------------------------------------------------------
  $outData = Join-Path $Out 'data\rime'
  if (Test-Path $outData) { Remove-Item $outData -Recurse -Force }
  New-Item -ItemType Directory -Force $outData, (Join-Path $outData 'en_dicts') | Out-Null
  Copy-Item $dll (Join-Path $Out 'rime.dll') -Force
  Copy-Item $build (Join-Path $outData 'build') -Recurse
  Copy-Item (Join-Path $shared 'lua') (Join-Path $outData 'lua') -Recurse
  Copy-Item (Join-Path $shared 'opencc') (Join-Path $outData 'opencc') -Recurse
  foreach ($f in 'custom_phrase.txt', 'RIME_ICE_COMMIT') {
    Copy-Item (Join-Path $shared $f) $outData
  }
  Copy-Item (Join-Path $shared 'en_dicts\cn_en*.txt') (Join-Path $outData 'en_dicts')

  $files = Get-ChildItem $outData -Recurse -File
  $mb = [math]::Round((($files | Measure-Object Length -Sum).Sum + (Get-Item (Join-Path $Out 'rime.dll')).Length) / 1MB, 1)
  "staged: $Out\rime.dll + $outData ($($files.Count) files, $mb MB total)"
} finally {
  $mutex.ReleaseMutex()
}

if ($Verify) {
  if (-not $Probe) { throw '-Verify needs -Probe' }
  Invoke-Low $Probe @('run', '--shared', $outData, '--dll', (Join-Path $Out 'rime.dll'), '--bench', '5')
}
