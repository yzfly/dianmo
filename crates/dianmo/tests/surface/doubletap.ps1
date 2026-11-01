# 单击 / 双击打开键盘 (TODO #45, issue #1) on a test copy of 点墨 (`--instance Test --no-elevate
# --hidden`, letters-only engine), ~90 s. Prepend crates/dianmo-win/tests/surface/touch.ps1.
# $RunDir: folder with dianmo.exe and doubletap-test.html. Injected touch goes through raw input
# (VIRTUAL_DIGITIZER) and, in Win32 apps (Notepad), the synthesized mouse too: several events per
# tap. Opens Notepad and a private Edge (own profile); everything opened here is closed again.
# An installed 点墨 may be running (单击): it pops up on the same taps; tapping the page's button
# (not editable) hides both, so it is left hidden. Targets sit in the page's top strip: with two
# keyboards docked one above the other they still aren't covered.
$exe = Join-Path $RunDir 'dianmo.exe'
$page = Join-Path $RunDir 'doubletap-test.html'
$data = Join-Path $env:APPDATA 'Dianmo-Test'; $ini = Join-Path $data 'settings.ini'; $applog = Join-Path $data 'dianmo.log'
$km = Join-Path $RunDir 'keymap.txt'; $sm = Join-Path $RunDir 'settingsmap.txt'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms, System.Drawing
Add-Type @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class C {
  [StructLayout(LayoutKind.Sequential)] struct P { public int x, y; }
  [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref P p);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern IntPtr FindWindowEx(IntPtr p, IntPtr a, string c, string n);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  public static int[] Origin(IntPtr h) { var p = new P(); ClientToScreen(h, ref p); return new[] { p.x, p.y }; }
  // A visible top-level window of class `cls` of process `pid`.
  public static IntPtr VisibleOf(string cls, int pid) { IntPtr h = IntPtr.Zero; while ((h = FindWindowEx(IntPtr.Zero, h, cls, null)) != IntPtr.Zero) { uint p; GetWindowThreadProcessId(h, out p); if (p == (uint)pid && IsWindowVisible(h)) return h; } return IntPtr.Zero; }
}
'@
$TS = [System.Windows.Automation.TreeScope]; $AEl = [System.Windows.Automation.AutomationElement]
function Cond($prop, $val) { New-Object System.Windows.Automation.PropertyCondition($prop, $val) }
function FindDesc($root, $prop, $val) { $root.FindFirst($TS::Descendants, (Cond $prop $val)) }
function WaitFind($root, $prop, $val, $ms = 8000) { $d = (Get-Date).AddMilliseconds($ms); do { $e = FindDesc $root $prop $val; if ($e) { return $e }; Start-Sleep -Milliseconds 200 } while ((Get-Date) -lt $d); $null }
function Center($el) { $r = $el.Current.BoundingRectangle; @([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) }
function Vis { [T]::IsWindowVisible($kh) }
function KmHas($name) { [bool](Get-Content $km -Encoding UTF8 -ErrorAction SilentlyContinue | ? { $_.StartsWith("$name ") }) }
function Ini($k) { (Get-Content $ini -Encoding UTF8 | ? { $_ -like "$k=*" }) -join ' ' }
$results = New-Object System.Collections.Generic.List[string]
function Mark($what) { Add-Content $env:DIANMO_FOCUS_LOG "### $what" -Encoding UTF8 -ErrorAction SilentlyContinue }
function Ok($what, $ok, $info) { $results.Add("$(if ($ok) { 'PASS' } else { 'FAIL' })  $what ($info)") }
function Check($what, $want) { $v = Vis; Ok $what ($v -eq $want) "keyboard visible=$v, want $want" }
function Tap1($p, $what, $want, $wait = 900) { Mark $what; [T]::Tap($p[0], $p[1]) | Out-Null; Start-Sleep -Milliseconds $wait; Check $what $want }
function Tap2($p, $what, $want, $gap = 100, $wait = 900) {
  Mark $what; [T]::Tap($p[0], $p[1]) | Out-Null; [Threading.Thread]::Sleep($gap); [T]::Tap($p[0], $p[1]) | Out-Null
  Start-Sleep -Milliseconds $wait; Check $what $want
}
function Shot($name) { $b = [System.Windows.Forms.SystemInformation]::VirtualScreen; $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size); $bmp.Save((Join-Path $RunDir "dbl-$name.png"), [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose() }
$edge = 'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
$edgeDir = Join-Path $RunDir 'edge-profile'
function EdgeProcs { Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" | Where-Object { $_.CommandLine -like "*dianmo-dbltap-test*" } }
function CloseEdge {
  foreach ($e in EdgeProcs) { $p = Get-Process -Id $e.ProcessId -ErrorAction SilentlyContinue; try { if ($p -and $p.MainWindowHandle -ne 0) { $p.CloseMainWindow() | Out-Null } } catch {} }
  $d = (Get-Date).AddSeconds(4); while ((EdgeProcs) -and (Get-Date) -lt $d) { Start-Sleep -Milliseconds 200 }
  EdgeProcs | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}

# The installed 点墨 (not ours): only read whether its keyboard is visible.
$user = Get-Process dianmo -ErrorAction SilentlyContinue | ? { $_.Path -and -not $_.Path.StartsWith($RunDir, [StringComparison]::OrdinalIgnoreCase) } | Select -First 1
$uk = if ($user) { [T]::FindOf('DianmoKeyboard', $user.Id) } else { [IntPtr]::Zero }
"installed dianmo: $(if ($user) { "pid $($user.Id), keyboard visible=$([T]::IsWindowVisible($uk))" } else { 'not running' })"

$hadData = Test-Path $data
if ($hadData) { "note: $data existed before, keeping a copy"; Copy-Item $data "$RunDir\Dianmo-Test.before" -Recurse -Force }
New-Item -ItemType Directory -Force $data | Out-Null
# 双击 chosen earlier (as if from the settings page); a v0.2.2 file has no such key (unit tested).
Set-Content $ini -Encoding UTF8 -Value @('onboarded=true', 'input_mode=keyboard', 'auto_show=true', 'auto_show_click=double', 'schema=pinyin')
$log0 = @(Get-Content $applog -ErrorAction SilentlyContinue).Count
$env:DIANMO_ENGINE = 'basic'
$env:DIANMO_KEYMAP = $km; $env:DIANMO_SETTINGSMAP = $sm
$env:DIANMO_FOCUS_LOG = Join-Path $RunDir 'focus.log'
Remove-Item $km, $sm, $env:DIANMO_FOCUS_LOG -ErrorAction SilentlyContinue
$np = $null; $dm = $null; $kh = [IntPtr]::Zero
try {
  $dm = Start-Process $exe -ArgumentList @('--instance', 'Test', '--no-elevate', '--hidden') -PassThru
  $d = (Get-Date).AddSeconds(8)
  while ($kh -eq [IntPtr]::Zero -and (Get-Date) -lt $d) { Start-Sleep -Milliseconds 50; $kh = [T]::FindOf('DianmoKeyboard', $dm.Id) }
  Start-Sleep -Milliseconds 2000
  "test instance pid $($dm.Id), keyboard window $kh, visible=$(Vis)"

  # --- Notepad (Win32: synthesized mouse + raw input for every tap) ---
  $np = Start-Process notepad -PassThru
  $d = (Get-Date).AddSeconds(8); while ($np.MainWindowHandle -eq 0 -and (Get-Date) -lt $d) { Start-Sleep -Milliseconds 100; $np.Refresh() }
  $nh = $np.MainWindowHandle; [T]::ShowWindow($nh, 3) | Out-Null; Start-Sleep -Milliseconds 800
  $edit = [T]::FindWindowEx($nh, [IntPtr]::Zero, 'Edit', [NullString]::Value)
  $r = New-Object T+RECT; [T]::GetWindowRect($edit, [ref]$r) | Out-Null
  $np1 = @([int](($r.left + $r.right) / 2), [int]($r.top + 200))
  Tap1 $np1 'notepad double: one tap (focus moves) does not open' $false
  Tap1 $np1 'notepad double: a slow second tap does not open' $false
  Tap2 $np1 'notepad double: double tap opens' $true

  # --- Edge (Chromium: raw input only) ---
  Start-Process $edge -ArgumentList "--user-data-dir=$edgeDir", '--no-first-run', '--no-default-browser-check', '--disable-sync', '--hide-crash-restore-bubble', '--start-maximized', '--new-window', "file:///$($page -replace '\\','/')"
  $ew = $null; $d = (Get-Date).AddSeconds(20)
  while (!$ew -and (Get-Date) -lt $d) { Start-Sleep -Milliseconds 300
    $ew = $AEl::RootElement.FindAll($TS::Children, (Cond $AEl::ClassNameProperty 'Chrome_WidgetWin_1')) | Where-Object { $_.Current.Name -like '*点墨双击测试*' } | Select-Object -First 1 }
  $txt = if ($ew) { WaitFind $ew $AEl::AutomationIdProperty 'txt' 10000 }
  if (!$txt) { $results.Add('FAIL  edge test page not found'); throw 'no edge page' }
  [T]::ShowWindow([IntPtr]$ew.Current.NativeWindowHandle, 3) | Out-Null; Start-Sleep -Milliseconds 1500
  # Positions move when a keyboard docks (AppBar) or the window settles: look them up per tap.
  $P = New-Object PSObject
  foreach ($id in 'txt', 'area', 'url', 'btn') { $P | Add-Member ScriptProperty $id ([ScriptBlock]::Create("Center (FindDesc `$ew `$AEl::AutomationIdProperty '$id')")) }
  Tap1 $P.btn 'edge: tap the button (not editable) hides' $false 1200
  Tap1 $P.txt 'edge double: one tap does not open' $false
  $a = $P.txt; $b = $P.area; Mark 'edge double: tap two different fields'; [T]::Tap($a[0], $a[1]) | Out-Null; [Threading.Thread]::Sleep(100); [T]::Tap($b[0], $b[1]) | Out-Null
  Start-Sleep -Milliseconds 900; Check 'edge double: one tap each on two fields does not open' $false
  Tap2 $P.area 'edge double: double tap the textarea opens' $true
  Tap1 $P.url 'edge double, shown: one tap on the url field keeps it open' $true 1200
  Ok 'edge double, shown: url hints follow the new field' (KmHas '.com') "quick key .com=$(KmHas '.com')"
  Shot 'url'
  Tap1 $P.txt 'edge double, shown: one tap back on the text field' $true 1200
  Ok 'edge double, shown: url hints gone on the text field' (-not (KmHas '.com')) "quick key .com=$(KmHas '.com')"
  Tap1 $P.btn 'edge double: tap outside still hides' $false 1200
  $a = $P.txt; $b = $P.btn; Mark 'edge double: tap outside between two taps'; [T]::Tap($a[0], $a[1]) | Out-Null; [Threading.Thread]::Sleep(60); [T]::Tap($b[0], $b[1]) | Out-Null; [Threading.Thread]::Sleep(60); [T]::Tap($a[0], $a[1]) | Out-Null
  Start-Sleep -Milliseconds 900; Check 'edge double: field, outside, field does not open' $false
  Tap1 $P.btn 'edge: reset' $false 900

  # --- Settings page: switch back to 单击 at run time ---
  $p2 = Start-Process $exe -ArgumentList @('--instance', 'Test', '--no-elevate', '--settings') -PassThru; $null = $p2.WaitForExit(5000)
  $sw = [IntPtr]::Zero; $d = (Get-Date).AddSeconds(8)
  while ($sw -eq [IntPtr]::Zero -and (Get-Date) -lt $d) { Start-Sleep -Milliseconds 150; $sw = [C]::VisibleOf('DianmoAppWindow', $dm.Id) }
  Start-Sleep -Milliseconds 1500
  $line = $null; $d = (Get-Date).AddSeconds(5)
  while (!$line -and (Get-Date) -lt $d) { $line = Get-Content $sm -Encoding UTF8 -ErrorAction SilentlyContinue | ? { $_ -like "auto_show_click/单击`t*" } | Select -First 1; if (!$line) { Start-Sleep -Milliseconds 200 } }
  "settings window $sw, segment: $line"
  if ($sw -ne [IntPtr]::Zero -and $line) {
    $f = $line -split "`t"; $o = [C]::Origin($sw); $s = [T]::GetDpiForWindow($sw) / 96.0
    $wsr = New-Object T+RECT; [T]::GetWindowRect($sw, [ref]$wsr) | Out-Null
    $bmp = New-Object System.Drawing.Bitmap ($wsr.right - $wsr.left), ($wsr.bottom - $wsr.top)
    [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($wsr.left, $wsr.top, 0, 0, $bmp.Size); $bmp.Save((Join-Path $RunDir 'dbl-settings-double.png')); $bmp.Dispose()
    [T]::Tap([int]($o[0] + [double]$f[1] * $s), [int]($o[1] + [double]$f[2] * $s)) | Out-Null; Start-Sleep -Milliseconds 800
    Ok 'settings: 单击 saved' ((Ini 'auto_show_click') -eq 'auto_show_click=single') (Ini 'auto_show_click')
    [T]::PostMessage($sw, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null; Start-Sleep -Milliseconds 800
  } else { $results.Add('FAIL  settings segment not found') }

  # --- 单击 again (default behaviour) ---
  Tap1 $P.txt 'edge single: one tap opens' $true 1200
  Tap1 $P.btn 'edge single: tap outside hides' $false 1200
  Tap1 $P.area 'edge single: one tap on another field opens' $true 1200
  Tap1 $P.btn 'edge single: hide again' $false 1200
  # Close the test copy, then tap the button once more: an installed 点墨 is left hidden.
  [T]::PostMessage($kh, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null; $null = $dm.WaitForExit(5000)
  $b = $P.btn; [T]::Tap($b[0], $b[1]) | Out-Null; Start-Sleep -Milliseconds 1200
  CloseEdge
  Stop-Process -Id $np.Id -Force; $np = $null
} catch { "error: $_" } finally {
  CloseEdge
  if ($np) { Stop-Process -Id $np.Id -Force -ErrorAction SilentlyContinue }
  if ($kh -ne [IntPtr]::Zero) { [T]::PostMessage($kh, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
  if ($dm -and !$dm.WaitForExit(5000)) { "test dianmo did not exit on WM_CLOSE, killing"; Stop-Process -Id $dm.Id -Force } elseif ($dm) { "test dianmo exited, code $($dm.ExitCode)" }
  "installed dianmo after: $(if ($user) { "pid $($user.Id) alive=$(!$user.HasExited), keyboard visible=$([T]::IsWindowVisible($uk))" } else { 'not running' })"
  "--- results ---"; $results
  "--- dianmo.log (this run, auto show / shown / hidden) ---"
  Get-Content $applog -Encoding UTF8 -ErrorAction SilentlyContinue | Select -Skip $log0 | ? { $_ -match 'auto show|keyboard (shown|hidden)|error|panic' }
  "--- focus log (events and marks) ---"
  Get-Content $env:DIANMO_FOCUS_LOG -Encoding UTF8 -ErrorAction SilentlyContinue | ? { $_ -match '^###|retap|Editable|NotEditable|= raw down|dup' } | % { if ($_.Length -gt 200) { $_.Substring(0, 200) } else { $_ } } | Select -Last 120
  Remove-Item $data -Recurse -Force -ErrorAction SilentlyContinue
  if ($hadData) { Copy-Item "$RunDir\Dianmo-Test.before" $data -Recurse -Force }
}
