# dianmo-win 平台层 · 状态（2026-10-06，M2 第三轮）

## 已完成并在 Surface 实测（Win10 LTSC 21H2，2880x1920，200%）
- **键盘窗口**：WS_POPUP + NOACTIVATE|TOPMOST|TOOLWINDOW|NOREDIRECTIONBITMAP；MA_/PA_NOACTIVATE；SW_SHOWNOACTIVATE；Per-Monitor-V2（代码设置，无 manifest）；底部停靠全宽，高度 = `view.preferred_height`（上限为显示器高度 × `max_height_fraction`）。
- **输入**：WM_POINTER 真多点触控（DIP 坐标、pointer id）；EnableMouseInPointer 让鼠标也能用；关闭触摸反馈和长按右键；取消/抢占的触点转成 `Cancel`。
- **渲染**：D3D11 → 用于合成的 flip 交换链 → ID2D1DeviceContext → DirectComposition；DirectWrite 文本，缓存文本格式和测宽结果；按需重绘（WM_PAINT 合并多次重绘请求）；设备丢失后自动重建。
- **默认用 WARP（软件渲染）**：硬件驱动（Intel igc64 等）要多占约 47MB 私有内存（56MB 对 9.8MB），而一次整屏重绘的 CPU 两者一样（约 6–7ms）。`HostOptions::hardware_gpu` 或环境变量 `DIANMO_D3D=hardware` 可改回 GPU。
- **计时器**：同一时间只保留一个单次计时器，新请求覆盖旧请求；Response 合并（repaint / timer / 嵌套 actions 交回 App）。
- **SendInputSink**：文字用 KEYEVENTF_UNICODE，代理对拆成两个事件，整串一次 SendInput；EditKey 发真实虚拟键，方向键、Home、End、Delete 加 EXTENDEDKEY。**Win+H**：一次 SendInput。
- **托盘**：图标优先用 exe 的图标资源 1（`LoadImageW`，按当前 DPI 的小图标尺寸，DPI 变了重载）；没有资源（如 demo）时用 GDI 画的「墨」。点一下切换显示/隐藏；菜单有显示/隐藏、AppBar 开关、退出。v0.2.0 加了新版本红点（`set_tray_badge`，见下面 v0.2.0 一节，未实测）。
- **悬浮球**（取代原来的边缘把手，见下面「第三轮」）：键盘隐藏时出现，点一下呼出键盘。
- **AppBar**：只在显示时注册；隐藏、退出、panic 时 ABM_REMOVE；收到 TaskbarCreated 重新注册；显示器变化重新布局；可开关。
- **系统触摸键盘设置**：`tabtip` 模块读写 EnableDesktopModeAutoInvoke / TouchKeyboardTapInvoke，可快照并原样恢复（未在测试里改注册表）。
- 实测数据（`tests/surface/e2e.ps1`，向经典记事本注入真实触摸）：
  - 依次点 你、好、⌫、好、⏎，两指重叠按 你+好，点候选「你好」，鼠标点「，」→ 记事本文本为 `你好\n你好你好，`，全程记事本保持前台；两指重叠时同时按下 2 个触点。
  - 私有内存 9.6MB，工作集 29MB；空闲 10 秒 CPU 增量 0ms；从启动进程到窗口可见 236ms；exe 361KB，只依赖系统 DLL。
  - AppBar：最大化的记事本底边从 1853 缩到 1333（键盘顶边 1320）；隐藏键盘和退出后工作区恢复为 (0,0)-(2880,1840)。
  - 托盘点两下分别隐藏、显示；边缘把手能呼出；麦克风键调起「正在聆听…」，焦点仍在记事本。
  - 200% 下渲染清晰（截图见下）。

