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
#   CLIPSET:<t> put text on the clipboard (as if another app copied it)
#   PAD:<dx>            hold the space bar (trackpad), drag <dx> DIPs (negative = left), release
#   PADSEL:<dx1>:<dx2>  hold the space bar, drag dx1, tap with a second finger (starts selecting),
#                       drag dx2, release (screenshots while dragging)
#   TRAYMENU[:<downs>:<rights>]  open the test instance's tray menu, optionally move with the
#               arrow keys (rights open a submenu), screenshot, Escape
#   TRAYPICK<n> pick the n-th tray menu item (keyboard navigation); TRAYPICK:<keys> with
#               d = down, r = right (submenu), e = enter, e.g. TRAYPICK:dddddrdde = 语音引擎 ▸ 3rd
#   RUNKEY      print the HKCU Run entry
#   @x,y        tap at screen pixel x,y (for panels whose keys aren't in the keymap)
#   SET:k=v     (before the test starts) write k=v into the test instance's settings.ini, e.g.
#               SET:pc_keyboard=true to start on the 电脑键盘
#   ENV:k=v     (before the test starts) environment variable for the test instance
#               (e.g. ENV:DIANMO_NO=clipboard,ball for memory measurements)
#   BALL        print the floating ball's rect and visibility
#   BALLTAP     tap the ball (its on-screen part: it tucks into the edge after 3 s)
#   BALLHOLD    long-press the ball (0.9 s)
#   BALLDRAG:<dx>:<dy>  drag the ball by dx, dy screen pixels and let go
#   MIC         print which voice engines hold the microphone (consent store), e.g. wetype=True
#   INI         print the test instance's settings.ini (voice / ball lines)
#   ESC         press Escape (close a flyout the test opened)
#   CPU         print the test instance's CPU time so far and private memory
#   VMAP        list the test instance's committed private allocations (biggest first)
#   MEDIUM      (anywhere) start Notepad un-elevated, as a normal app would be (voice tests)
#   FGADMIN / FGMED  bring an (extra, elevated) administrator Notepad / the test Notepad to the
#               front (voice: engines can't hear hotkeys while an elevated window is in front)
#   BUBBLE      print whether the voice-mode hint bubble next to the ball is visible (and its rect)
#   SECOND      start dianmo.exe again (the running test instance should show its keyboard)
#   SPEAK:<t>   say <t> with the zh-CN TTS voice (the microphone hears it; voice tests)
#   TEXTKW:<kw> print the test Notepad's character count and whether it contains <kw> (not the text)
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
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
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
  public static void MoveY(int dy) { cy += dy; Inj(M(0, cx, cy, UPDATE|INRANGE|INCONTACT)); }
}
'@
function Shot($name) { $b = [System.Windows.Forms.SystemInformation]::VirtualScreen; $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size); $bmp.Save("C:\Users\wecode\claude\dm-$name.png", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose() }
function TabTip { $k = Get-ItemProperty 'HKCU:\Software\Microsoft\TabletTip\1.7' -ErrorAction SilentlyContinue
  "EnableDesktopModeAutoInvoke=$($k.EnableDesktopModeAutoInvoke) TouchKeyboardTapInvoke=$($k.TouchKeyboardTapInvoke)" }
