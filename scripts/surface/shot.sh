#!/bin/bash
# Screenshot the Surface's screen into <out.png> (full 2880x1920) and <out>-s.png (1440 wide, for viewing).
#   scripts/surface/shot.sh <out.png>
set -euo pipefail
out=${1:?usage: shot.sh <out.png>}
here=$(cd "$(dirname "$0")" && pwd)
"$here/gui.sh" 30 <<'PS' >/dev/null
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type 'using System.Runtime.InteropServices; public class DpiAware { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }'
[DpiAware]::SetProcessDPIAware() | Out-Null
$b = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
[System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
$bmp.Save('C:\Users\wecode\claude\screen.png', [System.Drawing.Imaging.ImageFormat]::Png)
PS
scp -q -P 15570 wecode@127.0.0.1:C:/Users/wecode/claude/screen.png "$out"
uv run -q --with pillow python3 -c "import sys; from PIL import Image; im=Image.open(sys.argv[1]); im.thumbnail((1440,1440)); im.save(sys.argv[2])" "$out" "${out%.png}-s.png"
echo "${out%.png}-s.png"
