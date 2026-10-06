# Dot-sourced after crates/dianmo-win/tests/surface/touch.ps1 (class T). Needs $RunDir, $Keys.
# $Keys: key names from the keymap (see DianmoApp's DIANMO_KEYMAP hook), or
#   SHOT        screenshot to C:\Users\wecode\claude\dm-step<N>.png
#   WAIT<ms>    sleep
#   VIS         print whether the keyboard is visible
#   TRAYMENU    right-click the tray icon, screenshot the menu, Escape
#   TRAYPICK<n> pick the n-th tray menu item (keyboard navigation)
#   RUNKEY      print the HKCU Run entry
#   @x,y        tap at screen pixel x,y (for panels whose keys aren't in the keymap)
$exe = Join-Path $RunDir 'dianmo.exe'
$km = 'C:\Users\wecode\claude\dianmo-keymap.txt'
$applog = Join-Path $env:APPDATA 'Dianmo\dianmo.log'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
function Shot($name) { $b = [System.Windows.Forms.SystemInformation]::VirtualScreen; $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size); $bmp.Save("C:\Users\wecode\claude\dm-$name.png", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose() }
function TabTip { $k = Get-ItemProperty 'HKCU:\Software\Microsoft\TabletTip\1.7' -ErrorAction SilentlyContinue
  "EnableDesktopModeAutoInvoke=$($k.EnableDesktopModeAutoInvoke) TouchKeyboardTapInvoke=$($k.TouchKeyboardTapInvoke)" }
if (Get-Process dianmo -ErrorAction SilentlyContinue) { "dianmo already running; aborting test"; return }
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
$dm = Start-Process $exe -PassThru
$deadline = (Get-Date).AddSeconds(15); $kh = [IntPtr]::Zero
while ($kh -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 20; $kh = [T]::FindWindow('DianmoKeyboard', [NullString]::Value) }
while (-not [T]::IsWindowVisible($kh) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 20 }
"keyboard visible after $([int]((Get-Date) - $t0).TotalMilliseconds)ms (incl. process start)"
Start-Sleep -Milliseconds 800
try {
  "tabtip while running: $(TabTip)"
  "foreground still notepad: $([T]::GetForegroundWindow() -eq $nh)"
  $k = New-Object T+RECT; [T]::GetWindowRect($kh, [ref]$k) | Out-Null; "keyboard rect: $(Fmt $k)"
  $s = [T]::GetDpiForWindow($kh) / 96.0
  $map = @{}; Get-Content $km -Encoding UTF8 | % { $f = $_ -split ' '; $map[$f[0]] = @([double]$f[1], [double]$f[2]) }
  "keymap entries: $($map.Count)"
  function TapKey($name) {
    if (-not $map.ContainsKey($name)) { "no key $name"; return }
    $p = $map[$name]; $x = [int]($k.left + $p[0] * $s); $y = [int]($k.top + $p[1] * $s)
    $ok = [T]::Tap($x, $y); Start-Sleep -Milliseconds 180
  }
  $i = 0
  foreach ($key in $Keys) {
    if ($key -eq 'SHOT') { $i++; Shot "step$i"; continue }
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
  $dm2 = Start-Process $exe -PassThru; $null = $dm2.WaitForExit(5000)
  Start-Sleep -Milliseconds 500
  "second instance exited=$($dm2.HasExited); keyboard visible again=$([T]::IsWindowVisible($kh)); dianmo processes=$(@(Get-Process dianmo).Count)"
} finally {
  if ($kh -ne [IntPtr]::Zero) { [T]::PostMessage($kh, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
  if (!$dm.WaitForExit(5000)) { "dianmo did not exit on WM_CLOSE, killing"; Stop-Process -Id $dm.Id -Force } else { "dianmo exited, code $($dm.ExitCode)" }
  Start-Sleep -Milliseconds 300
  "tabtip after: $(TabTip)"
  "work area after exit: $(Fmt ([T]::Work())) (before: $(Fmt $work0))"
  Stop-Process -Id $np.Id -Force
  "--- dianmo.log (this run) ---"
  Get-Content $applog -Encoding UTF8 -ErrorAction SilentlyContinue | Select -Skip $logLines0
}