## M2（2026-10-06 第二轮，Surface 实测）
- **焦点监听 / 自动弹出**（`focus.rs`）：后台 MTA 线程上 `AddFocusChangedEventHandler` + CacheRequest（一次取齐进程 id、控件类型、IsEnabled、IsKeyboardFocusable、IsPassword、ValuePattern 是否可用 / 是否只读、TextPattern 是否可用、ClassName、AutomationId、AriaRole、窗口句柄、外框），分类成 `FocusEvent`，经 `HostProxy::post` 发到 UI 线程。线程平时睡在 `GetMessage` 里，**不轮询**。
  - 可编辑判定：Edit 看 ValuePattern 只读；Spinner / ComboBox 看可写 ValuePattern；DataItem（表格单元格）要可写且可聚焦；Document / Group / Custom / Pane 有 TextPattern 时看光标处（第一个选区）的 `IsReadOnly` 文本属性（多一次跨进程调用），没有选区算不可编辑；Win32 多行 Edit 退回到 ValuePattern。忽略点墨自己进程；同一元素、同一事件重复到达时去抖（新的触摸点击不算重复）。
  - 类型：密码（IsPassword）、数字（Spinner / `aria=spinbutton` / Win32 `ES_NUMBER`）、搜索（`aria=searchbox`、AutomationId/类名含 search）、网址（Edge/Chrome 地址栏 `OmniboxViewViews`、资源管理器地址栏 `Address Band Root` 下的 Edit），其余为 Text。网页里 `type=url/email` 的输入框 UIA 不暴露类型，只能是 Text。
  - **「刚有触摸」**两个来源：① 低级鼠标钩子（`WH_MOUSE_LL`）看由触摸合成的鼠标事件的 `dwExtraInfo` 签名（`0xFF515700`），只记时间戳，不在钩子里调 UIA；② Chromium/Edge/Electron 自己处理 `WM_POINTER`，不会合成鼠标事件（钩子看不到），这时用 `GetCursorInfo` 的 `CURSOR_SUPPRESSED`（触摸/笔输入后系统隐藏光标，鼠标一动就恢复）+ `GetLastInputInfo` 距今 ≤ 500ms，并排除点墨自己的 `SendInput`（`sink.rs` 记录最后一次 SendInput 的 tick）。
  - **点已聚焦的输入框**（没有焦点变化）：钩子在触摸抬起时如果落在当前可编辑元素的外框内（且不在点墨自己的窗口上），重发 `Editable { by_touch: true }`。Win32 / 资源管理器 / XAML 有效；Chromium/Electron 不行（见已知问题）。
  - 设 `DIANMO_FOCUS_LOG=<文件>` 记录每个焦点事件的原始属性（不记录名称和内容），排查新应用时用。
- **托盘菜单可扩展**：`TrayItem::{Command, Separator, Submenu}`，`HostOptions::tray_menu` 初始菜单，`HostControl::set_tray_menu` 运行时替换，选中后回调 `App::on_tray_command(id, ..)`。App 的项在内置项（显示/隐藏、AppBar、退出）上面；菜单 id 按序号映射，App 的 `id` 可以是任意 u32。
- **全屏应用**：托盘的隐藏窗口另外注册一个不占空间的 AppBar（只 `ABM_NEW`，不 `SETPOS`），专门接收 `ABN_FULLSCREENAPP`，键盘自己是不是 AppBar 都能收到。全屏应用在前台时：显示中的键盘移到 `HWND_BOTTOM`（失去 TOPMOST，留在全屏窗口后面），边缘把手隐藏；全屏结束恢复。全屏期间 App 再 `show()`（比如点了全屏网页里的输入框）会把键盘重新置顶。`HostControl::fullscreen_app()` 可查询。
- **`App::on_start`**：窗口、托盘、把手建好之后、第一次显示之前调用一次，用来 `start_focus_watcher(host.proxy())`。
- **实测**（`tests/surface/run-focus.sh`，demo 以 `--auto --hidden --tray-menu` 运行，注入真实触摸；关闭系统触摸键盘自动弹出，测完恢复）：

  | 场景 | 结果 |
  |---|---|
  | 记事本启动（非触摸聚焦） | `Editable{Text, by_touch:false}`，不弹出 |
  | 点记事本编辑区（已聚焦，走钩子重发） | `Editable{Text, true}` → 弹出；点 demo 的收起键后再点 → 再次弹出 |
  | 资源管理器搜索框 / 地址栏 / 文件列表 | `Search` / `Url` / `NotEditable{true}` → 收起 |
  | Edge 网页 text / search / number / password | `Text` / `Search` / `Number` / `Password`，都 by_touch=true |
  | Edge url / email / textarea / contenteditable | `Text`（contenteditable 是 Group，靠光标只读属性判定） |
  | Edge readonly 输入框 / 按钮 / 空白处 | `NotEditable{true}` → 收起 |
  | Edge 地址栏 | `Url` |
  | VS Code 编辑器（Electron，`native-edit-context`） | `Editable{Text}` |
  | 任务栏搜索（SearchApp `RichEditBox`） | `Search` |
  | Edge `--start-fullscreen` | 键盘失去 TOPMOST；再 `show()` 恢复置顶；隐藏时把手不出现；退出全屏后把手恢复 |
  | 托盘菜单 | 显示「布局 ▸ / 深色主题 / 关于 / 隐藏键盘 / 让出屏幕空间 / 退出」；点「深色主题」「布局 ▸ 小鹤双拼」「关于」分别回调 id 20 / 11 / 30，勾选状态随 `set_tray_menu` 更新 |
- **开销**：空闲 CPU 仍为 0（启动后 8–10 秒、跑完全部测试后 10 秒，CPU 增量都是 0ms）。同样显示键盘时，开 focus watcher 私有内存 7.2 → 8.4MB，工作集 22 → 31MB（UIAutomationCore 等共享 DLL 页），线程 5 → 15（UIA 自己的线程池），启动多 ~50ms CPU，`start_focus_watcher` 返回约 45–80ms。连续 450 次 Tab 切换焦点（Edge 页面，265 个事件）私有内存 13.19 → 13.30MB，无泄漏；每个焦点事件约 4–5ms CPU。exe 414KB（原 361KB），只依赖系统 DLL。原 e2e（`run.sh`）回归通过：私有内存 9.1MB，空闲 0ms。

