# One short GUI pass over the settings window and onboarding of a test copy (`--instance Test
# --no-elevate`), ~40 s. Run with crates/dianmo/tests/surface/run-settings.sh (prepends touch.ps1).
# $RunDir: folder with dianmo.exe (+ rime.dll, data\rime).
# Only windows of the test process are touched; everything it opened is closed at the end.
# Screenshots (window frame only): C:\Users\wecode\claude\set-*.png
# Coordinates (DIPs, 960x680 client) come from dianmo-ui's SettingsView::element_center.
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class S {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int size);
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  // Visible window of process `pid` with class `cls` whose title contains `title` ("" = any).
  public static IntPtr Find(uint pid, string cls, string title) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p); if (p != pid) return true;
      var c = new StringBuilder(64); GetClassName(h, c, 64); if (c.ToString() != cls) return true;
      var t = new StringBuilder(128); GetWindowText(h, t, 128);
      if (title != "" && !t.ToString().Contains(title)) return true;
      if (cls == "DianmoAppWindow" && !IsWindowVisible(h)) return true;
      found = h; return false; }, IntPtr.Zero);
    return found;
  }
}
'@
$exe = Join-Path $RunDir 'dianmo.exe'
$data = Join-Path $env:APPDATA 'Dianmo-Test'; $ini = Join-Path $data 'settings.ini'; $log = Join-Path $data 'dianmo.log'
New-Item -ItemType Directory -Force $data | Out-Null
Set-Content $ini -Encoding UTF8 -Value @('onboarded=false', 'theme=light', 'input_mode=keyboard', 'height=1', 'voice_engine=wetype')
$log0 = @(Get-Content $log -ErrorAction SilentlyContinue).Count
function Ini($k) { (Get-Content $ini -Encoding UTF8 | ? { $_ -like "$k=*" }) -join ' ' }
function WaitWin($title, $sec = 8) { $d = (Get-Date).AddSeconds($sec); do { $h = [S]::Find([uint32]$dm.Id, 'DianmoAppWindow', $title); if ($h -ne [IntPtr]::Zero) { return $h }; Start-Sleep -Milliseconds 100 } while ((Get-Date) -lt $d); [IntPtr]::Zero }
function Origin($h) { $p = New-Object S+POINT; [S]::ClientToScreen($h, [ref]$p) | Out-Null; $p }
function TapDip($h, $x, $y) { $o = Origin $h; $s = [S]::GetDpiForWindow($h) / 96.0; [T]::Tap([int]($o.x + $x * $s), [int]($o.y + $y * $s)) | Out-Null; Start-Sleep -Milliseconds 350 }
function Key($h, $vk) { [S]::PostMessage($h, 0x100, [IntPtr]$vk, [IntPtr]0) | Out-Null; [S]::PostMessage($h, 0x101, [IntPtr]$vk, [IntPtr]0) | Out-Null; Start-Sleep -Milliseconds 250 }
function ClientH($h) { $r = New-Object S+RECT; [S]::GetClientRect($h, [ref]$r) | Out-Null; $r.bottom / ([S]::GetDpiForWindow($h) / 96.0) }
function Shot($h, $name) {
  $r = New-Object S+RECT; [S]::DwmGetWindowAttribute($h, 9, [ref]$r, 16) | Out-Null   # extended frame bounds
  $bmp = New-Object System.Drawing.Bitmap ($r.right - $r.left), ($r.bottom - $r.top)
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($r.left, $r.top, 0, 0, $bmp.Size)
  $bmp.Save("C:\Users\wecode\claude\set-$name.png", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
  "shot $name ($($r.right - $r.left)x$($r.bottom - $r.top))"
}
$t0 = Get-Date
$dm = Start-Process $exe -ArgumentList @('--instance', 'Test', '--no-elevate', '--hidden') -PassThru
try {
  # 1. First start: onboarding.
  $ob = WaitWin '欢迎'
  "onboarding window: $($ob -ne [IntPtr]::Zero) after $([int]((Get-Date) - $t0).TotalMilliseconds)ms"
  Start-Sleep -Milliseconds 1500
  Shot $ob 'onboarding-1'
  Key $ob 0x27; Start-Sleep -Milliseconds 300; Shot $ob 'onboarding-2'
  Key $ob 0x27; Start-Sleep -Milliseconds 300; Shot $ob 'onboarding-3'
  Key $ob 0x1B; Start-Sleep -Milliseconds 400   # Esc = skip
  "after skip: onboarding open=$([S]::Find([uint32]$dm.Id, 'DianmoAppWindow', '欢迎') -ne [IntPtr]::Zero) ini: $(Ini 'onboarded')"

  # 2. `dianmo.exe --settings` again: forwarded to the running test instance.
  $p2 = Start-Process $exe -ArgumentList @('--instance', 'Test', '--no-elevate', '--settings') -PassThru; $null = $p2.WaitForExit(5000)
  $sw = WaitWin '设置'
  "settings window: $($sw -ne [IntPtr]::Zero); second process exit $($p2.ExitCode)"
  Start-Sleep -Milliseconds 1500   # admin task / voice engine checks come back
  "client height: $([int](ClientH $sw)) DIP, dpi $([S]::GetDpiForWindow($sw))"
  Shot $sw 'general'
  # Input mode cards: 语音球, then back to 键盘.
  TapDip $sw 596 199.7; Start-Sleep -Milliseconds 700
  "mode after 语音球: $(Ini 'input_mode')"
  $scr = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bmp = New-Object System.Drawing.Bitmap 700, 500; $by = [int]($scr.Height * 0.62) - 250
  [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen(0, $by, 0, 0, $bmp.Size); $bmp.Save('C:\Users\wecode\claude\set-ball.png'); $bmp.Dispose()
  TapDip $sw 376 199.7
  "mode after 键盘: $(Ini 'input_mode')"
  # Theme: scroll to the end, 深色.
  Key $sw 0x23; $y = (ClientH $sw) - 258.5
  TapDip $sw 867 $y; Start-Sleep -Milliseconds 300
  "theme after 深色: $(Ini 'theme')"
  Key $sw 0x24; Start-Sleep -Milliseconds 200; Shot $sw 'general-dark'
  TapDip $sw 116 249; Start-Sleep -Milliseconds 200; Shot $sw 'voice-dark'
  TapDip $sw 116 99; Key $sw 0x23
  TapDip $sw 807 $y
  "theme after 浅色: $(Ini 'theme')"
  # Keyboard height slider (120 %).
  TapDip $sw 116 149; TapDip $sw 735 142.5
  "height: $(Ini 'height')"
  Shot $sw 'keyboard'
  TapDip $sw 116 249; Start-Sleep -Milliseconds 200; Shot $sw 'voice'
  TapDip $sw 116 366; Start-Sleep -Milliseconds 200; Shot $sw 'about'
  # Esc closes the window.
  Key $sw 0x1B; Start-Sleep -Milliseconds 300
  "settings open after Esc: $([S]::Find([uint32]$dm.Id, 'DianmoAppWindow', '设置') -ne [IntPtr]::Zero)"
} finally {
  # Close everything of the test instance (WM_CLOSE on its keyboard window = quit).
  $kh = [S]::Find([uint32]$dm.Id, 'DianmoKeyboard', '')
  if ($kh -ne [IntPtr]::Zero) { [S]::PostMessage($kh, 0x10, [IntPtr]0, [IntPtr]0) | Out-Null }
  if (-not $dm.WaitForExit(5000)) { "test instance did not exit; killing"; Stop-Process -Id $dm.Id -Force }
  "exit code $($dm.ExitCode); total $([int]((Get-Date) - $t0).TotalSeconds)s"
  "log:"; Get-Content $log -Encoding UTF8 | Select-Object -Skip $log0 | ? { $_ -match 'settings|onboarding|theme|input mode|hint|panic|fail|error' } | Select-Object -First 60
}
