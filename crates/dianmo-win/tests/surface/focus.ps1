# Focus watcher / auto show-hide / tray menu / full-screen test (dot-sourced after touch.ps1 by
# run-focus.sh). Real touch is injected into Notepad, an Explorer window, a private Edge
# instance and the taskbar search; the demo runs with --auto --hidden --tray-menu and logs every
# FocusEvent. Everything opened here is closed again; the system keyboard's auto-invoke is turned
# off for the duration and restored afterwards.
$dir = 'C:\Users\wecode\claude'
$exe = 'C:\dev\dianmo-win\target\release\examples\demo.exe'
$log = "$dir\focus-demo.log"; $flog = "$dir\focus-uia.log"
Remove-Item $log, $flog -ErrorAction SilentlyContinue
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Threading; using System.Runtime.InteropServices;
public static class F {
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string n);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, IntPtr e);
  public static void Key(byte vk) { keybd_event(vk, 0, 0, IntPtr.Zero); Thread.Sleep(30); keybd_event(vk, 0, 2, IntPtr.Zero); }
  public static void RightClick(int x, int y) { SetCursorPos(x, y); Thread.Sleep(30); mouse_event(8, 0, 0, 0, IntPtr.Zero); Thread.Sleep(60); mouse_event(16, 0, 0, 0, IntPtr.Zero); }
  public static bool Topmost(IntPtr h) { return (GetWindowLong(h, -20) & 8) != 0; }
}
'@
$TS = [System.Windows.Automation.TreeScope]
$AEl = [System.Windows.Automation.AutomationElement]
function Cond($prop, $val) { New-Object System.Windows.Automation.PropertyCondition($prop, $val) }
function FindDesc($root, $prop, $val) { $root.FindFirst($TS::Descendants, (Cond $prop $val)) }
function WaitFind($root, $prop, $val, $ms = 8000) {
  $deadline = (Get-Date).AddMilliseconds($ms)
  do { $e = FindDesc $root $prop $val; if ($e) { return $e }; Start-Sleep -Milliseconds 200 } while ((Get-Date) -lt $deadline)
  $null
}
function Center($el) { $r = $el.Current.BoundingRectangle; @([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) }
function Mark($s) { Add-Content $log "### $s" -Encoding UTF8 }
function Vis { [F]::IsWindowVisible($kh) }
function TapP($p, $what, $wait = 900) {
  Mark $what; $ok = [T]::Tap($p[0], $p[1]); Start-Sleep -Milliseconds $wait
  "tap $what at ($($p[0]),$($p[1])) ok=$ok -> keyboard visible=$(Vis)"
}
function TapEl($el, $what, $wait = 900) { if (!$el) { "!! $what not found"; return }; TapP (Center $el) $what $wait }
function Shot($name) { $b = [System.Windows.Forms.SystemInformation]::VirtualScreen; $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size); $bmp.Save("$dir\$name.png", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose() }
function Cost($label, $secs) {
  $p = Get-Process -Id $demo.Id; $c0 = $p.TotalProcessorTime.TotalMilliseconds
  Start-Sleep -Seconds $secs; $p.Refresh(); $c1 = $p.TotalProcessorTime.TotalMilliseconds
  "[$label] private=$([math]::Round($p.PrivateMemorySize64/1MB,1))MB ws=$([math]::Round($p.WorkingSet64/1MB,1))MB threads=$($p.Threads.Count) handles=$($p.HandleCount) idle ${secs}s cpu delta=$([int]($c1-$c0))ms"
}

if (!$sections) { $sections = 'all' }
$edge = 'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
# Centre of the demo's hide key (second row, last key; layout as in examples/demo.rs).
function HideKey {
  $k = New-Object T+RECT; [T]::GetWindowRect($kh, [ref]$k) | Out-Null; $s = [T]::GetDpiForWindow($kh) / 96.0
  $W = ($k.right - $k.left) / $s; $H = ($k.bottom - $k.top) / $s; $rh = ($H - 52) / 2; $u = ($W - 6) / 5.5
  @([int]($k.left + ($W - 3 - $u * 0.5) * $s), [int]($k.top + (52 + 1.5 * $rh) * $s))
}
function On($n) { $sections -eq 'all' -or ($sections -split ',') -contains $n }
function EdgeProcs { Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" | Where-Object { $_.CommandLine -like "*edge-focus-profile*" } }
# Close the private Edge politely (WM_CLOSE to its windows) so it doesn't offer to restore pages.
function CloseEdge {
  foreach ($e in EdgeProcs) { $p = Get-Process -Id $e.ProcessId -ErrorAction SilentlyContinue; try { if ($p -and $p.MainWindowHandle -ne 0) { $p.CloseMainWindow() | Out-Null } } catch {} }
  $deadline = (Get-Date).AddSeconds(4); while ((EdgeProcs) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
  EdgeProcs | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
$tt = 'HKCU:\Software\Microsoft\TabletTip\1.7'
$oldAuto = (Get-ItemProperty $tt -ErrorAction SilentlyContinue).EnableDesktopModeAutoInvoke
Set-ItemProperty $tt EnableDesktopModeAutoInvoke 0 -Type DWord
$np = $null; $demo = $null; $kh = [IntPtr]::Zero; $edgeDir = "$dir\edge-focus-profile"; $exWin = $null
try {
  $env:DIANMO_DEMO_LOG = $log; $env:DIANMO_FOCUS_LOG = $flog
  $demo = Start-Process $exe -ArgumentList '--auto', '--hidden', '--tray-menu' -PassThru
  $deadline = (Get-Date).AddSeconds(8)
  while ($kh -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 50; $kh = [T]::FindOf('DianmoKeyboard', $demo.Id) }
  Start-Sleep -Milliseconds 1500
  "demo started, keyboard visible=$(Vis)"
  Cost 'idle, watcher running' 8

  # --- Notepad -------------------------------------------------------------------------------
  if (On 'notepad') {
    $np = Start-Process notepad -PassThru
    $deadline = (Get-Date).AddSeconds(8)
    while ($np.MainWindowHandle -eq 0 -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100; $np.Refresh() }
    $nh = $np.MainWindowHandle; [T]::ShowWindow($nh, 3) | Out-Null; Start-Sleep -Milliseconds 800
    "notepad started (focus without touch), keyboard visible=$(Vis)"
    $edit = [T]::FindWindowEx($nh, [IntPtr]::Zero, 'Edit', [NullString]::Value)
    $r = New-Object T+RECT; [T]::GetWindowRect($edit, [ref]$r) | Out-Null
    TapP @([int](($r.left + $r.right) / 2), [int]($r.top + 200)) 'notepad edit (already focused: retap path)'
    TapP (HideKey) 'demo hide key'
    [T]::GetWindowRect($edit, [ref]$r) | Out-Null
    TapP @([int](($r.left + $r.right) / 2), [int]($r.top + 200)) 'notepad edit again'
    Stop-Process -Id $np.Id -Force; $np = $null; Start-Sleep -Milliseconds 500
  }

  # --- Explorer --------------------------------------------------------------------------------
  if (On 'explorer') {
    $before = @((New-Object -ComObject Shell.Application).Windows() | ForEach-Object { $_.HWND })
    Start-Process explorer.exe $dir
    $deadline = (Get-Date).AddSeconds(8)
    while (!$exWin -and (Get-Date) -lt $deadline) {
      Start-Sleep -Milliseconds 300
      $exWin = (New-Object -ComObject Shell.Application).Windows() | Where-Object { $before -notcontains $_.HWND } | Select-Object -First 1
    }
    if ($exWin) {
      $eh = [IntPtr]$exWin.HWND; Start-Sleep -Milliseconds 1000
      $ewin = $AEl::FromHandle($eh)
      $search = FindDesc $ewin $AEl::ClassNameProperty 'UniversalSearchBand'
      TapEl $search 'explorer search box'
      $addr = FindDesc $ewin $AEl::ClassNameProperty 'Address Band Root'
      if ($addr) { $ar = $addr.Current.BoundingRectangle; TapP @([int]($ar.X + $ar.Width * 0.8), [int]($ar.Y + $ar.Height / 2)) 'explorer address bar' }
      $list = FindDesc $ewin $AEl::ClassNameProperty 'UIItemsView'
      TapEl $list 'explorer file list (non-editable)'
      $exWin.Quit(); $exWin = $null; Start-Sleep -Milliseconds 500
    } else { "!! explorer window not found" }
  }

  # --- Edge (private instance, own profile) ----------------------------------------------------
  if (On 'edge') {
    Start-Process $edge -ArgumentList "--user-data-dir=$edgeDir", '--no-first-run', '--no-default-browser-check', '--disable-sync', '--hide-crash-restore-bubble', '--start-maximized', '--new-window', "file:///$($dir -replace '\\','/')/focus-test.html"
    $edgeWin = $null; $deadline = (Get-Date).AddSeconds(20)
    while (!$edgeWin -and (Get-Date) -lt $deadline) {
      Start-Sleep -Milliseconds 300
      $edgeWin = $AEl::RootElement.FindAll($TS::Children, (Cond $AEl::ClassNameProperty 'Chrome_WidgetWin_1')) | Where-Object { $_.Current.Name -like '*点墨焦点测试*' } | Select-Object -First 1
    }
    $page = if ($edgeWin) { WaitFind $edgeWin $AEl::AutomationIdProperty 'txt' 10000 }
    if ($page) {
      Start-Sleep -Milliseconds 1000
      foreach ($id in 'txt', 'srch', 'num', 'pw', 'url', 'mail', 'area', 'ce', 'ro', 'btn', 'txt') {
        $el = FindDesc $edgeWin $AEl::AutomationIdProperty $id
        TapEl $el "edge #$id" 1000
      }
      $wr = $edgeWin.Current.BoundingRectangle
      TapP @([int]($wr.X + $wr.Width * 0.6), [int]($wr.Y + $wr.Height * 0.55)) 'edge empty page area (non-editable)' 1000
      $el = FindDesc $edgeWin $AEl::AutomationIdProperty 'txt'
      TapEl $el 'edge #txt (keyboard shows)' 1000
      if (Vis) { TapP (HideKey) 'demo hide key' }
      $el = FindDesc $edgeWin $AEl::AutomationIdProperty 'txt'
      TapEl $el 'edge #txt again (already focused: retap)' 1000
      "cursor after edge tap: $([System.Windows.Forms.Cursor]::Position)"
      $omni = FindDesc $edgeWin $AEl::ClassNameProperty 'OmniboxViewViews'
      TapEl $omni 'edge address bar' 1000
      [F]::Key(0x1B); Start-Sleep -Milliseconds 300
      Shot 'focus-edge'
    } else { "!! edge test page not found" }
    CloseEdge
  }

  # --- VS Code (Electron, own profile) -----------------------------------------------------------
  if (On 'vscode') {
    $code = "$env:LOCALAPPDATA\Programs\Microsoft VS Code\Code.exe"
    if (Test-Path $code) {
      Set-Content "$dir\focus-note.txt" 'dianmo focus test' -Encoding UTF8
      $codeArgs = @("--user-data-dir=$dir\vscode-focus", "--extensions-dir=$dir\vscode-focus-ext", '--disable-extensions', '--disable-workspace-trust', '--skip-welcome', '--skip-release-notes', '--new-window', "$dir\focus-note.txt")
      $cp = Start-Process $code -ArgumentList $codeArgs -PassThru
      $codeWin = $null; $deadline = (Get-Date).AddSeconds(20)
      while (!$codeWin -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 400
        $codeWin = $AEl::RootElement.FindAll($TS::Children, (Cond $AEl::ClassNameProperty 'Chrome_WidgetWin_1')) | Where-Object { $_.Current.Name -like '*focus-note.txt*' } | Select-Object -First 1
      }
      if ($codeWin) {
        Start-Sleep -Milliseconds 2500
        $cr = $codeWin.Current.BoundingRectangle
        TapP @([int]($cr.X + $cr.Width * 0.5), [int]($cr.Y + $cr.Height * 0.35)) 'vscode editor' 1200
        TapP @([int]($cr.X + 40), [int]($cr.Y + $cr.Height * 0.12)) 'vscode activity bar (non-editable)' 1200
        Shot 'focus-vscode'
      } else { "!! vscode window not found" }
      Get-CimInstance Win32_Process -Filter "Name='Code.exe'" | Where-Object { $_.CommandLine -like "*vscode-focus*" } | ForEach-Object {
        $p = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue; try { if ($p -and $p.MainWindowHandle -ne 0) { $p.CloseMainWindow() | Out-Null } } catch {} }
      Start-Sleep -Milliseconds 2500
      Get-CimInstance Win32_Process -Filter "Name='Code.exe'" | Where-Object { $_.CommandLine -like "*vscode-focus*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    }
  }

  # --- Taskbar search ----------------------------------------------------------------------------
  if (On 'search') {
    $tray = $AEl::FromHandle([F]::FindWindow('Shell_TrayWnd', [NullString]::Value))
    "taskbar buttons: " + (($tray.FindAll($TS::Descendants, (Cond $AEl::ControlTypeProperty ([System.Windows.Automation.ControlType]::Button))) | Select-Object -First 8 | ForEach-Object { "$($_.Current.ClassName)/$($_.Current.AutomationId)/$($_.Current.Name)" }) -join ' | ')
    $sb = FindDesc $tray $AEl::ClassNameProperty 'TrayDummySearchControl'
    if (!$sb) { $sb = $tray.FindAll($TS::Descendants, (Cond $AEl::ClassNameProperty 'TrayButton')) | Where-Object { $_.Current.Name -match 'Search|搜索' } | Select-Object -First 1 }
    if ($sb) {
      TapEl $sb 'taskbar search' 2000
      Shot 'focus-search'
      [F]::Key(0x1B); Start-Sleep -Milliseconds 800; Mark 'escape'
    } else { "!! taskbar search not found" }
  }

  # --- Full-screen app (Edge --start-fullscreen, own profile) -----------------------------------
  if (On 'fullscreen') {
    $hh = [T]::FindOf('DianmoBall', $demo.Id)
    [T]::PostMessage($kh, 0x8001, [IntPtr]1, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds 500   # CMD_SHOW
    "before full-screen: visible=$(Vis) topmost=$([F]::Topmost($kh))"
    Mark 'fullscreen edge'
    Start-Process $edge -ArgumentList "--user-data-dir=$edgeDir", '--no-first-run', '--no-default-browser-check', '--disable-sync', '--hide-crash-restore-bubble', '--start-fullscreen', '--new-window', "file:///$($dir -replace '\\','/')/focus-test.html"
    Start-Sleep -Milliseconds 3500
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $fg = [T]::GetForegroundWindow(); $fr = New-Object T+RECT; [T]::GetWindowRect($fg, [ref]$fr) | Out-Null
    "full-screen edge up (foreground rect $(Fmt $fr)): visible=$(Vis) topmost=$([F]::Topmost($kh))"
    Shot 'focus-fullscreen'
    [T]::PostMessage($kh, 0x8001, [IntPtr]1, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds 500   # CMD_SHOW
    "full-screen, show requested again: visible=$(Vis) topmost=$([F]::Topmost($kh))"
    [T]::PostMessage($kh, 0x8001, [IntPtr]2, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds 600   # CMD_HIDE
    "full-screen, keyboard hidden: edge handle visible=$([F]::IsWindowVisible($hh))"
    CloseEdge
    Mark 'fullscreen edge closed'
    "full-screen closed: keyboard visible=$(Vis) edge handle visible=$([F]::IsWindowVisible($hh))"
    [T]::PostMessage($kh, 0x8001, [IntPtr]1, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds 500
    "shown again: visible=$(Vis) topmost=$([F]::Topmost($kh))"
  }

  # --- Tray menu ---------------------------------------------------------------------------------
  if (On 'tray') {
    # The icon may sit in the overflow area (no rect), so open the menu the way the shell does:
    # the icon's callback message (WM_APP+4) with WM_CONTEXTMENU, menu at the cursor.
    $th = [T]::FindOf('DianmoTray', $demo.Id)
    [F]::SetCursorPos(2000, 1500) | Out-Null
    function Menus { $AEl::RootElement.FindAll($TS::Children, (Cond $AEl::ClassNameProperty '#32768')) | Where-Object { $_.Current.BoundingRectangle.Width -gt 0 } }
    function OpenMenu { [F]::SetCursorPos(2000, 1500) | Out-Null; [T]::PostMessage($th, 0x8004, [IntPtr]::Zero, [IntPtr]0x7B) | Out-Null; Start-Sleep -Milliseconds 700
      Menus | Select-Object -First 1 }
    # UIA sees the popup menu only as an empty pane, so items are clicked by their offset from the
    # menu's top edge (200%: items ~39px, separators ~5px; measured from a screenshot).
    function ClickAt($menu, $dy, $name) {
      $r = $menu.Current.BoundingRectangle; Mark "menu $name"
      [T]::Click([int]($r.X + 80), [int]($r.Y + $dy)); Start-Sleep -Milliseconds 700
    }
    if ($th -ne [IntPtr]::Zero) {
      $m = OpenMenu
      if ($m) {
        "menu rect: $($m.Current.BoundingRectangle)"
        ClickAt $m 59 '深色主题'
        $m = OpenMenu; ClickAt $m 20 '布局'
        $sub = Menus | Sort-Object { $_.Current.BoundingRectangle.X } | Select-Object -Last 1
        if ($sub -and $sub.Current.BoundingRectangle.X -gt $m.Current.BoundingRectangle.X) { "submenu rect: $($sub.Current.BoundingRectangle)"; ClickAt $sub 59 '小鹤双拼' } else { "!! submenu not found" }
        $m = OpenMenu; ClickAt $m 20 '布局'; Start-Sleep -Milliseconds 300; Shot 'focus-tray-menu'
        $m = Menus | Sort-Object { $_.Current.BoundingRectangle.X } | Select-Object -First 1
        if ($m) { $r = $m.Current.BoundingRectangle; [T]::Click([int]($r.X + 80), [int]($r.Y + 103)); Mark 'menu 关于'; Start-Sleep -Milliseconds 600 }
        "open menus left: $(@(Menus).Count)"
      } else { "!! tray menu did not open" }
    }
  }

  Cost 'idle after tests' 10
} finally {
  if ($exWin) { $exWin.Quit() }
  if ($np) { Stop-Process -Id $np.Id -Force -ErrorAction SilentlyContinue }
  EdgeProcs | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  Get-CimInstance Win32_Process -Filter "Name='Code.exe'" | Where-Object { $_.CommandLine -like "*vscode-focus*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  if ($kh -ne [IntPtr]::Zero) { [T]::PostMessage($kh, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
  if ($demo -and !$demo.WaitForExit(4000)) { "demo did not exit on WM_CLOSE, killing"; Stop-Process -Id $demo.Id -Force } elseif ($demo) { "demo exited, code $($demo.ExitCode)" }
  if ($null -ne $oldAuto) { Set-ItemProperty $tt EnableDesktopModeAutoInvoke $oldAuto -Type DWord } else { Remove-ItemProperty $tt EnableDesktopModeAutoInvoke -ErrorAction SilentlyContinue }
  "system keyboard auto-invoke restored: $((Get-ItemProperty $tt).EnableDesktopModeAutoInvoke)"
  "work area after exit: $(Fmt ([T]::Work()))"
  Start-Sleep -Milliseconds 500
  Remove-Item -Recurse -Force "$dir\edge-focus-profile", "$dir\vscode-focus", "$dir\vscode-focus-ext", "$dir\focus-note.txt" -ErrorAction SilentlyContinue
  "----- demo log -----"; Get-Content $log -Encoding UTF8 | Where-Object { $_ -notmatch ' id=\d+ (Down|Move|Up|Cancel) ' }
  "----- uia log -----"; Get-Content $flog -Encoding UTF8 -ErrorAction SilentlyContinue
}