## 第三轮（2026-10-06，Surface 实测）
- **托盘菜单打开时键盘消失（已修）**：根因不是 ABN_FULLSCREENAPP（全程没收到，键盘一直 TOPMOST），而是点托盘图标（鼠标右键或触摸长按）会先把焦点交给任务栏 `Shell_TrayWnd`，焦点监听报 `NotEditable`，App 照规则收起键盘、AppBar 移除，记事本就变成全高；菜单关掉后测试脚本点了记事本标题栏，焦点回到输入框又弹出。主程序的测试是用鼠标右键，`CURSOR_SUPPRESSED` 还停在之前的触摸状态，所以还被误判成 `by_touch:true`。修复：
  - 焦点落到 Explorer（任务栏 / 隐藏图标浮窗）且「最近一次按下的位置」（钩子或 Raw Input 记录，1.5 秒内；否则光标位置）在点墨自己的托盘图标上（`Shell_NotifyIconGetRect`），或者图标在隐藏区时按在「显示隐藏的图标」箭头上 → 当作点墨自己的界面，不报事件。
  - 鼠标点击（钩子看到不带触摸签名的按键）比任何触摸都新时，`by_touch` 一律为 false。
  - 实测：鼠标右键托盘图标、触摸长按托盘图标、图标在隐藏区时点箭头再长按浮窗里的图标，键盘都不收起，菜单正常；点任务栏空白处照常收起。
- **启动时第一条事件（已修）**：`AddFocusChangedEventHandler` 注册时 UIA 会立刻补发一条焦点事件（实测常是过期的元素，如 `ImmersiveBackground`），它走普通路径，`by_touch` 按「刚有触摸」算——用户刚点图标启动点墨时就是 true。现在初始快照发出之前到达的事件一律丢弃（日志 `early focus event dropped`），并且启动前发生的输入不算触摸；第一条事件永远是当前焦点、`by_touch:false`。
- **点任务栏没收起（找到一种必现情形并修好）**：焦点已经在任务栏上（例如刚点任务栏收起键盘，再用悬浮球/托盘手动呼出）时再点任务栏，焦点不变，没有任何事件。现在任务栏有焦点时，在任务栏范围内（通知区除外）抬起的触摸会重发 `NotEditable{by_touch:true}`。另外两种可能：主程序的「手动显示后 1.5 秒内不收起」宽限，以及（以前）Chromium 里同一处触摸被钩子和光标判定算成两次之类的时序，现在都有 Raw Input 的触点时间兜底。没有复现到别的情形。
- **Edge / Electron 里点已聚焦的输入框（已做）**：`focus/rawtouch.rs`。消息窗口（HWND_MESSAGE）上 `RegisterRawInputDevices(0x0D/0x04, RIDEV_INPUTSINK)`，只有手指按着时才有 `WM_INPUT`，空闲 CPU 仍为 0。`hid.dll` 在第一次收到报告时才 `LoadLibraryEx`（exe 导入表不变）；用 `HidP_GetCaps/GetValueCaps/GetButtonCaps` 找每个手指的链接集合（Tip Switch、Contact Id、X/Y 逻辑范围，Contact Count 处理混合模式），归一化后映射到触摸屏所在显示器（`GetPointerDevices` + `GetPointerDeviceRects`，按显示方向旋转；Win32 应用里同一触点合成的鼠标事件会校准旋转）。抬起时落在当前焦点输入框（或任务栏、控制台窗口）内就重发事件；钩子和 Raw Input 看到的是同一次触摸时只算一次（按下位置 24px、600ms 内去重，重发 250ms 内去重，焦点事件已经报过这次触摸也不重发）。
  - Surface 上的触摸设备：`\\?\VIRTUAL_DIGITIZER`（**`InjectTouchInput` 的注入触摸走这里**，逻辑范围 = 屏幕像素，所以自动测试能覆盖这条路径）、`Microsoft HID RID\000D_0004`（5 指，0–27388 × 0–18258，应是 IPTS 处理后的真实触摸）、VHF Col05、Surface Touch Screen Device（单点）。真实手指走哪个设备、旋转校准是否需要，没法在远程测试里验证（需要有人真正用手指点一下，`DIANMO_FOCUS_LOG` 里会有 `raw down/up` 行）。
  - 实测（注入触摸）：Edge 里点 #txt 弹出 → 收起键 → 再点 #txt：`retap(raw) Editable` → 弹出；再点已经显示时不重复发。
  - 笔（0x0D/0x02）没注册，Chromium 里用笔点已聚焦的框仍不会重发。
