# App-window test of window_demo (dot-sourced after touch.ps1 by run-window.sh).
# Only touches windows of the demo process it starts; closes it with WM_CLOSE.
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Threading; using System.Runtime.InteropServices;
public static class W {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINTER_INFO {
    public int pointerType; public uint pointerId; public uint frameId; public int pointerFlags;
    public IntPtr sourceDevice; public IntPtr hwndTarget; public POINT ptPixelLocation; public POINT ptHimetricLocation;
    public POINT ptPixelLocationRaw; public POINT ptHimetricLocationRaw; public uint dwTime; public uint historyCount;
    public int InputData; public uint dwKeyStates; public ulong PerformanceCount; public int ButtonChangeType; }
  [StructLayout(LayoutKind.Sequential)] public struct POINTER_TOUCH_INFO { public POINTER_INFO pointerInfo; public int touchFlags; public int touchMask; public RECT rcContact; public RECT rcContactRaw; public uint orientation; public uint pressure; }
  [DllImport("user32.dll", SetLastError=true)] public static extern bool InjectTouchInput(uint count, [In] POINTER_TOUCH_INFO[] contacts);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, int d, IntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr e);
  [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint f);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int n);
  public static string Under(int x, int y) { var h = GetAncestor(WindowFromPoint(new POINT{x=x,y=y}), 2); uint pid; GetWindowThreadProcessId(h, out pid); var sb = new System.Text.StringBuilder(256); GetClassName(h, sb, 256); return sb.ToString() + "/" + pid; }
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out int v, int cb);
  public static int[] Client(IntPtr h) { RECT r; GetClientRect(h, out r); var p = new POINT(); ClientToScreen(h, ref p); return new[]{p.x, p.y, r.right, r.bottom}; }
  public static int[] Outer(IntPtr h) { RECT r; GetWindowRect(h, out r); return new[]{r.left, r.top, r.right - r.left, r.bottom - r.top}; }
  const int INRANGE=0x2, INCONTACT=0x4, DOWN=0x10000, UPDATE=0x20000, UP=0x40000;
  static POINTER_TOUCH_INFO Make(int x, int y, int flags) {
    var c = new POINTER_TOUCH_INFO(); c.pointerInfo.pointerType = 2; c.pointerInfo.pointerId = 0;
    c.pointerInfo.ptPixelLocation = new POINT{x=x,y=y}; c.pointerInfo.pointerFlags = flags;
    c.touchMask = 0x7; c.rcContact = new RECT{left=x-6, top=y-6, right=x+6, bottom=y+6}; c.orientation = 90; c.pressure = 32000; return c; }
  // Finger drag from (x,y) by dy px in `steps` frames of 16 ms, then lift (a flick when fast).
  public static bool Drag(int x, int y, int dy, int steps) {
    if (!InjectTouchInput(1, new[]{Make(x,y, DOWN|INRANGE|INCONTACT)})) return false; Thread.Sleep(30);
    for (int i = 1; i <= steps; i++) { InjectTouchInput(1, new[]{Make(x, y + dy*i/steps, UPDATE|INRANGE|INCONTACT)}); Thread.Sleep(16); }
    return InjectTouchInput(1, new[]{Make(x, y+dy, UP)}); }
}
'@
$exe = 'C:\dev\dianmo-appwin\target\release\examples\window_demo.exe'
$log = 'C:\Users\wecode\claude\appwin.log'
Remove-Item $log -ErrorAction SilentlyContinue
$env:DIANMO_DEMO_LOG = $log

$p = Start-Process $exe -PassThru
function Mem { $p.Refresh(); "{0:N1}MB private, {1:N1}MB ws" -f ($p.PrivateMemorySize64/1MB), ($p.WorkingSet64/1MB) }
function Cpu { $p.Refresh(); $p.TotalProcessorTime.TotalMilliseconds }
function Client($h) { $a = [W]::Client($h); @{x=$a[0]; y=$a[1]; w=$a[2]; h=$a[3]} }
function Shot($h, $name) {
  $r = [W]::Outer($h)
  $bmp = New-Object System.Drawing.Bitmap $r[2], $r[3]
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($r[0], $r[1], 0, 0, $bmp.Size)
  $bmp.Save("C:\Users\wecode\claude\$name.png", [System.Drawing.Imaging.ImageFormat]::Png) }
$kb = [IntPtr]::Zero
for ($i = 0; $i -lt 50 -and $kb -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 100; $kb = [T]::FindOf('DianmoKeyboard', $p.Id) }
if ($kb -eq [IntPtr]::Zero) { "no keyboard window"; $p.Kill(); return }
Start-Sleep -Milliseconds 800
"start: $(Mem)"
$k = Client $kb; $s = [T]::GetDpiForWindow($kb) / 96
function StripTap($i) {
  $k = Client $kb; $x = [int]($k.x + (92 + 172*$i)*$s); $y = [int]($k.y + 32*$s)
  "  strip tap $i at $x,$y on $([W]::Under($x, $y)) (demo pid $($p.Id))"
  [T]::Tap($x, $y) | Out-Null }

