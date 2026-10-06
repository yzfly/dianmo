$exe = 'C:\dev\dianmo-win\target\release\examples\demo.exe'
$log = 'C:\Users\wecode\claude\demo-pointer.log'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
function Shot($name) { $b = [System.Windows.Forms.SystemInformation]::VirtualScreen; $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size); $bmp.Save("C:\Users\wecode\claude\$name.png", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose() }
$work0 = [T]::Work(); "work area before: $(Fmt $work0)"
$np = Start-Process notepad -PassThru
$deadline = (Get-Date).AddSeconds(8)
while ($np.MainWindowHandle -eq 0 -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100; $np.Refresh() }
$nh = $np.MainWindowHandle; $edit = [T]::FindWindowEx($nh, [IntPtr]::Zero, 'Edit', [NullString]::Value)
[T]::ShowWindow($nh, 3) | Out-Null; Start-Sleep -Milliseconds 400
$r = New-Object T+RECT; [T]::GetWindowRect($nh, [ref]$r) | Out-Null; "notepad maximized, no keyboard: $(Fmt $r)"
[T]::Tap(1200, 16) | Out-Null; Start-Sleep -Milliseconds 300   # title bar: focus without popping the system keyboard
"foreground is notepad: $([T]::GetForegroundWindow() -eq $nh)"
$env:DIANMO_DEMO_LOG = $log
$t0 = Get-Date
$demo = Start-Process $exe -PassThru
$deadline = (Get-Date).AddSeconds(8); $kh = [IntPtr]::Zero
while ($kh -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 20; $kh = [T]::FindWindow('DianmoKeyboard', [NullString]::Value) }
while (-not [T]::IsWindowVisible($kh) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 20 }
"keyboard window visible after $([int]((Get-Date) - $t0).TotalMilliseconds)ms (incl. process start)"
Start-Sleep -Milliseconds 1000
try {
  "foreground still notepad after launch: $([T]::GetForegroundWindow() -eq $nh)"
  $k = New-Object T+RECT; [T]::GetWindowRect($kh, [ref]$k) | Out-Null; "keyboard rect: $(Fmt $k)"
  $s = [T]::GetDpiForWindow($kh) / 96.0; "keyboard dpi scale: $s"
  "work area with keyboard: $(Fmt ([T]::Work()))"
  [T]::GetWindowRect($nh, [ref]$r) | Out-Null; "notepad maximized, keyboard shown: $(Fmt $r)  (bottom<=keyboard top: $($r.bottom -le $k.top + 13))"
  $W = ($k.right - $k.left) / $s; $H = ($k.bottom - $k.top) / $s; $rh = ($H - 52) / 2; $u = ($W - 6) / 5.5
  function P($xd, $yd) { @([int]($k.left + $xd * $s), [int]($k.top + $yd * $s)) }
  $y1 = 52 + $rh / 2; $y2 = 52 + 1.5 * $rh
  $ni = P (3 + $u * 0.5) $y1; $hao = P (3 + $u * 1.5) $y1; $dou = P (3 + $u * 2.5) $y1; $bs = P (3 + $u * 4.75) $y1
  $mic = P (3 + $u * 0.5) $y2; $enter = P (3 + $u * 4) $y2; $chip0 = P 50 33
  function Txt { ([T]::Text($edit) -replace "`r`n",'\n') }
  function Tap($p, $what) { $ok = [T]::Tap($p[0], $p[1]); Start-Sleep -Milliseconds 200; "tap $what ok=$ok fg=$([T]::GetForegroundWindow() -eq $nh) text=[$(Txt)]" }
  Tap $ni '你'; Tap $hao '好'; Tap $bs '⌫'; Tap $hao '好'; Tap $enter '⏎'
  $ok = [T]::Two($ni[0], $ni[1], $hao[0], $hao[1]); Start-Sleep -Milliseconds 250
  "two overlapping touches ok=$ok fg=$([T]::GetForegroundWindow() -eq $nh) text=[$(Txt)]"
  Tap $chip0 'candidate 你好'
  [T]::Click($dou[0], $dou[1]); Start-Sleep -Milliseconds 250
  "mouse click ， fg=$([T]::GetForegroundWindow() -eq $nh) text=[$(Txt)]"
  # tray icon: tap toggles
  $th = [T]::FindWindow('DianmoTray', [NullString]::Value); $tr = [T]::TrayRect($th); "tray icon rect: $(Fmt $tr)"
  if ($tr.right -gt 0) {
    [T]::Tap([int](($tr.left + $tr.right) / 2), [int](($tr.top + $tr.bottom) / 2)) | Out-Null; Start-Sleep -Milliseconds 600
    "after tray tap: keyboard visible=$([T]::IsWindowVisible($kh)) work=$(Fmt ([T]::Work()))"
    [T]::Tap([int](($tr.left + $tr.right) / 2), [int](($tr.top + $tr.bottom) / 2)) | Out-Null; Start-Sleep -Milliseconds 600
    "after 2nd tray tap: keyboard visible=$([T]::IsWindowVisible($kh)) work=$(Fmt ([T]::Work()))"
    [T]::Tap(1200, 16) | Out-Null; Start-Sleep -Milliseconds 300   # tray tap moved focus to the taskbar; back to notepad
    "foreground notepad again: $([T]::GetForegroundWindow() -eq $nh)"
  }
  Shot 'final-keyboard'
  # idle cost
  $p = Get-Process -Id $demo.Id; $c0 = $p.TotalProcessorTime.TotalMilliseconds
  Start-Sleep -Seconds 10; $p.Refresh(); $c1 = $p.TotalProcessorTime.TotalMilliseconds
  "memory: private=$([math]::Round($p.PrivateMemorySize64/1MB,1))MB workingset=$([math]::Round($p.WorkingSet64/1MB,1))MB; handles=$($p.HandleCount); cpu total=$([int]$c1)ms, idle 10s delta=$([int]($c1-$c0))ms"
  # voice: Win+H from the mic key, then close it again
  Tap $mic 'mic (Win+H)'
  Start-Sleep -Milliseconds 2500; Shot 'voice'
  "foreground during voice: $([T]::GetForegroundWindow()) (notepad=$nh)"
  Tap $mic 'mic again (close)'
  Start-Sleep -Milliseconds 1500; Shot 'voice-closed'
} finally {
  if ($kh -ne [IntPtr]::Zero) { [T]::PostMessage($kh, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
  if (!$demo.WaitForExit(3000)) { "demo did not exit on WM_CLOSE, killing"; Stop-Process -Id $demo.Id -Force } else { "demo exited, code $($demo.ExitCode)" }
  Start-Sleep -Milliseconds 300
  "work area after exit: $(Fmt ([T]::Work())) (before: $(Fmt $work0))"
  Stop-Process -Id $np.Id -Force
  $lines = Get-Content $log -ErrorAction SilentlyContinue; "pointer events logged: $($lines.Count); max simultaneous: $(($lines[-1] -split 'max=')[1])"
}