- **控制台（已修）**：conhost（`ConsoleWindowClass`，PowerShell/cmd）和 Windows Terminal（`CASCADIA_HOSTING_WINDOW_CLASS`）的焦点元素（窗口本身，或前台控制台窗口里的 Text Area）一律算 `Editable{Text}`，输入框范围是整个窗口的客户区，所以新开的控制台第一次点里面（焦点没变）也会弹出。实测：新开 cmd 窗口点里面 → `retap(raw) Editable{Text,true}` → 弹出；收起后再点 → 再弹出。
- **悬浮球**（DESIGN.md 悬浮球，TODO #30；`handle.rs`，取代左边缘长条把手）：
  - 直径 48 DIP，蓝 #3B82F6 → 紫 → 品红 #EC4899 的对角渐变圆 + 左上青色辉光 + 上半部白色光泽 + 图标里的白色书法点（同一条贝塞尔路径）+ 柔和投影。CPU 渲染进预乘 BGRA 的 DIB，`UpdateLayeredWindow(ULW_ALPHA)` 显示：圆边按像素覆盖率抗锯齿，书法点 4×4 超采样，透明的角不接收点击。只在 DPI 变化时重画（约十几毫秒）。
  - 拖动跟手，松手吸附到较近的左/右边缘，`BallEvent::Moved(BallPos{edge, y_frac})` 交给 App 保存，`HostOptions::ball_pos` 启动时传回（默认左边缘、62% 高度）。闲置 3 秒缩成 32 DIP 的半透明小球（alpha 150；v0.2.1 起不再半隐到屏幕外，外沿离边缘 10 DIP，几何在 `ball_geom.rs`）；按下时先恢复不透明，松手后出来（按下时不能挪窗口，否则会丢触摸捕获）。长按 0.55 秒发 `LongPress`。不抢焦点（NOACTIVATE + MA/PA_NOACTIVATE）。
  - `BallState::Listening` 画呼吸光圈（1.6 秒一呼吸，约 30fps 的计时器只在这个状态、球可见时运行）；`Idle` 时没有任何计时器（3 秒的收边计时器是一次性的）。
  - 窗口类名改为 `DianmoBall`（测试脚本已改）。
  - 实测：Tap / LongPress / Moved(Left, 0.217) 都收到；光圈期间 3 秒 CPU 31ms；闲置 5 秒 CPU 0ms；截图放大看边缘平滑、无锯齿。
- **测试脚本**：`touch.ps1` 新增 `[T]::FindOf(class, pid)`，`e2e.ps1`、`focus.ps1` 只找 demo 进程自己的窗口——Surface 上装好的点墨在运行时，旧脚本的 `FindWindow('DianmoKeyboard')` 会找到它（本轮因此误把 WM_CLOSE 发给了用户的点墨，已重新启动）。
- **回归**：`run.sh`（e2e）通过：文本 `你好\n你好你好，`，私有内存 9.1MB，空闲 10 秒 CPU 0ms；`run-focus.sh notepad,explorer,edge,fullscreen,tray` 结果同第二轮（Edge #ro/#btn 那次被用户正在运行的点墨键盘挡住，没点到）。开焦点监听 + Raw Input 后闲置 8 秒 CPU 0ms，私有内存 3.8MB（键盘未显示）。

## 收起后释放渲染资源（2026-10-06，主程序集成那轮）
- 键盘隐藏 5 秒后（`TIMER_TRIM` = 2，单次，`TRIM_AFTER_MS`）`Renderer::release()` 丢掉 `Device`（D3D/WARP 设备、交换链、D2D 上下文、DComp 目标）；显示时 `KillTimer` + `InvalidateRect`，第一次绘制时照常 `Device::new`。实测私有内存 34.7 → 15.5MB，再显示多约 50ms CPU，画面正常。主程序的 `App::on_ball` / `set_ball_state` 已接上（见 dianmo 状态）。

## 应用窗口（2026-10-06，TODO #33 / PRODUCT P3–P5，Surface 实测）
给设置、关于、首次引导用的普通窗口（`window.rs`）。
- **外观**：
  - 普通顶层窗口，可激活，有任务栏按钮；标题栏和任务栏用 exe 图标资源 1（随 DPI 重新加载）。
  - 系统标题栏，`WS_EX_NOREDIRECTIONBITMAP` + DirectComposition。第一帧画好再显示，不会闪白。
  - 深色标题栏用 `DWMWA_USE_IMMERSIVE_DARK_MODE`：`WindowOptions::dark = None` 时跟随系统「应用模式」，收到 `ImmersiveColorSet` 就更新；也可以强制指定，或运行时调 `set_window_dark`。Win10 上属性改了以后，标题栏要等下一次重画才变色，所以改完会切一下 `WM_NCACTIVATE` 强制重画（这一步没有实测）。v0.2.0 的改法见下面「v0.2.0：托盘红点、深色标题栏」。
  - Win11 可以开 Mica（`DWMWA_SYSTEMBACKDROP_TYPE`，只作用于标题栏；Win10 上忽略）。
