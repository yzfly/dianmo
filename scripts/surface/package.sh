#!/bin/bash
# Build and package 点墨 on the Surface; optionally install it for the user.
#
#   scripts/surface/package.sh <name> [--install] [--start] [--keep-target] [--no-build]
#   scripts/surface/package.sh <name> --dist-only --install [--start]   (install the existing dist as-is)
#   scripts/surface/package.sh <name> --uninstall
#   scripts/surface/package.sh <name> --stop        (only close a running 点墨)
#   scripts/surface/package.sh <name> --setup [--keep-target]   (also build DianmoSetup-<ver>.exe)
#
# 1. release build of dianmo.exe in C:\dev\dianmo-<name> (scripts/surface/build.sh, queued, low priority)
# 2. checks the exe only imports system DLLs (llvm-objdump)
# 3. assembles C:\dev\dianmo-dist\<name>\Dianmo\ (outside the work dir: sync.sh wipes it):
#    dianmo.exe + (scripts\rime\stage.ps1 -Out ...)
#    rime.dll and data\rime\ with the precompiled build\. Without stage.ps1 the package still runs,
#    with the built-in letters-only engine.
# --install    closes a running 点墨 with WM_CLOSE (never killed: that would leave the AppBar's
#              screen space reserved), copies dist to %LOCALAPPDATA%\Dianmo, creates 「点墨」 shortcuts
#              on the desktop and in the Start menu, and registers the scheduled task `Dianmo`
#              (`dianmo.exe --register-task`: run with highest privileges, so 点墨 can type into
#              administrator windows; TODO #23). The shortcuts point at dianmo.exe, which hands
#              over to the task. Autostart = the task's logon trigger (an old HKCU Run entry is
#              migrated).
# --start      starts the installed 点墨 in the user's desktop session afterwards (unelevated,
#              like the shortcut: it hands over to the task).
# --uninstall  closes 点墨, removes shortcuts, the task, the old Run entry and %LOCALAPPDATA%\Dianmo
#              (settings/log in %APPDATA%\Dianmo are kept).
# --setup      also builds the installer: crates/dianmo-setup (stub dianmo-setup.exe + packer
#              dianmo-pack.exe), packs the dist into C:\dev\dianmo-dist\<name>\DianmoSetup-<ver>.exe
#              (<ver> = workspace version) and copies it back to dist/ in the repo (git-ignored).
# Finally removes the build's target\ (TODO #10) unless --keep-target.
set -euo pipefail
name=${1:?usage: package.sh <name> [--install] [--start] [--keep-target] [--no-build] | --uninstall}; shift
install=0; start=0; keep=0; build=1; uninstall=0; stop=0; assemble=1; setup=0
for a in "$@"; do
  case $a in
    --install) install=1 ;;
    --start) start=1 ;;
    --keep-target) keep=1 ;;
    --no-build) build=0 ;;
    --dist-only) build=0; assemble=0; keep=1 ;;
    --uninstall) uninstall=1 ;;
    --stop) stop=1 ;;
    --setup) setup=1 ;;
    *) echo "unknown option $a" >&2; exit 2 ;;
  esac
done
here=$(cd "$(dirname "$0")" && pwd)
dir="C:\\dev\\dianmo-$name"
dist="C:\\dev\\dianmo-dist\\$name\\Dianmo"