# 1) open from the (non-activating) keyboard strip: the window must come to the front
"work area $(Fmt ([T]::Work())) keyboard $(Fmt (& { $r = New-Object T+RECT; [T]::GetWindowRect($kb, [ref]$r) | Out-Null; $r }))"
StripTap 0; Start-Sleep -Milliseconds 900
$win = [T]::FindOf('DianmoAppWindow', $p.Id)
if ($win -eq [IntPtr]::Zero) { "no app window"; [T]::PostMessage($kb, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null; return }
$c = Client $win; $ws = [T]::GetDpiForWindow($win) / 96
$dark = 0; [W]::DwmGetWindowAttribute($win, 20, [ref]$dark, 4) | Out-Null
"window outer $([W]::Outer($win) -join ',')"
"opened: foreground=$([T]::GetForegroundWindow() -eq $win) dpi=$([T]::GetDpiForWindow($win)) client=$($c.w)x$($c.h) darkattr=$dark $(Mem)"
Shot $win 'appwin-light'
function At($dx, $dy) { @([int]($c.x + $dx*$ws), [int]($c.y + $dy*$ws)) }

# 2) finger drag up 600 px fast (flick), then wait for the fling to end
$a = At 380 420; [W]::Drag($a[0], $a[1], -600, 12) | Out-Null; Start-Sleep -Milliseconds 1500
# 3) tap a row (no movement) and a second drag back down slowly (no fling)
$a = At 380 300; [T]::Tap($a[0], $a[1]) | Out-Null; Start-Sleep -Milliseconds 300
$a = At 380 200; [W]::Drag($a[0], $a[1], 300, 40) | Out-Null; Start-Sleep -Milliseconds 400
# 4) mouse: hover a row, wheel down twice and up once
$a = At 380 400; [W]::SetCursorPos($a[0], $a[1]) | Out-Null; Start-Sleep -Milliseconds 150
$a = At 382 402; [W]::SetCursorPos($a[0], $a[1]) | Out-Null; Start-Sleep -Milliseconds 150
[W]::mouse_event(0x800, 0, 0, -120, [IntPtr]::Zero); Start-Sleep -Milliseconds 120
[W]::mouse_event(0x800, 0, 0, -120, [IntPtr]::Zero); Start-Sleep -Milliseconds 120
[W]::mouse_event(0x800, 0, 0, 120, [IntPtr]::Zero); Start-Sleep -Milliseconds 200
# 5) keys (only while our window is in front)
if ([T]::GetForegroundWindow() -eq $win) {
  foreach ($vk in 0x28, 0x22) { [W]::keybd_event($vk, 0, 0, [IntPtr]::Zero); [W]::keybd_event($vk, 0, 2, [IntPtr]::Zero); Start-Sleep -Milliseconds 120 }
} else { "keys skipped: not foreground" }
# 6) idle CPU with the window open
Start-Sleep -Milliseconds 500; $t0 = Cpu; Start-Sleep -Seconds 5; "idle 5s with window: $([int]((Cpu) - $t0)) ms CPU"
# 7) dark button → dark title bar + screenshot
$a = At (760 - 24 - 96 - 12 - 48) 38; [T]::Tap($a[0], $a[1]) | Out-Null; Start-Sleep -Milliseconds 400
[W]::DwmGetWindowAttribute($win, 20, [ref]$dark, 4) | Out-Null; "after 深色: darkattr=$dark"
Shot $win 'appwin-dark'
# 8) close with its 关闭 button
$a = At (760 - 24 - 48) 38; [T]::Tap($a[0], $a[1]) | Out-Null; Start-Sleep -Milliseconds 800
"closed by button: window gone=$(-not [W]::IsWindow($win)) $(Mem)"
# 9) open/close from the strip 3 times (leak check), the last close via WM_CLOSE (title-bar ×)
for ($n = 1; $n -le 3; $n++) {
  StripTap 0; Start-Sleep -Milliseconds 700
  $win = [T]::FindOf('DianmoAppWindow', $p.Id); $open = Mem
  if ($n -lt 3) { StripTap 1 } else { [T]::PostMessage($win, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
  Start-Sleep -Milliseconds 700
  "cycle ${n}: open $open / closed $(Mem) gone=$(-not [W]::IsWindow($win))"
}
$t0 = Cpu; Start-Sleep -Seconds 3; "idle 3s after close: $([int]((Cpu) - $t0)) ms CPU"
# 10) hide the keyboard too: after 5 s the host drops its device, so no GPU objects are left
StripTap 2; Start-Sleep -Seconds 6; "keyboard hidden 6s: $(Mem)"
[T]::PostMessage($kb, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
if (-not $p.WaitForExit(3000)) { "demo did not exit"; $p.Kill() } else { "demo exited $($p.ExitCode)" }
"--- log"
Get-Content $log -Encoding UTF8 | Where-Object { $_ -notmatch ' pointer \d+ Move ' }