- **位置和大小**：打开在最后一次指针所在的显示器上，在工作区里居中。工作区不含点墨键盘占用的 AppBar；窗口比工作区大时会缩到工作区大小。Per-Monitor-V2：`WM_DPICHANGED` 按系统建议的矩形移动；`min_width/min_height` 通过 `WM_GETMINMAXINFO` 生效。
- **输入**：
  - `WM_POINTER`（触摸、笔，以及 `EnableMouseInPointer` 之后的鼠标）→ `View::pointer`。
  - 鼠标、笔悬停 → `View::hover`，离开窗口时传 (-1,-1)。
  - `WM_POINTERWHEEL`/`WM_MOUSEWHEEL` → `View::wheel`，换算成 DIP：每格 = 系统滚动行数 × 20 DIP，默认 60；设成「一次一屏」时为窗口高度的 90%。
  - 按键 → `View::key(vk, down)`，按键随后仍交给 DefWindowProc，所以 Alt+F4 照常可用。
  - 关闭了长按右键和对应的视觉反馈，保留了触点反馈。触摸滚动的惯性由 View 自己做。
- **运行方式**：按需重绘；每个窗口同一时间只有一个单次计时器；和键盘共用一个线程、一个消息循环。View 返回的 `UiAction` 交给 `App::on_window_action`。关窗（标题栏 ×、Alt+F4、`close_window`）时先丢弃 View 和渲染资源，再调 `App::on_window_closed`。
- **GPU 设备**：`canvas.rs` 现在有一个可共享的 `Gpu`（D3D11、D2D 设备、DComp 设备）。键盘用共享的那份；应用窗口各用自己的一份，原因见下面的内存数据。丢弃渲染资源时会先断开 DComp 树并 Commit，再 `ClearResources`、`ClearState`、`Flush`，确保交换链的内存真正释放。
- **`Canvas::image(name, rect)`**：
  - 查找顺序：exe 的 RCDATA 资源（资源名就是 name，大小写不敏感）→ exe 旁边的 `res\<name>.png` → `HostOptions::image_dir`。
  - 用 WIC 解码成 PBGRA，转成 D2D 位图，按窗口缓存；找不到也记下来，不重复查找。
  - 绘制时保持宽高比、居中放进 rect，插值用 `HIGH_QUALITY_CUBIC`。
  - 第一次用到图片时，UI 线程会执行 `CoInitializeEx(STA)`；如果已经初始化过（不论哪种模式）就沿用。
- **实测**（`tests/surface/run-window.sh`，Surface 200%，注入触摸）：
  - 在不抢焦点的键盘条上点「打开窗口」：窗口能拿到前台（`foreground=True`），DPI 192，客户区 1520×1066 px = 760×533 DIP（用户自己的点墨键盘占着 AppBar，所以高度被缩到工作区大小）。
  - 手指快速上划 600px：拖动结束时 offset=275，惯性滚到 458–528 后停下。
  - 点一行能切换开关（`tap Row(9)`）；慢速拖动不触发惯性。
  - 鼠标悬停有高亮；滚轮 ±120 → `d=±60.0`；方向键和 PgDn 都能收到。
  - 点「深色」后 `DWMWA_USE_IMMERSIVE_DARK_MODE` 读回 1。但截图里标题栏还是浅色，所以加了上面的 `WM_NCACTIVATE` 重画，加完没有再实测。
  - 点「关闭」→ `view dropped` → `closed`。键盘条开、关 3 次，最后一次用 WM_CLOSE（相当于点 ×），都能正常关掉。
  - 窗口开着空闲 5 秒，CPU 0ms；关掉后空闲 3 秒，CPU 0ms。
  - 截图里文字清晰；app-icon（RCDATA）和 demo-strip（`image_dir`）两张图都按比例缩放显示正常。
  - **内存**（私有字节，同一配置多次运行之间波动很大，±20MB）：
    - 只开键盘条：5.5MB。
    - 窗口共用设备：开着 24–30MB，关掉后 24–25MB，基本不回落，WARP 把释放的内存留在池里了。
    - 窗口用自己的设备：开着 30–31MB，关掉后 20–21MB，所以现在默认用这种。
    - 关窗后再隐藏键盘（键盘的设备也释放）：8.5–15MB。剩下的是 DirectWrite、WIC 和堆里留着的内存。
    - 同一配置的另外几次运行里，窗口刚打开就有 43–52MB，原因没查清，可能是 WARP 的 JIT 或线程。
    - 开、关多次后内存没有持续增长，看不出泄漏。
- 演示里用的 PNG 由 `crates/dianmo/res/icon/*.svg` 用 cairosvg 生成，放在 `examples/res/`。`.gitignore` 加了这个目录的例外（全局忽略 `*.png`）。`build.rs` 用 llvm-windres 把 `examples/res/examples.rc`（图标资源 1 + `app-icon` RCDATA）链接进各个 example，所以 `demo.exe` 现在也有图标了。