Add-Type -TypeDefinition @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices;
public class VM {
  [StructLayout(LayoutKind.Sequential)] struct MBI { public IntPtr Base; public IntPtr AllocBase; public uint AllocProtect; public ushort PartitionId; public IntPtr Size; public uint State; public uint Protect; public uint Type; }
  [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint a, bool i, uint pid);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] static extern IntPtr VirtualQueryEx(IntPtr p, IntPtr a, out MBI m, IntPtr n);
  public class Alloc { public long Base; public long Size; public string Kind; }
  // Committed MEM_PRIVATE regions summed per allocation (Kind: protection of the first region).
  public static List<Alloc> Private(uint pid) {
    var res = new Dictionary<long, Alloc>(); IntPtr h = OpenProcess(0x0410, false, pid); long a = 0; MBI m;
    while (VirtualQueryEx(h, (IntPtr)a, out m, (IntPtr)Marshal.SizeOf(typeof(MBI))) != IntPtr.Zero) {
      long size = (long)m.Size;
      if (m.State == 0x1000 && m.Type == 0x20000) { long b = (long)m.AllocBase; Alloc x;
        if (!res.TryGetValue(b, out x)) { x = new Alloc { Base = b, Size = 0, Kind = "prot=0x" + m.Protect.ToString("X") }; res[b] = x; } x.Size += size; }
      a = (long)m.Base + size; if (size <= 0) break; }
    CloseHandle(h); return new List<Alloc>(res.Values); }
}
'@
function ClipText { try { [System.Windows.Forms.Clipboard]::GetText() } catch { "(clipboard busy)" } }
$mine = @(Get-Process dianmo -ErrorAction SilentlyContinue | ? { $_.Path -and $_.Path.StartsWith($RunDir, [StringComparison]::OrdinalIgnoreCase) })
if ($mine.Count) { "a test dianmo from $RunDir is already running; aborting"; return }
"other dianmo processes (left alone): $(@(Get-Process dianmo -ErrorAction SilentlyContinue).Count)"
# The test copies and pastes: keep the user's clipboard text.
$clipHadText = [System.Windows.Forms.Clipboard]::ContainsText(); $clipBackup = if ($clipHadText) { [System.Windows.Forms.Clipboard]::GetText() } else { $null }
"clipboard backed up (text: $clipHadText, $(if ($clipBackup) { $clipBackup.Length } else { 0 }) chars)"
$sets = @($Keys | ? { $_ -like 'SET:*' } | % { $_.Substring(4) }); $Keys = @($Keys | ? { $_ -notlike 'SET:*' })
$envs = @($Keys | ? { $_ -like 'ENV:*' } | % { $_.Substring(4) }); $Keys = @($Keys | ? { $_ -notlike 'ENV:*' })
foreach ($e in $envs) { $kv = $e -split '=', 2; Set-Item "env:$($kv[0])" $kv[1]; "test env: $e" }
function MicState { $root = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged'
  $o = @(Get-ChildItem $root -ErrorAction SilentlyContinue | ? { $_.PSChildName -match 'wetype_update\.exe$|doubaovoice|#doubaoime#' } | % {
    $p = Get-ItemProperty $_.PSPath; "$((($_.PSChildName -split '#')[-1]).ToLower())=$($p.LastUsedTimeStop -eq 0)" })
  if ($o.Count) { $o -join ' ' } else { '(no engine has used the microphone)' } }
