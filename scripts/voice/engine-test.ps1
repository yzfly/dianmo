<#
  End-to-end test of crates/dianmo/src/voice.rs through examples/voice_probe.exe, in the user's
  desktop session (run by scripts/voice/engine-test.sh via scripts/surface/gui.sh).

  Opens its own Notepad, optionally switches that window's input method (Win+Space x ImeCycles,
  per-window input settings are on, so only this Notepad changes), runs voice_probe (start, wait
  Secs, stop), optionally speaks a phrase with the zh-CN TTS voice so the microphone hears
  something, then reports:
    * voice_probe's state log and session report (counts only)
    * how many characters landed in Notepad and whether they contain a TTS keyword (no content:
      the microphone also hears the real room)
    * whether the clipboard text is the same as before (restores the text itself if not)
  Closes its Notepad without saving.
#>
param(
  [string]$Exe = 'C:\dev\dianmo-voicebuild\target\release\examples\voice_probe.exe',
  [string]$Engine = 'wetype',
  [int]$ImeCycles = 0,
  [double]$Secs = 5,
  [string]$Speak = '',
  [string]$Keyword = '',
  [switch]$Cancel,
  [int]$AfterMs = 0,              # extra wait before reading Notepad (engines that type late)
  [switch]$Medium,                # target Notepad un-elevated (default: elevated, like gui.sh)
  [string]$Extra = '',            # more voice_probe arguments, e.g. '--no-fallback'
  [string]$Out = 'C:\dev\dianmo-voicebuild\target\shots',
  [string]$Tag = 'voice'
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Out | Out-Null
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, System.Speech
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class VT {
  [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit)] public struct U { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public U u; }
  [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] i, int cb);
  [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string title);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, StringBuilder l);
  [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  static INPUT K(ushort vk, bool up) {
    var i = new INPUT(); i.type = 1; i.u.ki.wVk = vk; i.u.ki.wScan = (ushort)MapVirtualKey(vk, 0);
    i.u.ki.dwFlags = (up ? 2u : 0u) | (vk == 0x5B ? 1u : 0u); return i;
  }
  public static void WinSpace() {
    var l = new List<INPUT> { K(0x5B, false), K(0x20, false), K(0x20, true), K(0x5B, true) };
    SendInput((uint)l.Count, l.ToArray(), Marshal.SizeOf(typeof(INPUT)));
  }
  public static string EditText(IntPtr top) {
    IntPtr e = FindWindowEx(top, IntPtr.Zero, "Edit", null);
    if (e == IntPtr.Zero) return null;
    int n = (int)SendMessage(e, 0x000E, IntPtr.Zero, IntPtr.Zero);
    var sb = new StringBuilder(n + 1); SendMessage(e, 0x000D, (IntPtr)(n + 1), sb); return sb.ToString();
  }
}
'@
[VT]::SetProcessDPIAware() | Out-Null

function Shot([string]$name) {
  $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
  $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size); $g.Dispose()
  $bmp.Save("$Out\$Tag-$name.png", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
function ClipText { try { Get-Clipboard -Raw } catch { $null } }

$clip0 = ClipText
"clipboard text before: $(if ($clip0) { "len=$($clip0.Length)" } else { 'none' })"

if ($Medium) {
  # Un-elevated Notepad (explorer starts it with the shell's token); gui.sh itself runs elevated.
  $before = @(Get-Process notepad -ErrorAction SilentlyContinue | % Id)
  Start-Process explorer.exe 'C:\Windows\notepad.exe'
  $np = $null; $t0 = Get-Date
  while (-not $np -and ((Get-Date) - $t0).TotalSeconds -lt 10) { Start-Sleep -Milliseconds 150; $np = Get-Process notepad -ErrorAction SilentlyContinue | ? { $before -notcontains $_.Id -and $_.MainWindowHandle -ne 0 } | select -First 1 }
} else {
  $np = Start-Process notepad -PassThru
  $t0 = Get-Date
  while ($np.MainWindowHandle -eq 0 -and ((Get-Date) - $t0).TotalSeconds -lt 10) { Start-Sleep -Milliseconds 100; $np.Refresh() }
}
$hwnd = $np.MainWindowHandle
$log = "$Out\$Tag-probe.txt"
try {
  [VT]::SetForegroundWindow($hwnd) | Out-Null
  Start-Sleep -Milliseconds 600
  for ($i = 0; $i -lt $ImeCycles; $i++) { [VT]::WinSpace(); Start-Sleep -Milliseconds 800 }
  Shot '0-before'
  $pargs = "--engine $Engine --secs $Secs" + $(if ($Cancel) { ' --cancel' } else { '' }) + " $Extra"
  $p = Start-Process $Exe -ArgumentList $pargs -NoNewWindow -RedirectStandardOutput $log -PassThru
  Start-Sleep -Milliseconds 1500
  Shot '1-listening'
  if ($Speak) {
    $tts = New-Object System.Speech.Synthesis.SpeechSynthesizer
    $tts.SelectVoice('Microsoft Huihui Desktop'); $tts.Volume = 100; $tts.Speak($Speak); $tts.Dispose()
  }
  if (-not $p.WaitForExit(40000)) { "voice_probe did not exit; killing"; $p.Kill() }
  if ($AfterMs) { Start-Sleep -Milliseconds $AfterMs }
  Shot '2-after'
  Get-Content $log -Encoding UTF8 | % { "  | $_" }
  $t = [VT]::EditText($hwnd)
  "notepad: chars=$(if ($t) { $t.Length } else { 0 }) keyword=$(if ($Keyword -and $t) { $t.Contains($Keyword) } else { 'n/a' })"
  "foreground is test notepad: $([VT]::GetForegroundWindow() -eq $hwnd)"
} finally {
  Stop-Process -Id $np.Id -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 300
  $clip1 = ClipText
  $same = ($clip0 -eq $clip1)
  "clipboard text unchanged: $same"
  if (-not $same -and $clip0) { Set-Clipboard -Value $clip0; "clipboard text put back by the test script" }
}