## v0.2.0：托盘红点、深色标题栏（2026-10-06，TODO #38，只在服务器上 cargo check / clippy，未实测）
- **托盘新版本红点**：`HostControl::set_tray_badge(on)`、`HostProxy::set_tray_badge(on) -> bool`（任意线程，比如检查更新的线程）。
  - 打开时，用当前托盘图标（exe 图标资源 1，或 demo 里画的「墨」）在内存里合成一个新 HICON：`GetIconInfo` + `GetDIBits` 取出图标自己的像素（没有 alpha 的老式图标按 AND 掩码补 alpha），右上角贴边画红点（#E81123，外径约为图标边长的 36%，最小 6px），外面一圈白描边（图标每 16px 宽 1px：100% 时 1px，200% 时 2px），4×4 超采样抗锯齿，按非预乘 alpha 叠加，再 `CreateIconIndirect`（32 位 + 按 alpha 生成的掩码），然后 `NIM_MODIFY`。
  - 关闭时 `NIM_MODIFY` 换回原图标，再销毁合成的图标（shell 有自己的副本）。重复设置同一状态什么都不做；合成失败就保持原图标。
  - 状态保存在 `Tray` 里：DPI 变了按新尺寸重新加载原图标并重新合成（各 DPI 都是按实际像素画的，不是缩放）；Explorer 重启（TaskbarCreated）后 `NIM_ADD` 用的就是当前显示的图标。没有托盘图标（`HostOptions::tray = false`）时无效果。
- **深色标题栏**（`window.rs`）：
  - 用 `RtlGetVersion` 取系统版本号（`GetVersionEx` 在没有兼容性清单时会谎报）。19041（20H1）及以后先用属性 20，1809–1909 先用 19，失败再试另一个；1809 之前不设。
  - Win10 1903 及以后（< 22000）另外调一次 user32 的 `SetWindowCompositionAttribute(WCA_USEDARKMODECOLORS)`（未公开；Explorer、winit 用的就是这个），保证窗口框架自己记录的主题和 DWM 属性一致。
  - 主题在 `CreateWindowExW` 之后立刻设置（Mica 也一样），早于渲染器创建和第一次 `ShowWindow`。
  - 运行中切换（`set_window_dark`、系统主题变化）：先 `SetWindowPos(SWP_FRAMECHANGED | NOMOVE | NOSIZE | NOZORDER | NOACTIVATE)` 让框架重算重画；Win10 上再对 `DefWindowProc` 发一对 `WM_NCACTIVATE`（先反、再正，相当于失活再激活，但焦点和前台窗口都不变），让 DWM 用新颜色重画标题栏。Win11 自己会重画，只做第一步。
  - 记录每个窗口当前的标题栏主题，`ImmersiveColorSet` 连发几次、或设成同一个值时不再重复重画。
- **要实机看的点**（主会话合并到一次验证）：
  1. 主程序有新版本时托盘图标右上角出现红点：100% 和 200% 下都清晰、有白边、不糊；关掉后恢复原图标，没有残影。
  2. 红点亮着时改缩放（或把键盘拖到别的 DPI 的显示器）、重启 Explorer（`taskkill /f /im explorer.exe` 后再开），红点还在。
  3. 系统为浅色时用 `--dark` 打开窗口（`window_demo.exe --open --dark`），一出现标题栏就是深色。
  4. 窗口开着、处于前台时点「深色」/「浅色」，标题栏立即变色，窗口不闪、焦点不丢。窗口在后台时切换也要变色。
  5. 系统「应用模式」切换时，跟随系统的窗口标题栏跟着变。
  - 如果第 4 点还是不变色，下一步可以试：隐藏再显示窗口（`SW_HIDE` + `SW_SHOWNA`），或者把窗口宽度临时 +1px 再改回去。这两种都能确定触发重画，但会有闪烁或额外的交换链缩放，所以这次没有用。

## 未完成 / 已知问题
- 没测：运行中改 DPI、多显示器、旋转屏幕、Explorer 重启（这些代码路径都已写好）。
- 进程被强杀（TerminateProcess）时 AppBar 占的工作区不会释放，要等下一次有 AppBar 变化（比如再开一次点墨）才恢复。测试时请用 WM_CLOSE 关闭（e2e 脚本就是这样做的）。
- 每帧都整窗重绘（约 6–7ms CPU）。以后可以只重绘变化区域（见下面的接口需求）。
- Raw Input 用真实手指还没验证过（见第三轮）；旋转屏幕时的坐标映射靠显示方向 + 自动校准，也没实测。
- ~~悬浮球贴边半隐时，从屏幕最边上开始拖可能触发系统的边缘手势~~：v0.2.1 已改（球离边缘 10 DIP，收起时缩小而不是半隐）。
- `CURSOR_SUPPRESSED` 判定会把「触摸之后 500ms 内的实体键盘按键」也算作触摸（比如触摸后马上按 Esc 关掉搜索，焦点回到别处时 `by_touch=true`）。纯触屏用户没有实体键盘，影响很小；点墨自己 SendInput 发出的键已排除。
- 监听焦点会让 Chromium 打开无障碍树（和屏幕阅读器一样），Edge 自身内存会多一些，没量。
- 托盘图标现在被系统放进了「隐藏的图标」里（Shell_NotifyIconGetRect 拿不到位置），所以 e2e 里「点托盘图标」那一步被跳过；`run-focus.sh` 改为直接给托盘窗口发图标回调消息来打开菜单。
- 托盘菜单里调用 SetForegroundWindow 会激活隐藏的托盘窗口（这是 Windows 的已知要求）；这时用户本来就已经离开了目标应用。
- 测试时 demo 由提权的计划任务启动，所以 UIPI 限制（普通权限的键盘向管理员窗口输入）没有覆盖到。
- 应用窗口：
  - 深色标题栏运行中切换的重画（v0.2.0 改过，见上）、Mica（Win11）、运行中改 DPI、多显示器，都没有实测。
  - 关窗后内存只回落约 10MB，见上面的数据。
  - 应用窗口没有接 `WM_CHAR`，不能直接输入文字，以后要做搜索框得补上。
  - 在 Surface 上跑测试时，注入的触摸如果落在用户焦点所在的控制台里，正在运行的点墨会把它当成「点了输入框」，然后弹出自己的键盘（另一个进程，不算 bug）。这时 demo 的键盘条会被挤上去，所以测试脚本每次点之前都会重新读取位置。