if ($sets.Count) {
  $ini = Join-Path $env:APPDATA "Dianmo-$instance\settings.ini"; New-Item -ItemType Directory -Force (Split-Path $ini) | Out-Null
  Set-Content $ini -Value $sets -Encoding UTF8; "test settings: $($sets -join ', ')"
}
$ini = Join-Path $env:APPDATA "Dianmo-$instance\settings.ini"
$adm = $null
$logLines0 = @(Get-Content $applog -ErrorAction SilentlyContinue).Count
"tabtip before: $(TabTip)"
$work0 = [T]::Work(); "work area before: $(Fmt $work0)"
if ($Keys -contains 'MEDIUM') {
  # Un-elevated Notepad (explorer starts it with the shell's token; gui.sh runs elevated): voice
  # engines run un-elevated and don't see hotkeys while an elevated window is in front (UIPI).
  $Keys = @($Keys | ? { $_ -ne 'MEDIUM' })
  $before = @(Get-Process notepad -ErrorAction SilentlyContinue | % Id)
  Start-Process explorer.exe 'C:\Windows\notepad.exe'
  $np = $null; $deadline = (Get-Date).AddSeconds(10)
  while (-not $np -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 150; $np = Get-Process notepad -ErrorAction SilentlyContinue | ? { $before -notcontains $_.Id -and $_.MainWindowHandle -ne 0 } | select -First 1 }
  "notepad started un-elevated (pid $($np.Id))"
} else {
  $np = Start-Process notepad -PassThru
}
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
    if ($key -like 'TRAYPICK*' -or $key -like 'TRAYMENU*') {
      # Opens the test instance's tray menu by posting the icon's callback (WM_APP+4 with
      # WM_CONTEXTMENU) to its own tray window (found by process id; the icon may sit in the
      # overflow area). TRAYMENU[:<downs>:<rights>] screenshots it (after navigating) and closes it
      # with Escape; TRAYPICK<n> picks the n-th item (separators are skipped).
      $th = [G]::FindOf([uint32]$dm.Id, 'DianmoTray')
      if ($th -eq [IntPtr]::Zero) { "no tray window"; continue }
      [T]::SetCursorPos(2200, 1000) | Out-Null
      [T]::PostMessage($th, 0x8004, [IntPtr]::Zero, [IntPtr]0x7B) | Out-Null; Start-Sleep -Milliseconds 700
      if ($key -like 'TRAYPICK*') {
        $seq = $key.Substring(8)
        # TRAYPICK<n>, or TRAYPICK:<keys> with d = down, r = right (open submenu), e = enter.
        $send = if ($seq -like ':*') { ($seq.Substring(1).ToCharArray() | % { @{ d = '{DOWN}'; r = '{RIGHT}'; e = '{ENTER}' }[[string]$_] }) -join '' } else { ('{DOWN}' * [int]$seq) + '{ENTER}' }
        [System.Windows.Forms.SendKeys]::SendWait($send); Start-Sleep -Milliseconds 500
      } else {
        $f = $key -split ':'
        if ($f.Count -ge 3) { [System.Windows.Forms.SendKeys]::SendWait(('{DOWN}' * [int]$f[1]) + ('{RIGHT}' * [int]$f[2])); Start-Sleep -Milliseconds 500 }
        $i++; Shot "step$i"
        [System.Windows.Forms.SendKeys]::SendWait('{ESC}{ESC}{ESC}'); Start-Sleep -Milliseconds 300
      }
      [T]::Tap(1200, 16) | Out-Null; Start-Sleep -Milliseconds 300
      continue
    }
    if ($key -eq 'RUNKEY') { "autostart Run value: [$((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -ErrorAction SilentlyContinue).Dianmo)]"; continue }
    if ($key -like 'BALL*') {
      $bh = [G]::FindOf([uint32]$dm.Id, 'DianmoBall'); $br = New-Object T+RECT; [T]::GetWindowRect($bh, [ref]$br) | Out-Null
      $scr = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
      $bx = [int](([math]::Max($br.left, 0) + [math]::Min($br.right, $scr.Width)) / 2); $by = [int](($br.top + $br.bottom) / 2)
      if ($key -eq 'BALL') { "ball: $(if ($bh -eq [IntPtr]::Zero) { 'none' } else { "$(Fmt $br) visible=$([T]::IsWindowVisible($bh))" }) screen=$($scr.Width)x$($scr.Height)"; continue }
      if ($bh -eq [IntPtr]::Zero -or -not [T]::IsWindowVisible($bh)) { "ball not visible"; continue }
      if ($key -eq 'BALLTAP') { [T]::Tap($bx, $by) | Out-Null; Start-Sleep -Milliseconds 250; continue }
      if ($key -eq 'BALLHOLD') { [G]::Down($bx, $by) | Out-Null; [G]::Hold(900); [G]::Up() | Out-Null; Start-Sleep -Milliseconds 400; continue }
      if ($key -like 'BALLDRAG:*') {
        $f = $key -split ':'; [G]::Down($bx, $by) | Out-Null; [G]::Hold(100)
        [G]::Move([int]$f[1], 12, 15); $n = [math]::Abs([int]$f[2]) / 12
        for ($j = 0; $j -lt $n; $j++) { [G]::MoveY([int]([math]::Sign([int]$f[2]) * 12)); Start-Sleep -Milliseconds 15 }
        [G]::Up() | Out-Null; Start-Sleep -Milliseconds 600; continue }
    }
    if ($key -eq 'VMAP') {
      # Committed private memory of the test instance, biggest allocations first (memory budget).
      $regs = [VM]::Private([uint32]$dm.Id)
      $tot = ($regs | Measure-Object -Property Size -Sum).Sum
      "vmap: private committed $([math]::Round($tot/1MB,1))MB in $($regs.Count) allocations; largest:"
      $regs | Sort-Object Size -Descending | Select -First 14 | % { "  0x{0:X12} {1,8:N0} KB  {2}" -f $_.Base, ($_.Size/1KB), $_.Kind }
      $small = @($regs | ? { $_.Size -lt 1MB }); "  ($($small.Count) allocations < 1 MB: $([math]::Round((($small | Measure-Object -Property Size -Sum).Sum)/1MB,1)) MB)"
      continue
    }
    if ($key -eq 'CPU') { $pp = Get-Process -Id $dm.Id; "cpu: total=$([int]$pp.TotalProcessorTime.TotalMilliseconds)ms private=$([math]::Round($pp.PrivateMemorySize64/1MB,1))MB"; continue }
    if ($key -eq 'ESC') { [System.Windows.Forms.SendKeys]::SendWait('{ESC}'); Start-Sleep -Milliseconds 300; continue }
    if ($key -eq 'MIC') { "mic: $(MicState)"; continue }
    if ($key -eq 'FGADMIN') {
      if (-not $adm) { $adm = Start-Process notepad -PassThru; $dl = (Get-Date).AddSeconds(8); while ($adm.MainWindowHandle -eq 0 -and (Get-Date) -lt $dl) { Start-Sleep -Milliseconds 100; $adm.Refresh() } }
      [G]::SetForegroundWindow($adm.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 400
      "foreground: admin notepad=$([T]::GetForegroundWindow() -eq $adm.MainWindowHandle)"; continue }
    if ($key -eq 'FGMED') { [G]::SetForegroundWindow($nh) | Out-Null; Start-Sleep -Milliseconds 400; "foreground: test notepad=$([T]::GetForegroundWindow() -eq $nh)"; continue }
    if ($key -eq 'BUBBLE') {
      $bb = [G]::FindOf([uint32]$dm.Id, 'DianmoBubble'); $r = New-Object T+RECT
      if ($bb -ne [IntPtr]::Zero) { [T]::GetWindowRect($bb, [ref]$r) | Out-Null }
      "bubble: $(if ($bb -eq [IntPtr]::Zero) { 'none' } else { "visible=$([T]::IsWindowVisible($bb)) $(Fmt $r)" })"; continue }
    if ($key -eq 'SECOND') {
      $dm2 = Start-Process $exe -ArgumentList $dmArgs -PassThru; $null = $dm2.WaitForExit(5000); Start-Sleep -Milliseconds 700
      "second launch exited=$($dm2.HasExited); keyboard visible=$([T]::IsWindowVisible($kh))"; continue }
    if ($key -like 'SPEAK:*') {
      Add-Type -AssemblyName System.Speech; $tts = New-Object System.Speech.Synthesis.SpeechSynthesizer
      try { $tts.SelectVoice('Microsoft Huihui Desktop') } catch {}
      $tts.Volume = 100; $tts.Speak($key.Substring(6)); $tts.Dispose(); continue }
    if ($key -like 'TEXTKW:*') { $t = [T]::Text($edit); "notepad: chars=$(if ($t) { $t.Length } else { 0 }) keyword=$(if ($t) { $t.Contains($key.Substring(7)) } else { $false })"; continue }
    if ($key -eq 'INI') { "settings: $((Get-Content $ini -Encoding UTF8 -ErrorAction SilentlyContinue | ? { $_ -match '^(voice|ball)' }) -join '; ')"; continue }
    if ($key -eq 'VIS') { "keyboard visible: $([T]::IsWindowVisible($kh))  fg=$([T]::GetForegroundWindow() -eq $nh)  work=$(Fmt ([T]::Work()))"; continue }
    if ($key -eq 'TEXT') { "text: [$(([T]::Text($edit)) -replace "`r`n",'\n')]"; continue }
    if ($key -like 'CLIPSET:*') { try { Set-Clipboard -Value $key.Substring(8) -ErrorAction Stop } catch { "clipset: $_" }; Start-Sleep -Milliseconds 100; continue }
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
  if ($adm) { Stop-Process -Id $adm.Id -Force -ErrorAction SilentlyContinue }
  try { if ($clipHadText) { [System.Windows.Forms.Clipboard]::SetText($clipBackup) } else { [System.Windows.Forms.Clipboard]::Clear() }; "clipboard restored: $((ClipText) -eq $(if ($clipHadText) { $clipBackup } else { '' }))" } catch { "restoring the clipboard failed: $_" }
  "--- dianmo.log (this run) ---"
  Get-Content $applog -Encoding UTF8 -ErrorAction SilentlyContinue | Select -Skip $logLines0
}
