<#
  Synthesize the fixed test sentences (sentences.txt: id<TAB>text) with the built-in zh-CN voice
  (Microsoft Huihui Desktop) into 16 kHz / 16-bit / mono WAVs. No audio is played, no window opens.

    powershell -ExecutionPolicy Bypass -File tts.ps1 [-Out C:\dev\dianmo-asr\wavs]

  Writes <Out>\<id>.wav (rate 0) and <Out>\<id>-fast.wav (rate +3), plus <Out>\list.tsv
  (path<TAB>reference) for asr_wav.exe --list.
#>
param([string]$Out = 'C:\dev\dianmo-asr\wavs', [string]$Sentences = "$PSScriptRoot\sentences.txt")
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech
New-Item -ItemType Directory -Force $Out | Out-Null
$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000,
  [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
$tts = New-Object System.Speech.Synthesis.SpeechSynthesizer
$tts.SelectVoiceByHints([System.Speech.Synthesis.VoiceGender]::Female, [System.Speech.Synthesis.VoiceAge]::Adult, 0,
  [Globalization.CultureInfo]'zh-CN')
"voice: $($tts.Voice.Name)"
$list = @()
foreach ($line in Get-Content $Sentences -Encoding UTF8) {
  if (-not $line.Trim()) { continue }
  $id, $text = $line -split "`t", 2
  foreach ($v in @(@{ s = ''; r = 0 }, @{ s = '-fast'; r = 3 })) {
    $p = Join-Path $Out "$id$($v.s).wav"
    $tts.Rate = $v.r
    $tts.SetOutputToWaveFile($p, $fmt)
    $tts.Speak($text)
    $tts.SetOutputToNull()
    $list += "$p`t$text"
    '{0,-12} {1,5:N2} s  {2}' -f "$id$($v.s)", (((Get-Item $p).Length - 44) / 32000), $text
  }
}
$tts.Dispose()
[IO.File]::WriteAllLines((Join-Path $Out 'list.tsv'), $list, (New-Object Text.UTF8Encoding $false))