## 公开 API
- `run(view: Box<dyn View>, app: Box<dyn App>) -> windows::core::Result<()>`；`run_with(view, app, HostOptions)`；`enable_per_monitor_dpi()`
- `HostOptions { start_visible, appbar, tray, edge_handle /*悬浮球开关*/, tray_tip, max_height_fraction, hardware_gpu, tray_menu: Vec<TrayItem>, ball_pos: Option<BallPos>, image_dir: Option<PathBuf> }`（实现了 Default）
- `trait App { on_start(..) /*默认空*/; on_action(&mut self, UiAction, &mut dyn View, &mut HostControl) -> Response; on_event(Box<dyn Any+Send>, ..) -> Response /*默认空*/; on_visibility_changed(bool, ..) /*默认空*/; on_tray_command(id: u32, ..) /*默认空*/; on_ball(BallEvent, ..) -> Response /*默认 Tap/LongPress → show()*/ }`
- `HostControl`：`show / hide / toggle / is_visible / set_appbar / appbar_enabled / quit / proxy / set_tray_menu(Vec<TrayItem>) / set_tray_badge(bool) / fullscreen_app() / set_ball_state(BallState)`（请求在回调返回后才生效）
- 应用窗口：
  - `HostControl::open_window(Box<dyn View>, WindowOptions) -> WindowId`：id 立即可用，窗口在回调返回后创建。
  - `close_window(id)`：在同一个回调里刚打开的窗口直接取消，不回调 `on_window_closed`。
  - `window_open(id) -> bool`、`windows() -> &[WindowId]`、`focus_window(id)`（最小化时先还原）、`set_window_title(id, ..)`、`set_window_dark(id, Option<bool>)`。
  - `update_keyboard(FnOnce(&mut dyn View) -> Response)`、`update_window(id, FnOnce(..))`：回调结束后在对应的 View 上执行，返回的 Response 按那个 View 的规则处理。窗口回调里要改键盘（比如换主题），或者托盘里要改已打开的设置页，都用这两个。
- `WindowOptions { title, width, height, min_width, min_height /*客户区，DIP*/, resizable, icon /*exe 图标资源 1*/, dark: Option<bool> /*None=跟随系统*/, mica }`，实现了 Default（880×620，最小 480×360，可调整大小，有图标）。`WindowId` 实现了 Copy、Eq、Hash、Ord。`system_dark_mode() -> bool`。
- `App` 新增两个方法，都有默认实现：
  - `on_window_action(id, UiAction, view /*该窗口的 View*/, host) -> Response`
  - `on_window_closed(id)`：用户关窗、`close_window` 或者窗口创建失败时都会调用，调用时 View 已经丢弃。
- dianmo-ui 新增的方法，都有默认实现：
  - `View::wheel(x, y, delta_y /*DIP，向下为正*/)`、`View::key(vk, down)`、`View::hover(x, y)`，只有应用窗口会调用。
  - `Canvas::image(name, rect)`。
- 悬浮球：`BallEvent::{Tap, LongPress, Moved(BallPos)}`、`BallPos { edge: BallEdge, y_frac: f32 }`（实现了 Default）、`BallEdge::{Left, Right}`、`BallState::{Idle, Listening}`（在 `handle.rs`；需要 `lib.rs` 加 `pub use handle::{BallEdge, BallEvent, BallPos, BallState};`）
- `TrayItem::{Command { id: u32, label: String, checked: bool }, Separator, Submenu { label: String, items: Vec<TrayItem> }}`
- 2026-10-06 新增（主程序接设置窗口用）：
  - `HostOptions::tray_builtins`（默认 true）：false 时托盘菜单只有 app 的项（app 自己放「显示/隐藏键盘」「退出」，用 `toggle()` / `quit()`）。
  - `HostControl::reveal_ball(ms)`：球（键盘收起时）完全露出并保持 `ms` 毫秒再收边（取代主程序重置内部计时器 id 2 的做法）；在同一回调的显示 / 隐藏之后执行。
  - `HostControl::set_ball_pos(BallPos)`（设置里「靠左 / 靠右」）、`set_ball_enabled(bool)`（运行中创建 / 销毁悬浮球，位置保留）。
  - `App::on_system_theme_changed(dark, view, host) -> Response`（默认空）：键盘窗口收到 `WM_SETTINGCHANGE "ImmersiveColorSet"` 时调用（Windows 一次切换会发好几次，app 自己去重）。
