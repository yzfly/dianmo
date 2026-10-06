# Dot-sourced after crates/dianmo-win/tests/surface/touch.ps1 (class T). Needs $RunDir, $Keys.
# Runs a separate test instance (`--instance Test --no-elevate`: own mutex, data dir
# %APPDATA%\Dianmo-Test, task name), so the user's own 点墨 can keep running; our keyboard window is
# found by process id. The user's clipboard text is saved first and restored at the end.
# $Keys: key names from the keymap (see DianmoApp's DIANMO_KEYMAP hook; re-read before every tap,
# the app rewrites it whenever the layout changes), or
#   SHOT        screenshot to C:\Users\wecode\claude\dm-step<N>.png
#   WAIT<ms>    sleep
#   VIS         print whether the keyboard is visible
#   TEXT        print Notepad's text so far
#   CLIP        print the clipboard text
#   PAD:<dx>            hold the space bar (trackpad), drag <dx> DIPs (negative = left), release
#   PADSEL:<dx1>:<dx2>  hold the space bar, drag dx1, tap with a second finger (starts selecting),
#                       drag dx2, release (screenshots while dragging)
#   TRAYMENU    right-click the tray icon, screenshot the menu, Escape
#   TRAYPICK<n> pick the n-th tray menu item (keyboard navigation)
#   RUNKEY      print the HKCU Run entry
#   @x,y        tap at screen pixel x,y (for panels whose keys aren't in the keymap)
#   SET:k=v     (before the test starts) write k=v into the test instance's settings.ini, e.g.
#               SET:pc_keyboard=true to start on the 电脑键盘
# Note: the keymap is rewritten when the app handles an action or event; panel switches inside
# the keyboard (layout menu, trackpad end) don't produce one, so tap a key that does afterwards.
$exe = Join-Path $RunDir 'dianmo.exe'
$instance = 'Test'
$dmArgs = @('--instance', $instance, '--no-elevate')
$km = 'C:\Users\wecode\claude\dianmo-keymap.txt'
$applog = Join-Path $env:APPDATA "Dianmo-$instance\dianmo.log"
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Text; using System.Threading; using System.Runtime.InteropServices;
public static class G {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  public static IntPtr FindOf(uint pid, string cls) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p != pid) return true;
      var sb = new StringBuilder(256); GetClassName(h, sb, 256); if (sb.ToString() == cls) { found = h; return false; } return true; }, IntPtr.Zero);
    return found; }
  [StructLayout(LayoutKind.Sequential)] public struct PT { public int x, y; }
  [StructLayout(LayoutKind.Sequential)] public struct RC { public int left, top, right, bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct PI {
    public int pointerType; public uint pointerId; public uint frameId; public int pointerFlags;
    public IntPtr sourceDevice; public IntPtr hwndTarget; public PT ptPixelLocation; public PT ptHimetricLocation;
    public PT ptPixelLocationRaw; public PT ptHimetricLocationRaw; public uint dwTime; public uint historyCount;
    public int InputData; public uint dwKeyStates; public ulong PerformanceCount; public int ButtonChangeType; }
  [StructLayout(LayoutKind.Sequential)] public struct PTI { public PI pointerInfo; public int touchFlags; public int touchMask; public RC rcContact; public RC rcContactRaw; public uint orientation; public uint pressure; }
  [DllImport("user32.dll", SetLastError=true)] static extern bool InjectTouchInput(uint count, [In] PTI[] contacts);
  const int INRANGE=0x2, INCONTACT=0x4, DOWN=0x10000, UPDATE=0x20000, UP=0x40000;
  static PTI M(uint id, int x, int y, int flags) {
    var c = new PTI(); c.pointerInfo.pointerType = 2; c.pointerInfo.pointerId = id;
    c.pointerInfo.ptPixelLocation = new PT{x=x,y=y}; c.pointerInfo.pointerFlags = flags;
    c.touchMask = 0x7; c.rcContact = new RC{left=x-6, top=y-6, right=x+6, bottom=y+6}; c.orientation = 90; c.pressure = 32000; return c; }
  static int cx, cy;
  static bool Inj(params PTI[] c) { return InjectTouchInput((uint)c.Length, c); }
  public static bool Down(int x, int y) { cx = x; cy = y; return Inj(M(0, x, y, DOWN|INRANGE|INCONTACT)); }
  public static void Hold(int ms) { for (int t = 0; t < ms; t += 50) { Thread.Sleep(50); Inj(M(0, cx, cy, UPDATE|INRANGE|INCONTACT)); } }
  // Drags the held contact by dx pixels in steps of `step` pixels every `dt` ms.
  public static void Move(int dx, int step, int dt) {
    int n = Math.Abs(dx) / step, s = Math.Sign(dx) * step;
    for (int i = 0; i < n; i++) { cx += s; Thread.Sleep(dt); Inj(M(0, cx, cy, UPDATE|INRANGE|INCONTACT)); } }
  // A second finger taps at (x, y) while the first stays down.
  public static void Aux(int x, int y) {
    Inj(M(0, cx, cy, UPDATE|INRANGE|INCONTACT), M(1, x, y, DOWN|INRANGE|INCONTACT)); Thread.Sleep(60);
    Inj(M(0, cx, cy, UPDATE|INRANGE|INCONTACT), M(1, x, y, UPDATE|INRANGE|INCONTACT)); Thread.Sleep(40);
    Inj(M(0, cx, cy, UPDATE|INRANGE|INCONTACT), M(1, x, y, UP)); Thread.Sleep(50); }
  public static bool Up() { return Inj(M(0, cx, cy, UP)); }
}
'@
function Shot($name) { $b = [System.Windows.Forms.SystemInformation]::VirtualScreen; $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size); $bmp.Save("C:\Users\wecode\claude\dm-$name.png", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose() }
function TabTip { $k = Get-ItemProperty 'HKCU:\Software\Microsoft\TabletTip\1.7' -ErrorAction SilentlyContinue
  "EnableDesktopModeAutoInvoke=$($k.EnableDesktopModeAutoInvoke) TouchKeyboardTapInvoke=$($k.TouchKeyboardTapInvoke)" }
