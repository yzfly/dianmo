<#
  Download a spread-out sample of the AISHELL-1 test set (real read speech, official transcripts)
  through the Hugging Face datasets-server API (dataset AudioLLMs/aishell_1_zh_test, 6920 rows,
  ~20 speakers in order), so the sample covers many speakers.

    powershell -ExecutionPolicy Bypass -File aishell.ps1 [-Out C:\dev\dianmo-asr\wavs\aishell] [-Count 39] [-Step 179]

  Writes <Out>\aishell-<row>.wav and <Out>\..\aishell.tsv (path<TAB>reference).
  The API rate-limits (HTTP 429): the script waits and retries; rerunning skips finished rows.
#>
param([string]$Out = 'C:\dev\dianmo-asr\wavs\aishell', [int]$Count = 39, [int]$Step = 179)
$ProgressPreference = 'SilentlyContinue'
New-Item -ItemType Directory -Force $Out | Out-Null
$api = 'https://datasets-server.huggingface.co/rows?dataset=AudioLLMs/aishell_1_zh_test&config=default&split=test'
$utf8 = New-Object Text.UTF8Encoding $false
$list = @()
for ($i = 0; $i -lt $Count; $i++) {
  $off = $i * $Step
  $p = Join-Path $Out ('aishell-{0:D4}.wav' -f $off)
  $txt = [IO.Path]::ChangeExtension($p, '.txt')
  if (-not ((Test-Path $p) -and (Test-Path $txt))) {
    $json = Join-Path $Out 'row.json'
    $ok = $false
    for ($t = 0; $t -lt 6 -and -not $ok; $t++) {
      & curl.exe -s --fail --max-time 60 -o $json "$api&offset=$off&length=1"
      if ($LASTEXITCODE -eq 0) { $ok = $true } else { Start-Sleep 10 }
    }
    Start-Sleep -Milliseconds 1500
    if (-not $ok) { "skip $off (API error / rate limited)"; continue }
    # read as UTF-8 explicitly (Windows PowerShell would assume the ANSI code page)
    $rows = ([IO.File]::ReadAllText($json, $utf8) | ConvertFrom-Json).rows
    if (-not $rows) { "no row $off"; continue }
    $row = $rows[0].row
    & curl.exe -s --fail -o $p $row.context[0].src
    if ($LASTEXITCODE -ne 0) { "download failed $off"; continue }
    [IO.File]::WriteAllText($txt, $row.answer, $utf8)
  }
  $list += "$p`t$([IO.File]::ReadAllText($txt, $utf8).Trim())"
}
[IO.File]::WriteAllLines((Join-Path (Split-Path $Out) 'aishell.tsv'), $list, $utf8)
"$($list.Count) files"
