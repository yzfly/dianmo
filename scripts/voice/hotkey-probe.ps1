<#
  Probe whether a third-party voice input (WeType / Doubao / DouBaoVoice ...) reacts to a hotkey
  injected with SendInput (the same way dianmo would send it).

  Runs inside the user's desktop session (scripts/voice/probe.sh uploads it and runs it via
  scripts/surface/gui.sh). Opens its own Notepad, focuses it, optionally switches that window's
  input method, injects <Keys> down - hold - up, and records evidence:
    * screenshots (before / during hold / after release) in $Out
    * visible top-level windows owned by the voice-related processes while holding
    * apps currently using the microphone (CapabilityAccessManager, LastUsedTimeStop = 0)
    * Notepad's text afterwards, and the foreground window (Start menu must not pop up)
  Then closes its Notepad without saving and restores whatever it switched.

  Keys: names joined with '+', pressed in order, released in reverse.
    LCtrl RCtrl LShift RShift LAlt RAlt LWin RWin Space F1..F24 Esc Enter Back or 0xNN
  Mode: hold  = down, wait HoldMs, up
        tap   = down/up, wait HoldMs, down/up again (toggle-style hotkeys)
#>
param(
  [string]$Keys = 'LCtrl+LWin',
  [int]$HoldMs = 3000,
  [ValidateSet('hold', 'tap')][string]$Mode = 'hold',
  [int]$ImeCycles = 0,          # how many Win+Space presses to switch Notepad's input method first
  [string]$Tag = 'probe',
  [string]$Out = 'C:\dev\dianmo-voice\shots',
  [string]$TypeTest = '',       # if set: after the hotkey test, run the Back/Enter compatibility test
  [int]$AfterMs = 1500,         # wait after stopping before reading the result (cloud engines finish late)
  [ValidateSet('', 'type', 'switch', 'esc')][string]$During = ''   # mid-recording: dianmo types text / focus moves to a 2nd Notepad / Esc (cancel, no 2nd tap)
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Out | Out-Null

Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class P {
  [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit)] public struct U { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public U u; }
  [DllImport("user32.dll", SetLastError=true)] static extern uint SendInput(uint n, INPUT[] i, int cb);
  [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint tid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int l, t, r, b; }
  delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string title);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, StringBuilder l);
  [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
  [DllImport("user32.dll", SetLastError=true)] public static extern bool SystemParametersInfo(uint a, uint b, ref bool c, uint d);

  static bool IsExt(ushort vk) { return vk == 0xA3 || vk == 0xA5 || vk == 0x5B || vk == 0x5C || (vk >= 0x21 && vk <= 0x2E); }
  static INPUT K(ushort vk, bool up) {
    var i = new INPUT(); i.type = 1; i.u.ki.wVk = vk; i.u.ki.wScan = (ushort)MapVirtualKey(vk, 0);
    i.u.ki.dwFlags = (up ? 2u : 0u) | (IsExt(vk) ? 1u : 0u); return i;
  }
  public static uint Keys(ushort[] vks, bool up) {
    var l = new List<INPUT>();
    if (up) for (int k = vks.Length - 1; k >= 0; k--) l.Add(K(vks[k], true)); else foreach (var v in vks) l.Add(K(v, false));
    return SendInput((uint)l.Count, l.ToArray(), Marshal.SizeOf(typeof(INPUT)));
  }
  public static uint Text(string s) {
    var l = new List<INPUT>();
    foreach (char c in s) { foreach (bool up in new[]{false, true}) { var i = new INPUT(); i.type = 1; i.u.ki.wScan = c; i.u.ki.dwFlags = 4u | (up ? 2u : 0u); l.Add(i); } }
    return SendInput((uint)l.Count, l.ToArray(), Marshal.SizeOf(typeof(INPUT)));
  }
  public static string Describe(IntPtr h) {
    var c = new StringBuilder(256); GetClassName(h, c, 256); var t = new StringBuilder(256); GetWindowText(h, t, 256);
    uint pid; GetWindowThreadProcessId(h, out pid); RECT r; GetWindowRect(h, out r);
    return string.Format("pid={0} class={1} title=\"{2}\" rect=({3},{4})-({5},{6})", pid, c, t, r.l, r.t, r.r, r.b);
  }
  public static List<string> VisibleWindowsOf(HashSet<uint> pids) {
    var res = new List<string>();
    EnumWindows((h, p) => { uint pid; GetWindowThreadProcessId(h, out pid); if (pids.Contains(pid) && IsWindowVisible(h)) res.Add(Describe(h)); return true; }, IntPtr.Zero);
    return res;
  }
  public static string EditText(IntPtr top) {
    IntPtr e = FindWindowEx(top, IntPtr.Zero, "Edit", null);
    if (e == IntPtr.Zero) return "<no Edit>";
    int n = (int)SendMessage(e, 0x000E, IntPtr.Zero, IntPtr.Zero);
    var sb = new StringBuilder(n + 1); SendMessage(e, 0x000D, (IntPtr)(n + 1), sb); return sb.ToString();
  }
  public static bool ThreadLocalInput() { bool v = false; SystemParametersInfo(0x104E, 0, ref v, 0); return v; }
}
'@
[P]::SetProcessDPIAware() | Out-Null

