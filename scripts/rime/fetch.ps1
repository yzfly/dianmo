<#
  Fetch librime + rime-ice data for Dianmo into a shared folder (default C:\dev\dianmo-data).

    powershell -ExecutionPolicy Bypass -File fetch.ps1 [-Root C:\dev\dianmo-data] [-RepoData <repo>\data\rime]

  Result:
    <Root>\librime\rime.dll, rime_deployer.exe, rime_api.h   (librime 1.17.0, MSVC x64)
    <Root>\rime\                                             shared data dir = the app's data\rime
        *.schema.yaml, *.dict.yaml, cn_dicts\, en_dicts\, lua\, opencc\, default.yaml (+ our default.custom.yaml)
        build\   <- filled by stage.ps1 -Probe (probe.exe deploy), not by this script

  Idempotent: downloads are cached in <Root>\downloads (archives checked by SHA-256);
  rime-ice files are re-fetched only when the pinned commit changes.
#>
param(
  [string]$Root = 'C:\dev\dianmo-data',
  [string]$RepoData = ''
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

# ---- pins -------------------------------------------------------------------------------------
$LibrimeTag = '1.17.0'
$LibrimeAsset = 'rime-33e7814-Windows-msvc-x64.7z'
$LibrimeSha = '7478C7CAA4FF6B37DE86DABA1F7CE4A994A4F5BA24872A820FB2B3A9B01FED15'
$DepsAsset = 'rime-deps-33e7814-Windows-msvc-x64.7z'
$DepsSha = '9EF5608D8A54FF52BBAD7A9B4128DE42B232F8E3DD1F5FD3BFF42A0B1BACD7E8'
# rime-ice main @ 2026-10-05 "dict: 增改词汇 (#1636)"
$IceCommit = 'da1fbe602e38f26db846fa10120ee64c2b0324c0'
$IceFiles = @(
  'rime_ice.schema.yaml', 'rime_ice.dict.yaml', 't9.schema.yaml',
  'double_pinyin_flypy.schema.yaml', 'double_pinyin.schema.yaml', 'double_pinyin_mspy.schema.yaml',
  'double_pinyin_sogou.schema.yaml',
  'melt_eng.schema.yaml', 'melt_eng.dict.yaml', 'radical_pinyin.schema.yaml', 'radical_pinyin.dict.yaml',
  'symbols_v.yaml', 'symbols_caps_v.yaml', 'custom_phrase.txt', 'default.yaml',
  'cn_dicts/8105.dict.yaml', 'cn_dicts/base.dict.yaml', 'cn_dicts/ext.dict.yaml', 'cn_dicts/others.dict.yaml',
  'en_dicts/en.dict.yaml', 'en_dicts/en_ext.dict.yaml', 'en_dicts/cn_en.txt', 'en_dicts/cn_en_flypy.txt',
  'en_dicts/cn_en_double_pinyin.txt', 'en_dicts/cn_en_mspy.txt', 'en_dicts/cn_en_sogou.txt',
  'opencc/emoji.json', 'opencc/emoji.txt', 'opencc/others.txt',
  'lua/select_character.lua', 'lua/date_translator.lua', 'lua/convert_ar_num_to_zh.lua', 'lua/lunar.lua',
  'lua/lunar.db', 'lua/uuid.lua', 'lua/unicode.lua', 'lua/number_translator.lua', 'lua/calc_translator.lua',
  'lua/force_gc.lua', 'lua/corrector.lua', 'lua/autocap_filter.lua', 'lua/v_filter.lua',
  'lua/pin_cand_filter.lua', 'lua/long_word_filter.lua', 'lua/reduce_english_filter.lua', 'lua/search.lua'
)
# -----------------------------------------------------------------------------------------------

function Get-File([string]$Url, [string]$Out) {
  New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null
  for ($i = 1; $i -le 3; $i++) {
    try { Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile "$Out.part"; Move-Item -Force "$Out.part" $Out; return }
    catch { if ($i -eq 3) { throw "download failed: $Url : $_" }; Start-Sleep 2 }
  }
}

function Get-Archive([string]$Name, [string]$Sha) {
  $out = Join-Path $Root "downloads\$Name"
  if ((Test-Path $out) -and (Get-FileHash $out -Algorithm SHA256).Hash -eq $Sha) { return $out }
  Get-File "https://github.com/rime/librime/releases/download/$LibrimeTag/$Name" $out
  $h = (Get-FileHash $out -Algorithm SHA256).Hash
  if ($h -ne $Sha) { Remove-Item $out; throw "sha256 mismatch for $Name : $h" }
  return $out
}

# Windows' bsdtar usually lacks LZMA for .7z; fall back to the standalone 7zr.exe from 7-zip.org.
function Expand-7z([string]$Archive, [string]$Dest, [string[]]$Members) {
  New-Item -ItemType Directory -Force $Dest | Out-Null
  # (native stderr would be a terminating error under 'Stop' in PowerShell 5.1)
  $ErrorActionPreference = 'Continue'
  & tar -xf $Archive -C $Dest @Members 2>&1 | Out-Null
  $ok = $LASTEXITCODE -eq 0
  $ErrorActionPreference = 'Stop'
  if ($ok) { return }
  $7zr = Join-Path $Root 'tools\7zr.exe'
  if (-not (Test-Path $7zr)) { Get-File 'https://www.7-zip.org/a/7zr.exe' $7zr }
  & $7zr x -y -bso0 -bsp0 "-o$Dest" $Archive @Members
  if ($LASTEXITCODE -ne 0) { throw "extract failed: $Archive" }
}

New-Item -ItemType Directory -Force $Root, "$Root\downloads", "$Root\librime", "$Root\rime" | Out-Null

# ---- librime ------------------------------------------------------------------------------------
$lib = Join-Path $Root 'librime'
if (-not (Test-Path "$lib\rime.dll") -or (Get-Content "$lib\VERSION" -ErrorAction SilentlyContinue) -ne $LibrimeAsset) {
  $a = Get-Archive $LibrimeAsset $LibrimeSha
  $tmp = Join-Path $Root 'tmp-librime'
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
  Expand-7z $a $tmp @('dist\lib\rime.dll', 'dist\bin\rime_deployer.exe', 'dist\include\rime_api.h', 'version-info.txt')
  Copy-Item "$tmp\dist\lib\rime.dll", "$tmp\dist\bin\rime_deployer.exe", "$tmp\dist\include\rime_api.h", "$tmp\version-info.txt" $lib -Force
  Remove-Item $tmp -Recurse -Force
  Set-Content "$lib\VERSION" $LibrimeAsset
  "librime: $LibrimeAsset"
}

# OpenCC data shipped with librime (s2t.json etc., used by the 简繁 switch).
$data = Join-Path $Root 'rime'
if (-not (Test-Path "$data\opencc\s2t.json")) {
  $a = Get-Archive $DepsAsset $DepsSha
  $tmp = Join-Path $Root 'tmp-deps'
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
  Expand-7z $a $tmp @('share\opencc\*')
  New-Item -ItemType Directory -Force "$data\opencc" | Out-Null
  # only Simplified -> Traditional is reachable from rime-ice's switch
  Get-ChildItem "$tmp\share\opencc" | Where-Object { $_.Name -match '^(s2t\.json|STCharacters\.ocd2|STPhrases\.ocd2)$' } |
    Copy-Item -Destination "$data\opencc" -Force
  Remove-Item $tmp -Recurse -Force
  "opencc: $((Get-ChildItem "$data\opencc").Name -join ', ')"
}

# ---- rime-ice -----------------------------------------------------------------------------------
# A new pinned commit re-fetches everything; otherwise only files added to $IceFiles since.
$sameCommit = (Get-Content "$data\RIME_ICE_COMMIT" -ErrorAction SilentlyContinue) -eq $IceCommit
$fetch = @($IceFiles | Where-Object { -not $sameCommit -or -not (Test-Path (Join-Path $data ($_ -replace '/', '\'))) })
if ($fetch.Count -gt 0) {
  foreach ($f in $fetch) {
    Get-File "https://raw.githubusercontent.com/iDvel/rime-ice/$IceCommit/$f" (Join-Path $data ($f -replace '/', '\'))
  }
  $utf8 = New-Object Text.UTF8Encoding($false)
  function Edit-Text([string]$Path, [scriptblock]$Fn) {
    $t = [IO.File]::ReadAllText($Path, $utf8); $n = & $Fn $t
    if ($n -eq $t) { throw "patch did not apply: $Path" }
    [IO.File]::WriteAllText($Path, $n, $utf8)
  }
  # Skip the 17MB Tencent word-vector dictionary (deploy time + size); base/ext cover daily use.
  if ($fetch -contains 'rime_ice.dict.yaml') {
    Edit-Text "$data\rime_ice.dict.yaml" { param($t) $t -replace '(?m)^(\s*)- cn_dicts/tencent', '$1# - cn_dicts/tencent' }
  }
  # rime-ice PR #1451 added an iOS-only C++ processor (t9_processor) that librime doesn't have.
  if ($fetch -contains 't9.schema.yaml') {
    Edit-Text "$data\t9.schema.yaml" { param($t) $t -replace '(?m)^\s*- t9_processor.*\r?\n', '' }
  }
  Set-Content "$data\RIME_ICE_COMMIT" $IceCommit
  "rime-ice: $IceCommit ($($fetch.Count) of $($IceFiles.Count) files fetched)"
}

# ---- Dianmo's own config (default.custom.yaml etc. from the repo) -------------------------------
if ($RepoData -and (Test-Path $RepoData)) {
  Get-ChildItem $RepoData -File | Where-Object { $_.Name -ne 'README.md' } | Copy-Item -Destination $data -Force
  "config: $((Get-ChildItem $RepoData -File).Name -join ', ')"
}

$size = (Get-ChildItem $data -Recurse -File | Measure-Object Length -Sum).Sum
"shared data: $data ($([math]::Round($size / 1MB, 1)) MB)"