# Closes a running 点墨 gracefully. Runs in the desktop session (window messages don't cross
# sessions). Prints "closed", "not running" or "STILL RUNNING".
close_ps='
Add-Type -Namespace DM -Name W -MemberDefinition @"
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern System.IntPtr FindWindow(string c, string n);
[DllImport("user32.dll")] public static extern bool PostMessage(System.IntPtr h, uint m, System.IntPtr w, System.IntPtr l);
"@
$procs = @(Get-Process dianmo -ErrorAction SilentlyContinue)
if ($procs.Count -eq 0) { "not running" } else {
  $h = [DM.W]::FindWindow("DianmoKeyboard", [NullString]::Value)
  if ($h -ne [IntPtr]::Zero) { [DM.W]::PostMessage($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
  $deadline = (Get-Date).AddSeconds(8)
  while ((Get-Process dianmo -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
  if (Get-Process dianmo -ErrorAction SilentlyContinue) { "STILL RUNNING" } else { "closed" }
}'

if [ $stop = 1 ]; then
  "$here/gui.sh" 30 <<<"$close_ps"
  exit 0
fi

if [ $uninstall = 1 ]; then
  "$here/gui.sh" 60 <<PS
$close_ps
\$inst = Join-Path \$env:LOCALAPPDATA 'Dianmo'
\$sh = New-Object -ComObject WScript.Shell
foreach (\$d in @(\$sh.SpecialFolders('Desktop'), \$sh.SpecialFolders('Programs'))) { Remove-Item (Join-Path \$d '点墨.lnk') -ErrorAction SilentlyContinue }
Remove-Item (Join-Path \$sh.SpecialFolders('Programs') '点墨设置.lnk') -ErrorAction SilentlyContinue
Remove-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name Dianmo -ErrorAction SilentlyContinue
if (Get-ScheduledTask -TaskName Dianmo -ErrorAction SilentlyContinue) { Unregister-ScheduledTask -TaskName Dianmo -Confirm:\$false; "removed task Dianmo" }
if (-not (Get-Process dianmo -ErrorAction SilentlyContinue)) { Remove-Item \$inst -Recurse -Force -ErrorAction SilentlyContinue; "removed \$inst" }
PS
  exit 0
fi

if [ $build = 1 ]; then
  pkgs="-p dianmo"; [ $setup = 1 ] && pkgs="$pkgs -p dianmo-setup"
  out=$("$here/build.sh" "$name" build --release $pkgs 2>&1 | tee /dev/stderr)
  grep -q 'cargo exit: 0' <<<"$out" || { echo "build failed" >&2; exit 1; }
elif [ $assemble = 1 ]; then
  "$here/sync.sh" "$name"
fi

[ $assemble = 1 ] && "$here/ps.sh" <<PS
\$ErrorActionPreference = 'Stop'
\$env:Path = "C:\dev\tools\llvm-mingw\bin;\$env:Path"
Set-Location '$dir'
\$exe = 'target\x86_64-pc-windows-gnullvm\release\dianmo.exe'
if (-not (Test-Path \$exe)) { \$exe = 'target\release\dianmo.exe' }
if (-not (Test-Path \$exe)) { throw "dianmo.exe not built" }
\$imports = llvm-objdump -p \$exe | Select-String 'DLL Name: (.+)' | % { \$_.Matches[0].Groups[1].Value.ToLower() }
\$system = 'kernel32.dll','user32.dll','gdi32.dll','advapi32.dll','shell32.dll','ole32.dll','oleaut32.dll','d2d1.dll','d3d11.dll','dxgi.dll','dwrite.dll','dcomp.dll','dwmapi.dll','uxtheme.dll','comctl32.dll','shcore.dll','ws2_32.dll','bcrypt.dll','bcryptprimitives.dll','ntdll.dll','userenv.dll','uiautomationcore.dll','imm32.dll','msvcrt.dll','ucrtbase.dll','version.dll','winmm.dll','propsys.dll','combase.dll','synchronization.dll','winhttp.dll'
\$bad = @(\$imports | ? { \$_ -notin \$system -and \$_ -notlike 'api-ms-win-*' })
"imports: \$(\$imports -join ', ')"
if (\$bad.Count) { throw "unexpected DLL imports: \$(\$bad -join ', ')" }
\$dist = '$dist'
if (Test-Path \$dist) { Remove-Item \$dist -Recurse -Force }
New-Item -ItemType Directory -Force \$dist | Out-Null
Copy-Item \$exe \$dist
if (Test-Path 'scripts\rime\stage.ps1') {
  # Rebuilds the precompiled Rime data only if it is stale and a probe.exe is around.
  \$stageArgs = @('-Out', \$dist)
  \$probe = Get-ChildItem 'target' -Recurse -Filter probe.exe -ErrorAction SilentlyContinue | Select -First 1
  if (\$probe) { \$stageArgs += @('-Probe', \$probe.FullName) }
  & powershell -NoProfile -ExecutionPolicy Bypass -File 'scripts\rime\stage.ps1' @stageArgs
  if (\$LASTEXITCODE) { throw "stage.ps1 failed (\$LASTEXITCODE)" }
} else {
  "WARNING: scripts\rime\stage.ps1 not found; packaging without Rime (built-in engine only)"
}
\$files = Get-ChildItem \$dist -Recurse -File
"dist: \$dist  files=\$(\$files.Count)  size=\$([math]::Round((\$files | Measure-Object Length -Sum).Sum/1MB,1))MB  exe=\$([math]::Round((Get-Item "\$dist\dianmo.exe").Length/1KB))KB"
PS

if [ $setup = 1 ]; then
  ver=$(sed -n 's/^version = "\(.*\)"/\1/p' "$here/../../Cargo.toml" | head -1)
  setup_out="C:\\dev\\dianmo-dist\\$name\\DianmoSetup-$ver.exe"
  "$here/ps.sh" <<PS
\$ErrorActionPreference = 'Stop'
\$env:Path = "C:\dev\tools\llvm-mingw\bin;\$env:Path"
Set-Location '$dir'
\$rel = @('target\x86_64-pc-windows-gnullvm\release', 'target\release') | ? { Test-Path "\$_\dianmo-setup.exe" } | Select -First 1
if (-not \$rel) { throw "dianmo-setup.exe not built (package.sh --setup builds it)" }
\$imports = llvm-objdump -p "\$rel\dianmo-setup.exe" | Select-String 'DLL Name: (.+)' | % { \$_.Matches[0].Groups[1].Value.ToLower() }
"setup stub imports: \$(\$imports -join ', ')  size=\$([math]::Round((Get-Item "\$rel\dianmo-setup.exe").Length/1KB))KB"
\$out = '$setup_out'
cmd /c start '""' /belownormal /b /wait "\$rel\dianmo-pack.exe" "\$rel\dianmo-setup.exe" '$dist' \$out --version $ver 2>&1
if (\$LASTEXITCODE) { throw "dianmo-pack failed (\$LASTEXITCODE)" }
PS
  mkdir -p "$here/../../dist"
  scp -q -P 15570 "wecode@127.0.0.1:C:/dev/dianmo-dist/$name/DianmoSetup-$ver.exe" "$here/../../dist/"
  ls -l "$here/../../dist/DianmoSetup-$ver.exe"
fi

if [ $install = 1 ]; then
  "$here/gui.sh" 90 <<PS
$close_ps
if (Get-Process dianmo -ErrorAction SilentlyContinue) { "install aborted: 点墨 did not exit"; return }
\$inst = Join-Path \$env:LOCALAPPDATA 'Dianmo'
robocopy '$dist' \$inst /MIR /NJH /NJS /NFL /NDL /NP | Out-Null
if (\$LASTEXITCODE -ge 8) { "robocopy failed: \$LASTEXITCODE"; return }
\$sh = New-Object -ComObject WScript.Shell
foreach (\$d in @(\$sh.SpecialFolders('Desktop'), \$sh.SpecialFolders('Programs'))) {
  \$lnk = \$sh.CreateShortcut((Join-Path \$d '点墨.lnk'))
  \$lnk.TargetPath = Join-Path \$inst 'dianmo.exe'
  \$lnk.WorkingDirectory = \$inst
  \$lnk.IconLocation = (Join-Path \$inst 'dianmo.exe') + ',0'
  \$lnk.Description = '点墨 · 触屏输入法'
  \$lnk.Save()
}
# Start menu 「点墨设置」: opens the settings window (of the running 点墨, or starts it hidden).
\$lnk = \$sh.CreateShortcut((Join-Path \$sh.SpecialFolders('Programs') '点墨设置.lnk'))
\$lnk.TargetPath = Join-Path \$inst 'dianmo.exe'
\$lnk.Arguments = '--settings'
\$lnk.WorkingDirectory = \$inst
\$lnk.IconLocation = (Join-Path \$inst 'dianmo.exe') + ',0'
\$lnk.Description = '点墨设置'
\$lnk.Save()
"installed to \$inst; shortcuts: desktop + start menu (点墨, 点墨设置)"
# The gui job is elevated, as registering a highest-privileges task requires.
\$p = Start-Process (Join-Path \$inst 'dianmo.exe') -ArgumentList '--register-task' -Wait -PassThru
\$t = Get-ScheduledTask -TaskName Dianmo -ErrorAction SilentlyContinue
if (\$p.ExitCode -ne 0 -or -not \$t) { "WARNING: registering task Dianmo failed (exit \$(\$p.ExitCode)); see %APPDATA%\Dianmo\dianmo.log" }
else { "task Dianmo: \$(\$t.Actions[0].Execute) \$(\$t.Actions[0].Arguments)  runlevel=\$(\$t.Principal.RunLevel)  autostart=\$([bool](\$t.Triggers | ? { \$_.CimClass.CimClassName -eq 'MSFT_TaskLogonTrigger' }))" }
PS
fi

if [ $start = 1 ]; then
  "$here/gui.sh" 30 <<'PS'
$exe = Join-Path $env:LOCALAPPDATA 'Dianmo\dianmo.exe'
# The gui job runs elevated; start 点墨 unelevated (like the shortcut would) through Explorer.
# It hands over to the task Dianmo (elevated) and exits.
Start-Process explorer.exe -ArgumentList "`"$exe`""
Start-Sleep -Seconds 3
$p = @(Get-Process dianmo -ErrorAction SilentlyContinue)
if ($p.Count) { "started: pid $($p.Id -join ', ')" } else { "not running after 3s; see %APPDATA%\Dianmo\dianmo.log" }
Get-Content (Join-Path $env:APPDATA 'Dianmo\dianmo.log') -Tail 4 -Encoding UTF8
PS
fi

if [ $keep = 0 ]; then
  "$here/clean.sh" "$name" --target
fi