$vkNames = @{ LCtrl = 0xA2; RCtrl = 0xA3; LShift = 0xA0; RShift = 0xA1; LAlt = 0xA4; RAlt = 0xA5; LWin = 0x5B; RWin = 0x5C;
  Space = 0x20; Esc = 0x1B; Enter = 0x0D; Back = 0x08 }
function Vk([string]$n) {
  if ($vkNames.ContainsKey($n)) { return [uint16]$vkNames[$n] }
  if ($n -match '^F(\d+)$') { return [uint16](0x6F + [int]$Matches[1]) }
  if ($n -match '^0x[0-9a-fA-F]+$') { return [uint16][Convert]::ToInt32($n, 16) }
  throw "unknown key $n"
}
$vks = [uint16[]]($Keys -split '\+' | % { Vk $_ })

function Shot([string]$name) {
  $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
  $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size); $g.Dispose()
  $bmp.Save("$Out\$Tag-$name.png", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
function MicInUse {
  $root = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone'
  Get-ChildItem $root, "$root\NonPackaged" -ErrorAction SilentlyContinue | % {
    $p = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
    if ($p -and $p.LastUsedTimeStart -and $p.LastUsedTimeStop -eq 0) { $_.PSChildName }
  }
}
function VoicePids {
  $set = New-Object 'System.Collections.Generic.HashSet[uint32]'
  Get-Process | ? { $_.Name -match '(?i)wetype|doubao|sogou|ifly|voice' } | % { [void]$set.Add([uint32]$_.Id) }
  , $set
}
# Recognized speech is the user's room audio: report only how many characters were committed.
function Report([string]$name, $h) {
  $t = [P]::EditText($h)
  $probe = ($t -split 'probe:').Count - 1
  $other = ($t -replace 'probe:', '' -replace '\[Z\]', '').Length
  "${name}: probe-markers=$probe dianmo-marker=$($t.Contains('[Z]')) other-chars=$other"
}
function Fg { [P]::Describe([P]::GetForegroundWindow()) }

"keys=$Keys vks=$($vks -join ',') mode=$Mode hold=$HoldMs imeCycles=$ImeCycles threadLocalInput=$([P]::ThreadLocalInput())"

$np = Start-Process notepad -PassThru
$t0 = Get-Date
while ($np.MainWindowHandle -eq 0 -and ((Get-Date) - $t0).TotalSeconds -lt 10) { Start-Sleep -Milliseconds 100; $np.Refresh() }
$hwnd = $np.MainWindowHandle
try {
  [P]::SetForegroundWindow($hwnd) | Out-Null
  Start-Sleep -Milliseconds 600
  "foreground: $(Fg)"
  $tid = [P]::GetWindowThreadProcessId($hwnd, [ref]0)
  "notepad HKL before: 0x{0:X}" -f [int64][P]::GetKeyboardLayout($tid)

  for ($i = 0; $i -lt $ImeCycles; $i++) {
    [P]::Keys([uint16[]](0x5B, 0x20), $false) | Out-Null; Start-Sleep -Milliseconds 150
    [P]::Keys([uint16[]](0x5B, 0x20), $true) | Out-Null; Start-Sleep -Milliseconds 700
  }
  [P]::Text('probe:') | Out-Null
  Start-Sleep -Milliseconds 400
  Shot '0-before'
  "mic in use before: $((MicInUse) -join '; ')"

  $pids = VoicePids
  $clip0 = [P]::GetClipboardSequenceNumber()
  $sent = [P]::Keys($vks, $false); "down sent=$sent"
  if ($Mode -eq 'tap') { Start-Sleep -Milliseconds 60; [P]::Keys($vks, $true) | Out-Null }
  Start-Sleep -Milliseconds 900
  Shot '1-hold'
  "voice windows (hold): "; [P]::VisibleWindowsOf($pids) | % { "  $_" }
  "mic in use (hold): $((MicInUse) -join '; ')"
  "foreground (hold): $(Fg)"
  $np2 = $null
  if ($During -eq 'type') { [P]::Text('[Z]') | Out-Null; Start-Sleep -Milliseconds 500; "typed [Z] mid-recording"; Shot '1b-typed'; "voice windows (after typing): "; [P]::VisibleWindowsOf($pids) | % { "  $_" } }
  if ($During -eq 'esc') { [P]::Keys([uint16[]](0x1B), $false) | Out-Null; [P]::Keys([uint16[]](0x1B), $true) | Out-Null; Start-Sleep -Milliseconds 500; "sent Esc mid-recording"; "voice windows (after Esc): "; [P]::VisibleWindowsOf($pids) | % { "  $_" }; "mic in use (after Esc): $((MicInUse) -join '; ')" }
  if ($During -eq 'switch') {
    $np2 = Start-Process notepad -PassThru; $t1 = Get-Date
    while ($np2.MainWindowHandle -eq 0 -and ((Get-Date) - $t1).TotalSeconds -lt 10) { Start-Sleep -Milliseconds 100; $np2.Refresh() }
    [P]::SetForegroundWindow($np2.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 600
    "switched focus mid-recording: $(Fg)"; Shot '1b-switched'; "voice windows (after switch): "; [P]::VisibleWindowsOf($pids) | % { "  $_" }
  }
  Start-Sleep -Milliseconds ([Math]::Max(0, $HoldMs - 900))
  Shot '2-hold-end'
  if ($Mode -eq 'hold') { [P]::Keys($vks, $true) | Out-Null }
  elseif ($During -ne 'esc') { [P]::Keys($vks, $false) | Out-Null; Start-Sleep -Milliseconds 60; [P]::Keys($vks, $true) | Out-Null }
  "released"
  $tr = Get-Date; $n0 = ([P]::EditText($hwnd)).Length
  while (((Get-Date) - $tr).TotalMilliseconds -lt $AfterMs) {
    Start-Sleep -Milliseconds 100
    if ($n0 -ge 0 -and ([P]::EditText($hwnd)).Length -ne $n0) { "text arrived $([int]((Get-Date) - $tr).TotalMilliseconds) ms after stop"; $n0 = -1 }
  }
  "clipboard changed during test: $([P]::GetClipboardSequenceNumber() -ne $clip0)"
  Shot '3-after'
  "voice windows (after): "; [P]::VisibleWindowsOf($pids) | % { "  $_" }
  "mic in use (after): $((MicInUse) -join '; ')"
  "foreground (after): $(Fg)"
  Report 'notepad' $hwnd
  if ($np2) { Report 'notepad2' $np2.MainWindowHandle }

  if ($TypeTest) {
    [P]::SetForegroundWindow($hwnd) | Out-Null; Start-Sleep -Milliseconds 300
    # dianmo-style output: Unicode text, then real Back / Enter / arrows / Ctrl+A
    [P]::Text("|你好abc") | Out-Null; Start-Sleep -Milliseconds 200
    [P]::Keys([uint16[]](0x08), $false) | Out-Null; [P]::Keys([uint16[]](0x08), $true) | Out-Null
    [P]::Keys([uint16[]](0x08), $false) | Out-Null; [P]::Keys([uint16[]](0x08), $true) | Out-Null
    [P]::Keys([uint16[]](0x0D), $false) | Out-Null; [P]::Keys([uint16[]](0x0D), $true) | Out-Null
    [P]::Text("x") | Out-Null
    [P]::Keys([uint16[]](0x25), $false) | Out-Null; [P]::Keys([uint16[]](0x25), $true) | Out-Null
    [P]::Text("y") | Out-Null
    [P]::Keys([uint16[]](0x20), $false) | Out-Null; [P]::Keys([uint16[]](0x20), $true) | Out-Null
    Start-Sleep -Milliseconds 500
    Shot '4-typetest'
    $t = [P]::EditText($hwnd); $i = $t.LastIndexOf('|')
    "typetest tail: [$(($t.Substring([Math]::Max(0,$i))) -replace "`r`n", '\n')]  (expected: [|你好a\ny x])"
  }

  for ($i = 0; $i -lt $ImeCycles; $i++) {
    # cycle back: Win+Shift+Space goes to the previous input method
    [P]::SetForegroundWindow($hwnd) | Out-Null
    [P]::Keys([uint16[]](0x5B, 0xA0, 0x20), $false) | Out-Null; Start-Sleep -Milliseconds 150
    [P]::Keys([uint16[]](0x5B, 0xA0, 0x20), $true) | Out-Null; Start-Sleep -Milliseconds 700
  }
}
finally {
  Stop-Process -Id $np.Id -Force -ErrorAction SilentlyContinue
  if ($np2) { Stop-Process -Id $np2.Id -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds 300
  "foreground (end): $(Fg)"
}
