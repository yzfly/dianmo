<#
  Non-streaming comparison: run sherpa-onnx-offline.exe (official CLI) on a list and compute CER
  the same way as asr_wav (punctuation/spaces dropped, ASCII upper-cased).

    powershell -ExecutionPolicy Bypass -File offline.ps1 -ModelArgs '--zipformer-ctc-model=...' [-List wavs\aishell.tsv]
#>
param([string]$Root = 'C:\dev\dianmo-asr', [string]$List = 'wavs\aishell.tsv', [Parameter(Mandatory)][string]$ModelArgs,
      [string]$Tokens, [string]$Out = 'results\offline.txt')
$utf8 = New-Object Text.UTF8Encoding $false
Set-Location $Root
function Norm([string]$s) {
  if (-not $s) { return '' } (($s.ToCharArray() | ? { [char]::IsLetterOrDigit($_) }) -join '').ToUpperInvariant() }
function Dist([string]$a, [string]$b) {
  if ($b.Length -eq 0) { return $a.Length }
  if ($a.Length -eq 0) { return $b.Length }
  $p = 0..$b.Length
  for ($i = 1; $i -le $a.Length; $i++) {
    $c = @($i) + (1..$b.Length | % { 0 })
    for ($j = 1; $j -le $b.Length; $j++) {
      $c[$j] = [Math]::Min([Math]::Min($p[$j - 1] + [int]($a[$i - 1] -ne $b[$j - 1]), $p[$j] + 1), $c[$j - 1] + 1)
    }
    $p = $c
  }
  $p[$b.Length]
}
$items = [IO.File]::ReadAllLines((Resolve-Path $List), $utf8) | % { $p, $t = $_ -split "`t"; [pscustomobject]@{ Path = $p; Ref = $t } }
$wavs = ($items | % { "`"$($_.Path)`"" }) -join ' '
$raw = Join-Path $Root "$Out.raw"
$sw = [Diagnostics.Stopwatch]::StartNew()
cmd /c "start `"`" /belownormal /b /wait `"$Root\sherpa\bin\sherpa-onnx-offline.exe`" --tokens=`"$Tokens`" $ModelArgs --num-threads=1 $wavs > `"$raw`" 2>&1"
$wall = $sw.Elapsed.TotalSeconds
$lines = [IO.File]::ReadAllLines($raw, $utf8)
# The CLI prints all paths first, then one JSON result per file in argument order.
$texts = @($lines | ? { $_ -match '^\{.*"text": "([^"]*)"' } | % { $Matches[1] })
$E = 0; $L = 0; $o = @(); $k = 0
foreach ($it in $items) {
  $h = $texts[$k++]; $r = Norm $it.Ref; $e = Dist $r (Norm $h); $E += $e; $L += $r.Length
  $o += "{0}`t{1:N1}%`t{2}" -f (Split-Path $it.Path -Leaf), ($e * 100.0 / [Math]::Max(1, $r.Length)), $h
}
$o += ($lines | Select-String 'created in|Real time factor|Elapsed') | % { "# $_" }
$o += '# CER {0:N2}% ({1}/{2}), wall {3:N1} s' -f ($E * 100.0 / $L), $E, $L, $wall
[IO.File]::WriteAllLines((Join-Path $Root $Out), $o, $utf8)
$o | Select -Last 4
