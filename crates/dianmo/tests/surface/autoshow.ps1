# Auto show / hide of the real app (a test copy, `--instance Test --no-elevate --hidden`) in
# Notepad, an Explorer window's search box, a private Edge instance (own profile), VS Code (own
# profile: editor, re-tap of the already focused editor after hiding, terminal) and an elevated
# PowerShell console. Run with crates/dianmo/tests/surface/run-autoshow.sh (prepends touch.ps1).
# $RunDir: folder with dianmo.exe; $Sections: all or a comma list of
# notepad,explorer,edge,vscode,admin. Everything opened here is closed again.
$dir = 'C:\Users\wecode\claude'
$exe = Join-Path $RunDir 'dianmo.exe'
$applog = Join-Path $env:APPDATA 'Dianmo-Test\dianmo.log'
$km = "$dir\dianmo-keymap.txt"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms, System.Drawing
Add-Type -TypeDefinition @'
using System; using System.Threading; using System.Runtime.InteropServices;
public static class F {
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, IntPtr extra);
  public static void Key(byte vk) { keybd_event(vk, 0, 0, IntPtr.Zero); Thread.Sleep(30); keybd_event(vk, 0, 2, IntPtr.Zero); }
  public static void Chord(byte mod, byte vk) { keybd_event(mod, 0, 0, IntPtr.Zero); Thread.Sleep(30); Key(vk); Thread.Sleep(30); keybd_event(mod, 0, 2, IntPtr.Zero); }
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
function Vis { [T]::IsWindowVisible($kh) }
$results = New-Object System.Collections.Generic.List[string]
function Check($what, $want) { Add-Content $env:DIANMO_FOCUS_LOG "### $what" -Encoding UTF8 -ErrorAction SilentlyContinue; $v = Vis; $ok = $v -eq $want; $results.Add("$(if ($ok) { 'PASS' } else { 'FAIL' })  $what (keyboard visible=$v, want $want)"); "$what -> visible=$v (want $want)" }
function TapP($p, $what, $want, $wait = 1000) { [T]::Tap($p[0], $p[1]) | Out-Null; Start-Sleep -Milliseconds $wait; Check $what $want }
function TapEl($el, $what, $want, $wait = 1000) { if (!$el) { $results.Add("FAIL  $what (element not found)"); "!! $what not found"; return }; TapP (Center $el) $what $want $wait }
function Shot($name) { $b = [System.Windows.Forms.SystemInformation]::VirtualScreen; $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size); $bmp.Save("$dir\auto-$name.png", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose() }
# The hide key, from the test hook's keymap (DIPs, window coordinates).
function HideKeyTap {
  $line = Get-Content $km -Encoding UTF8 -ErrorAction SilentlyContinue | ? { $_ -like 'hide *' } | Select -First 1
  if (-not $line) { "no hide key in keymap"; return }
  $f = $line -split ' '; $r = New-Object T+RECT; [T]::GetWindowRect($kh, [ref]$r) | Out-Null; $s = [T]::GetDpiForWindow($kh) / 96.0
  [T]::Tap([int]($r.left + [double]$f[1] * $s), [int]($r.top + [double]$f[2] * $s)) | Out-Null; Start-Sleep -Milliseconds 700
}
function On($n) { $Sections -eq 'all' -or ($Sections -split ',') -contains $n }
$edge = 'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
$edgeDir = "$dir\edge-focus-profile"
function EdgeProcs { Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" | Where-Object { $_.CommandLine -like "*edge-focus-profile*" } }
function CloseEdge {
  foreach ($e in EdgeProcs) { $p = Get-Process -Id $e.ProcessId -ErrorAction SilentlyContinue; try { if ($p -and $p.MainWindowHandle -ne 0) { $p.CloseMainWindow() | Out-Null } } catch {} }
  $deadline = (Get-Date).AddSeconds(4); while ((EdgeProcs) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
  EdgeProcs | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

$mine = @(Get-Process dianmo -ErrorAction SilentlyContinue | ? { $_.Path -and $_.Path.StartsWith($RunDir, [StringComparison]::OrdinalIgnoreCase) })
if ($mine.Count) { "a test dianmo from $RunDir is already running; aborting"; return }
$ini = Join-Path $env:APPDATA 'Dianmo-Test\settings.ini'; New-Item -ItemType Directory -Force (Split-Path $ini) | Out-Null
Set-Content $ini -Encoding UTF8 -Value @('onboarded=true', 'input_mode=keyboard', 'auto_show=true', 'schema=pinyin')
$log0 = @(Get-Content $applog -ErrorAction SilentlyContinue).Count
$env:DIANMO_KEYMAP = $km; Remove-Item $km -ErrorAction SilentlyContinue
$env:DIANMO_FOCUS_LOG = "$dir\dianmo-focus.log"; Remove-Item $env:DIANMO_FOCUS_LOG -ErrorAction SilentlyContinue
$np = $null; $dm = $null; $kh = [IntPtr]::Zero; $exWin = $null; $adm = $null
try {
  $dm = Start-Process $exe -ArgumentList @('--instance', 'Test', '--no-elevate', '--hidden') -PassThru
  $deadline = (Get-Date).AddSeconds(8)
  while ($kh -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 50; $kh = [T]::FindOf('DianmoKeyboard', $dm.Id) }
  Start-Sleep -Milliseconds 2500
  "started hidden, keyboard visible=$(Vis)"

  if (On 'notepad') {
    $np = Start-Process notepad -PassThru
    $deadline = (Get-Date).AddSeconds(8)
    while ($np.MainWindowHandle -eq 0 -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100; $np.Refresh() }
    $nh = $np.MainWindowHandle; [T]::ShowWindow($nh, 3) | Out-Null; Start-Sleep -Milliseconds 800
    Check 'notepad opened (focus without touch)' $false
    $edit = [T]::FindWindowEx($nh, [IntPtr]::Zero, 'Edit', [NullString]::Value)
    $r = New-Object T+RECT; [T]::GetWindowRect($edit, [ref]$r) | Out-Null
    TapP @([int](($r.left + $r.right) / 2), [int]($r.top + 200)) 'notepad: tap the edit area' $true
    HideKeyTap; Check 'notepad: hide key' $false
    TapP @([int](($r.left + $r.right) / 2), [int]($r.top + 200)) 'notepad: tap the (focused) edit area again' $true
    TapP @(1440, 12) 'notepad: tap the title bar (focus stays in the edit area)' $true
    Stop-Process -Id $np.Id -Force; $np = $null; Start-Sleep -Milliseconds 600
  }

  if (On 'explorer') {
    $before = @((New-Object -ComObject Shell.Application).Windows() | ForEach-Object { $_.HWND })
    Start-Process explorer.exe $dir
    $deadline = (Get-Date).AddSeconds(8)
    while (!$exWin -and (Get-Date) -lt $deadline) {
      Start-Sleep -Milliseconds 300
      $exWin = (New-Object -ComObject Shell.Application).Windows() | Where-Object { $before -notcontains $_.HWND } | Select-Object -First 1
    }
    if ($exWin) {
      $eh = [IntPtr]$exWin.HWND; Start-Sleep -Milliseconds 1200
      $ewin = $AEl::FromHandle($eh)
      $search = FindDesc $ewin $AEl::ClassNameProperty 'UniversalSearchBand'
      TapEl $search 'explorer: search box' $true
      Shot 'explorer'
      $list = FindDesc $ewin $AEl::ClassNameProperty 'UIItemsView'
      TapEl $list 'explorer: file list (not editable)' $false
      $exWin.Quit(); $exWin = $null; Start-Sleep -Milliseconds 600
    } else { $results.Add('FAIL  explorer window not found') }
  }

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
      TapEl (FindDesc $edgeWin $AEl::AutomationIdProperty 'txt') 'edge: text input' $true
      Shot 'edge'
      HideKeyTap; Check 'edge: hide key' $false
      TapEl (FindDesc $edgeWin $AEl::AutomationIdProperty 'txt') 'edge: tap the (focused) text input again' $true
      $wr = $edgeWin.Current.BoundingRectangle
      TapP @([int]($wr.X + $wr.Width * 0.5), [int]($wr.Y + $wr.Height - 80)) 'edge: empty page area (not editable)' $false
      Shot 'edge-blank'
      TapEl (FindDesc $edgeWin $AEl::AutomationIdProperty 'area') 'edge: textarea' $true
      TapEl (FindDesc $edgeWin $AEl::AutomationIdProperty 'btn') 'edge: button (not editable)' $false
      TapEl (FindDesc $edgeWin $AEl::AutomationIdProperty 'txt') 'edge: text input after the button' $true
      Shot 'edge-area'
      TapEl (FindDesc $edgeWin $AEl::ClassNameProperty 'OmniboxViewViews') 'edge: address bar' $true
      [F]::Key(0x1B); Start-Sleep -Milliseconds 300
    } else { $results.Add('FAIL  edge test page not found') }
    CloseEdge
  }

  if (On 'vscode') {
    $code = "$env:LOCALAPPDATA\Programs\Microsoft VS Code\Code.exe"
    if (Test-Path $code) {
      Set-Content "$dir\focus-note.txt" 'dianmo focus test' -Encoding UTF8
      $codeArgs = @("--user-data-dir=$dir\vscode-focus", "--extensions-dir=$dir\vscode-focus-ext", '--disable-extensions', '--disable-workspace-trust', '--skip-welcome', '--skip-release-notes', '--new-window', "$dir\focus-note.txt")
      Start-Process $code -ArgumentList $codeArgs | Out-Null
      $codeWin = $null; $deadline = (Get-Date).AddSeconds(25)
      while (!$codeWin -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 400
        $codeWin = $AEl::RootElement.FindAll($TS::Children, (Cond $AEl::ClassNameProperty 'Chrome_WidgetWin_1')) | Where-Object { $_.Current.Name -like '*focus-note.txt*' } | Select-Object -First 1
      }
      if ($codeWin) {
        Start-Sleep -Milliseconds 3000
        $cw = [IntPtr]$codeWin.Current.NativeWindowHandle; [T]::ShowWindow($cw, 3) | Out-Null; Start-Sleep -Milliseconds 800
        $cr = $codeWin.Current.BoundingRectangle
        TapP @([int]($cr.X + $cr.Width * 0.5), [int]($cr.Y + $cr.Height * 0.3)) 'vscode: editor' $true 1300
        Shot 'vscode'
        HideKeyTap; Check 'vscode: hide key' $false
        $cr = $codeWin.Current.BoundingRectangle
        TapP @([int]($cr.X + $cr.Width * 0.5), [int]($cr.Y + $cr.Height * 0.3)) 'vscode: tap the (focused) editor again' $true 1300
        HideKeyTap
        TapP @([int]($cr.X + 24), [int]($cr.Y + $cr.Height * 0.12)) 'vscode: activity bar (not editable)' $false 1300
        # Terminal: Ctrl+` (focus goes there without a touch), then tap into it.
        TapP @([int]($cr.X + $cr.Width * 0.5), [int]($cr.Y + $cr.Height * 0.3)) 'vscode: editor (before terminal)' $true 1300
        HideKeyTap
        [F]::Chord(0x11, 0xC0); Start-Sleep -Milliseconds 3500
        $cr = $codeWin.Current.BoundingRectangle
        TapP @([int]($cr.X + $cr.Width * 0.5), [int]($cr.Y + $cr.Height * 0.85)) 'vscode: terminal' $true 1500
        Shot 'vscode-terminal'
        HideKeyTap; Check 'vscode terminal: hide key' $false
        $cr = $codeWin.Current.BoundingRectangle
        TapP @([int]($cr.X + $cr.Width * 0.5), [int]($cr.Y + $cr.Height * 0.85)) 'vscode: tap the (focused) terminal again' $true 1500
      } else { $results.Add('FAIL  vscode window not found') }
      Get-CimInstance Win32_Process -Filter "Name='Code.exe'" | Where-Object { $_.CommandLine -like "*vscode-focus*" } | ForEach-Object {
        $p = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue; try { if ($p -and $p.MainWindowHandle -ne 0) { $p.CloseMainWindow() | Out-Null } } catch {} }
      Start-Sleep -Milliseconds 2500
      Get-CimInstance Win32_Process -Filter "Name='Code.exe'" | Where-Object { $_.CommandLine -like "*vscode-focus*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
    } else { $results.Add('SKIP  vscode not installed') }
  }

  if (On 'admin') {
    # gui.sh runs elevated: this console is an administrator PowerShell.
    if (Vis) { HideKeyTap }
    $adm = Start-Process powershell.exe -ArgumentList '-NoProfile', '-NoExit', '-Command', '$Host.UI.RawUI.WindowTitle = ''dianmo-admin-test''' -PassThru
    $deadline = (Get-Date).AddSeconds(8)
    while ($adm.MainWindowHandle -eq 0 -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100; $adm.Refresh() }
    Start-Sleep -Milliseconds 1200
    $ar = New-Object T+RECT; [T]::GetWindowRect($adm.MainWindowHandle, [ref]$ar) | Out-Null
    TapP @([int](($ar.left + $ar.right) / 2), [int](($ar.top + $ar.bottom) / 2)) 'admin powershell: tap the console' $true 1300
    Shot 'admin'
    $t = Get-Content $km -Encoding UTF8 | ? { $_ -like 'e *' } | Select -First 1
    "admin console typing: keymap has e=$([bool]$t)"
    Stop-Process -Id $adm.Id -Force; $adm = $null
  }
} finally {
  if ($exWin) { try { $exWin.Quit() } catch {} }
  if ($np) { Stop-Process -Id $np.Id -Force -ErrorAction SilentlyContinue }
  if ($adm) { Stop-Process -Id $adm.Id -Force -ErrorAction SilentlyContinue }
  if ($kh -ne [IntPtr]::Zero) { [T]::PostMessage($kh, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
  if ($dm -and !$dm.WaitForExit(5000)) { "dianmo did not exit on WM_CLOSE, killing"; Stop-Process -Id $dm.Id -Force } elseif ($dm) { "dianmo exited, code $($dm.ExitCode)" }
  "--- focus log (DIANMO_FOCUS_LOG, last 50) ---"; Get-Content $env:DIANMO_FOCUS_LOG -Encoding UTF8 -ErrorAction SilentlyContinue | Select -Last 50
  "--- results ---"; $results
  "--- dianmo.log (this run) ---"
  Get-Content $applog -Encoding UTF8 -ErrorAction SilentlyContinue | Select -Skip $log0 | Select -Last 60
}
