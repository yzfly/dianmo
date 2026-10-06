Add-Type -TypeDefinition @'
using System; using System.Text; using System.Threading; using System.Runtime.InteropServices;
public static class T {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int x, y; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINTER_INFO {
    public int pointerType; public uint pointerId; public uint frameId; public int pointerFlags;
    public IntPtr sourceDevice; public IntPtr hwndTarget; public POINT ptPixelLocation; public POINT ptHimetricLocation;
    public POINT ptPixelLocationRaw; public POINT ptHimetricLocationRaw; public uint dwTime; public uint historyCount;
    public int InputData; public uint dwKeyStates; public ulong PerformanceCount; public int ButtonChangeType; }
  [StructLayout(LayoutKind.Sequential)] public struct POINTER_TOUCH_INFO { public POINTER_INFO pointerInfo; public int touchFlags; public int touchMask; public RECT rcContact; public RECT rcContactRaw; public uint orientation; public uint pressure; }
  [DllImport("user32.dll", SetLastError=true)] public static extern bool InitializeTouchInjection(uint max, uint mode);
  [DllImport("user32.dll", SetLastError=true)] public static extern bool InjectTouchInput(uint count, [In] POINTER_TOUCH_INFO[] contacts);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr a, string c, string n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  // The top-level window of class `cls` that belongs to process `pid` (an installed 点墨 may be
  // running too, with the same window classes: never touch its windows).
  public static IntPtr FindOf(string cls, int pid) { IntPtr h = IntPtr.Zero; while ((h = FindWindowEx(IntPtr.Zero, h, cls, null)) != IntPtr.Zero) { uint p; GetWindowThreadProcessId(h, out p); if (p == (uint)pid) return h; } return IntPtr.Zero; }
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, StringBuilder l);
  [DllImport("user32.dll")] public static extern bool SystemParametersInfo(uint a, uint p, out RECT r, uint f);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [StructLayout(LayoutKind.Sequential)] public struct NOTIFYICONIDENTIFIER { public uint cbSize; public IntPtr hWnd; public uint uID; public Guid guidItem; }
  [DllImport("shell32.dll")] public static extern int Shell_NotifyIconGetRect(ref NOTIFYICONIDENTIFIER id, out RECT r);
  public static RECT TrayRect(IntPtr h) { var id = new NOTIFYICONIDENTIFIER(); id.cbSize = (uint)Marshal.SizeOf(id); id.hWnd = h; id.uID = 1; RECT r; int hr = Shell_NotifyIconGetRect(ref id, out r); if (hr != 0) r = new RECT(); return r; }
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, IntPtr e);
  public static void Click(int x, int y) { SetCursorPos(x, y); Thread.Sleep(30); mouse_event(2, 0, 0, 0, IntPtr.Zero); Thread.Sleep(60); mouse_event(4, 0, 0, 0, IntPtr.Zero); }
  public static string Text(IntPtr h) { var sb = new StringBuilder(4096); SendMessage(h, 0x000D, (IntPtr)4096, sb); return sb.ToString(); }
  public static RECT Work() { RECT r; SystemParametersInfo(0x30, 0, out r, 0); return r; }
  const int INRANGE=0x2, INCONTACT=0x4, DOWN=0x10000, UPDATE=0x20000, UP=0x40000;
  static POINTER_TOUCH_INFO Make(uint id, int x, int y, int flags) {
    var c = new POINTER_TOUCH_INFO(); c.pointerInfo.pointerType = 2; c.pointerInfo.pointerId = id;
    c.pointerInfo.ptPixelLocation = new POINT{x=x,y=y}; c.pointerInfo.pointerFlags = flags;
    c.touchMask = 0x7; c.rcContact = new RECT{left=x-6, top=y-6, right=x+6, bottom=y+6}; c.orientation = 90; c.pressure = 32000; return c; }
  public static bool Tap(int x, int y) {
    if (!InjectTouchInput(1, new[]{Make(0,x,y, DOWN|INRANGE|INCONTACT)})) return false; Thread.Sleep(50);
    if (!InjectTouchInput(1, new[]{Make(0,x,y, UPDATE|INRANGE|INCONTACT)})) return false; Thread.Sleep(30);
    return InjectTouchInput(1, new[]{Make(0,x,y, UP)}); }
  // Two contacts overlapping in time: A down, B down, both held, A up, B up.
  public static bool Two(int x1, int y1, int x2, int y2) {
    if (!InjectTouchInput(1, new[]{Make(0,x1,y1, DOWN|INRANGE|INCONTACT)})) return false; Thread.Sleep(40);
    if (!InjectTouchInput(2, new[]{Make(0,x1,y1, UPDATE|INRANGE|INCONTACT), Make(1,x2,y2, DOWN|INRANGE|INCONTACT)})) return false; Thread.Sleep(60);
    if (!InjectTouchInput(2, new[]{Make(0,x1,y1, UP), Make(1,x2,y2, UPDATE|INRANGE|INCONTACT)})) return false; Thread.Sleep(40);
    return InjectTouchInput(1, new[]{Make(1,x2,y2, UP)}); }
}
'@
[T]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
[T]::InitializeTouchInjection(10, 3) | Out-Null
function Fmt($r) { "($($r.left),$($r.top))-($($r.right),$($r.bottom))" }