function ClipText { try { [System.Windows.Forms.Clipboard]::GetText() } catch { "(clipboard busy)" } }
$mine = @(Get-Process dianmo -ErrorAction SilentlyContinue | ? { $_.Path -and $_.Path.StartsWith($RunDir, [StringComparison]::OrdinalIgnoreCase) })
if ($mine.Count) { "a test dianmo from $RunDir is already running; aborting"; return }
"other dianmo processes (left alone): $(@(Get-Process dianmo -ErrorAction SilentlyContinue).Count)"
# The test copies and pastes: keep the user's clipboard text.
$clipHadText = [System.Windows.Forms.Clipboard]::ContainsText(); $clipBackup = if ($clipHadText) { [System.Windows.Forms.Clipboard]::GetText() } else { $null }
"clipboard backed up (text: $clipHadText, $(if ($clipBackup) { $clipBackup.Length } else { 0 }) chars)"
$sets = @($Keys | ? { $_ -like 'SET:*' } | % { $_.Substring(4) }); $Keys = @($Keys | ? { $_ -notlike 'SET:*' })
if ($sets.Count) {
  $ini = Join-Path $env:APPDATA "Dianmo-$instance\settings.ini"; New-Item -ItemType Directory -Force (Split-Path $ini) | Out-Null
  Set-Content $ini -Value $sets -Encoding UTF8; "test settings: $($sets -join ', ')"
}
$logLines0 = @(Get-Content $applog -ErrorAction SilentlyContinue).Count
"tabtip before: $(TabTip)"
$work0 = [T]::Work(); "work area before: $(Fmt $work0)"
$np = Start-Process notepad -PassThru
$deadline = (Get-Date).AddSeconds(8)
while ($np.MainWindowHandle -eq 0 -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100; $np.Refresh() }
$nh = $np.MainWindowHandle; $edit = [T]::FindWindowEx($nh, [IntPtr]::Zero, 'Edit', [NullString]::Value)
[T]::ShowWindow($nh, 3) | Out-Null; Start-Sleep -Milliseconds 400
[T]::Tap(1200, 16) | Out-Null; Start-Sleep -Milliseconds 300
Remove-Item $km -ErrorAction SilentlyContinue
$env:DIANMO_KEYMAP = $km
$env:DIANMO_FOCUS_LOG = "C:\Users\wecode\claude\dianmo-focus.log"; Remove-Item $env:DIANMO_FOCUS_LOG -ErrorAction SilentlyContinue
$t0 = Get-Date
$dm = Start-Process $exe -ArgumentList $dmArgs -PassThru
$deadline = (Get-Date).AddSeconds(15); $kh = [IntPtr]::Zero
while ($kh -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 20; $kh = [G]::FindOf([uint32]$dm.Id, 'DianmoKeyboard') }
while (-not [T]::IsWindowVisible($kh) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 20 }
"keyboard visible after $([int]((Get-Date) - $t0).TotalMilliseconds)ms (incl. process start)"
Start-Sleep -Milliseconds 800
try {
  "tabtip while running: $(TabTip)"
  "foreground still notepad: $([T]::GetForegroundWindow() -eq $nh)"
  $k = New-Object T+RECT; [T]::GetWindowRect($kh, [ref]$k) | Out-Null; "keyboard rect: $(Fmt $k)"
  $s = [T]::GetDpiForWindow($kh) / 96.0
  function LoadMap { $m = @{}; Get-Content $km -Encoding UTF8 -ErrorAction SilentlyContinue | % { $f = $_ -split ' '; if ($f.Count -ge 3) { $m[$f[0]] = @([double]$f[1], [double]$f[2]) } }; $m }
  $map = LoadMap
  "keymap entries: $($map.Count)"
  # Screen pixel of a key (keymap and window rect read fresh: layouts and AppBar stacking change).
  function KeyPx($name) {
    $m = LoadMap
    if (-not $m.ContainsKey($name)) { return $null }
    $r = New-Object T+RECT; [T]::GetWindowRect($kh, [ref]$r) | Out-Null
    $p = $m[$name]; @([int]($r.left + $p[0] * $s), [int]($r.top + $p[1] * $s))
  }
  function TapKey($name) {
    $p = KeyPx $name
    if (-not $p) { "no key $name"; return }
    $ok = [T]::Tap($p[0], $p[1]); Start-Sleep -Milliseconds 180
  }
  $i = 0
  foreach ($key in $Keys) {
    if ($key -eq 'SHOT') { $i++; Shot "step$i"; continue }
    if ($key -like 'PAD*') {
      # Trackpad: hold the space bar, drag (4 DIP every 30 ms = slow, one character per ~14 DIP).
      $f = $key -split ':'; $sp = KeyPx 'space'; $q = KeyPx 'q'
      if (-not $sp) { "no space key"; continue }
      [G]::Down($sp[0], $sp[1]) | Out-Null; [G]::Hold(650)
      [G]::Move([int]([int]$f[1] * $s), [int](4 * $s), 30)
      $i++; Shot "step$i"
      if ($f[0] -eq 'PADSEL') {
        [G]::Aux($q[0], $q[1]); [G]::Move([int]([int]$f[2] * $s), [int](4 * $s), 30)
        $i++; Shot "step$i"
      }
      [G]::Up() | Out-Null; Start-Sleep -Milliseconds 300
      continue
    }
    if ($key -like 'TRAYPICK*') {
      # Open the tray menu and pick its N-th item with the arrow keys (separators are skipped).
      $n = [int]$key.Substring(8)
      $th = [T]::FindWindow('DianmoTray', [NullString]::Value); $tr = [T]::TrayRect($th)
      if ($tr.right -le 0) { "tray icon not visible (overflow?)"; continue }
      [T]::SetCursorPos([int](($tr.left + $tr.right) / 2), [int](($tr.top + $tr.bottom) / 2)) | Out-Null; Start-Sleep -Milliseconds 50
      [T]::mouse_event(8, 0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 60; [T]::mouse_event(16, 0, 0, 0, [IntPtr]::Zero)
      Start-Sleep -Milliseconds 600
      [System.Windows.Forms.SendKeys]::SendWait(('{DOWN}' * $n) + '{ENTER}'); Start-Sleep -Milliseconds 500
      [T]::Tap(1200, 16) | Out-Null; Start-Sleep -Milliseconds 300
      continue
    }
    if ($key -eq 'RUNKEY') { "autostart Run value: [$((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -ErrorAction SilentlyContinue).Dianmo)]"; continue }
    if ($key -eq 'TRAYMENU') {
      # Right-click the tray icon, screenshot the menu, dismiss it with Escape.
      $th = [T]::FindWindow('DianmoTray', [NullString]::Value); $tr = [T]::TrayRect($th)
      if ($tr.right -le 0) { "tray icon not visible (overflow?)"; continue }
      [T]::SetCursorPos([int](($tr.left + $tr.right) / 2), [int](($tr.top + $tr.bottom) / 2)) | Out-Null; Start-Sleep -Milliseconds 50
      [T]::mouse_event(8, 0, 0, 0, [IntPtr]::Zero); Start-Sleep -Milliseconds 60; [T]::mouse_event(16, 0, 0, 0, [IntPtr]::Zero)
      Start-Sleep -Milliseconds 700; $i++; Shot "step$i"
      [System.Windows.Forms.SendKeys]::SendWait('{ESC}'); Start-Sleep -Milliseconds 300
      [T]::Tap(1200, 16) | Out-Null; Start-Sleep -Milliseconds 300
      continue
    }
    if ($key -eq 'VIS') { "keyboard visible: $([T]::IsWindowVisible($kh))  fg=$([T]::GetForegroundWindow() -eq $nh)  work=$(Fmt ([T]::Work()))"; continue }
    if ($key -eq 'TEXT') { "text: [$(([T]::Text($edit)) -replace "`r`n",'\n')]"; continue }
    if ($key -eq 'CLIP') { "clipboard: [$(ClipText)]"; continue }
    if ($key -like 'WAIT*') { Start-Sleep -Milliseconds ([int]$key.Substring(4)); continue }
    if ($key -like '@*') { $xy = $key.Substring(1) -split ','; [T]::Tap([int]$xy[0], [int]$xy[1]) | Out-Null; Start-Sleep -Milliseconds 250; continue }
    TapKey $key
  }
  "text after keys: [$(([T]::Text($edit)) -replace "`r`n",'\n')]  fg=$([T]::GetForegroundWindow() -eq $nh)"
  Shot 'final'
  $p = Get-Process -Id $dm.Id; $c0 = $p.TotalProcessorTime.TotalMilliseconds
  Start-Sleep -Seconds 10; $p.Refresh(); $c1 = $p.TotalProcessorTime.TotalMilliseconds
  "memory: private=$([math]::Round($p.PrivateMemorySize64/1MB,1))MB workingset=$([math]::Round($p.WorkingSet64/1MB,1))MB; cpu total=$([int]$c1)ms, idle 10s delta=$([int]($c1-$c0))ms"
  # single instance: hide, start again -> the running one shows the keyboard
  TapKey 'hide'; Start-Sleep -Milliseconds 400
  "after hide key: visible=$([T]::IsWindowVisible($kh))"
  $dm2 = Start-Process $exe -ArgumentList $dmArgs -PassThru; $null = $dm2.WaitForExit(5000)
  Start-Sleep -Milliseconds 500
  "second instance exited=$($dm2.HasExited); keyboard visible again=$([T]::IsWindowVisible($kh)); test dianmo processes=$(@(Get-Process dianmo | ? { $_.Path -and $_.Path.StartsWith($RunDir, [StringComparison]::OrdinalIgnoreCase) }).Count)"
} finally {
  if ($kh -ne [IntPtr]::Zero) { [T]::PostMessage($kh, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
  if (!$dm.WaitForExit(5000)) { "dianmo did not exit on WM_CLOSE, killing"; Stop-Process -Id $dm.Id -Force } else { "dianmo exited, code $($dm.ExitCode)" }
  Start-Sleep -Milliseconds 300
  "tabtip after: $(TabTip)"
  "work area after exit: $(Fmt ([T]::Work())) (before: $(Fmt $work0))"
  Stop-Process -Id $np.Id -Force
  try { if ($clipHadText) { [System.Windows.Forms.Clipboard]::SetText($clipBackup) } else { [System.Windows.Forms.Clipboard]::Clear() }; "clipboard restored: $((ClipText) -eq $(if ($clipHadText) { $clipBackup } else { '' }))" } catch { "restoring the clipboard failed: $_" }
  "--- dianmo.log (this run) ---"
  Get-Content $applog -Encoding UTF8 -ErrorAction SilentlyContinue | Select -Skip $logLines0
}