- `focus::{start_focus_watcher(HostProxy) -> Result<FocusWatcher>, FocusWatcher /*Drop 即停止*/, FocusEvent::{Editable { kind, by_touch }, NotEditable { by_touch }}, FieldKind::{Text, Number, Password, Url, Search}, TOUCH_WINDOW_MS}`；第一条事件永远是当前焦点（by_touch=false）。App 在 `on_event` 里 `event.downcast::<FocusEvent>()`。
- `HostProxy`（Send + Sync + Copy）：`post<T: Any+Send>(T)`（交给 `App::on_event`）、`show / hide / toggle / quit / set_tray_badge(bool) / hwnd`
- `SendInputSink`（实现 `TextSink`）、`send_text`、`send_edit_key`、`start_voice_typing`、`now_ms`
- `tabtip::{read_dword, write_dword, SystemKeyboardSettings::{read, apply, auto_invoke_enabled}, disable_system_keyboard_auto_invoke}`

## 运行
- 构建：`scripts/surface/build.sh win build --release -p dianmo-win --example demo`
- 应用窗口演示：`scripts/surface/build.sh appwin build --release -p dianmo-win --example window_demo`，参数 `window_demo.exe [--open] [--dark]`。测试脚本 `crates/dianmo-win/tests/surface/run-window.sh`，约 30 秒，会在用户屏幕上弹出窗口，**用户在用 Surface 时不要跑**。
- demo 参数：`demo.exe [--no-appbar] [--hidden] [--no-tray] [--no-handle] [--focus] [--auto] [--tray-menu] [--voice-ball] [--ball-right]`（`--voice-ball`：点球切换 Listening 光圈，长按呼出键盘；`--ball-right`：球从右边缘 85% 高度开始）（`--focus` 启动焦点监听并记录事件；`--auto` 再按 by_touch 自动显示/收起；`--tray-menu` 加自定义托盘菜单）；设置 `DIANMO_DEMO_LOG=<文件>` 会记录触点、焦点事件、托盘命令和显示状态，`DIANMO_FOCUS_LOG=<文件>` 记录焦点事件的原始 UIA 属性。
- 端到端测试：`crates/dianmo-win/tests/surface/run.sh`。脚本会启动记事本和 demo，注入触摸并截图，最后用 WM_CLOSE 关掉 demo、强杀记事本（不保存）。
- 焦点 / 托盘 / 全屏测试：`crates/dianmo-win/tests/surface/run-focus.sh [notepad,explorer,edge,vscode,search,fullscreen,tray]`（默认全部，约 80 秒）。会打开记事本、资源管理器窗口、独立配置目录的 Edge 和 VS Code、任务栏搜索，测完全部关掉并删掉临时配置目录；期间把 `EnableDesktopModeAutoInvoke` 设为 0，结束时恢复原值。
- 服务器上的快速检查：`flock /tmp/heavy.lock nice -n 10 cargo check -p dianmo-win --examples --target x86_64-pc-windows-gnullvm`

## 下一步
1. 主程序 `dianmo` 用 `run_with` 把 dianmo-ui 的 View 和 InputController + SendInputSink 接起来（`UiAction::Input` → `handle` → `view.set_input_state`）。
2. 启动时调用 `disable_system_keyboard_auto_invoke()` 并保存快照，退出时 `apply()` 恢复。
3. 主程序在 `on_start` 里 `start_focus_watcher(host.proxy())`，`on_event` 里按规则显示/收起：`Editable{by_touch:true}` → `show()`（可按 `kind` 切数字/英文布局），`NotEditable{by_touch:true}` → `hide()`；by_touch=false 的事件不改变显示状态（用户自己用托盘/把手呼出时不被收起）。
4. 按键气泡和动画用 DirectComposition（已经接好）。主程序接 `on_ball` / `set_ball_state` 做语音球，保存 `Moved` 的位置。

## 接口变更需求（dianmo-ui，都是新增，不破坏现有代码）
- `canvas::Font` 加上 `#[derive(Hash)]`（目前在 canvas.rs 里自己映射，不急）。
- 可选：`Response` 加 `dirty: Option<Rect>`，实现局部重绘，把每次按键的 CPU 从约 13ms 降下来。
- 可选：按键气泡需要画到键盘窗口上方，要么由宿主提供一个 overlay（DComp 视觉 + 透明弹窗），要么在 View 契约里加一个「浮层」绘制入口。等 UI 那边定好再说。
