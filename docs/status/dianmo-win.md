# dianmo-win 平台层 · 状态（2026-10-06）

## 已完成并在 Surface 实测（Win10 LTSC 21H2，2880x1920，200%）
- **键盘窗口**：WS_POPUP + NOACTIVATE|TOPMOST|TOOLWINDOW|NOREDIRECTIONBITMAP；MA_/PA_NOACTIVATE；SW_SHOWNOACTIVATE；Per-Monitor-V2（代码设置，无 manifest）；底部停靠全宽，高度 = `view.preferred_height`（上限为显示器高度 × `max_height_fraction`）。
- **输入**：WM_POINTER 真多点触控（DIP 坐标、pointer id）；EnableMouseInPointer 让鼠标也能用；关闭触摸反馈和长按右键；取消/抢占的触点转成 `Cancel`。
- **渲染**：D3D11 → 用于合成的 flip 交换链 → ID2D1DeviceContext → DirectComposition；DirectWrite 文本，缓存文本格式和测宽结果；按需重绘（WM_PAINT 合并多次重绘请求）；设备丢失后自动重建。
- **默认用 WARP（软件渲染）**：硬件驱动（Intel igc64 等）要多占约 47MB 私有内存（56MB 对 9.8MB），而一次整屏重绘的 CPU 两者一样（约 6–7ms）。`HostOptions::hardware_gpu` 或环境变量 `DIANMO_D3D=hardware` 可改回 GPU。
- **计时器**：同一时间只保留一个单次计时器，新请求覆盖旧请求；Response 合并（repaint / timer / 嵌套 actions 交回 App）。
- **SendInputSink**：文字用 KEYEVENTF_UNICODE，代理对拆成两个事件，整串一次 SendInput；EditKey 发真实虚拟键，方向键、Home、End、Delete 加 EXTENDEDKEY。**Win+H**：一次 SendInput。
- **托盘**：GDI 画的「墨」图标；点一下切换显示/隐藏；菜单有显示/隐藏、AppBar 开关、退出。**边缘把手**：键盘隐藏时左边缘出现小把手，点一下呼出键盘。
- **AppBar**：只在显示时注册；隐藏、退出、panic 时 ABM_REMOVE；收到 TaskbarCreated 重新注册；显示器变化重新布局；可开关。
- **系统触摸键盘设置**：`tabtip` 模块读写 EnableDesktopModeAutoInvoke / TouchKeyboardTapInvoke，可快照并原样恢复（未在测试里改注册表）。
- 实测数据（`tests/surface/e2e.ps1`，向经典记事本注入真实触摸）：
  - 依次点 你、好、⌫、好、⏎，两指重叠按 你+好，点候选「你好」，鼠标点「，」→ 记事本文本为 `你好\n你好你好，`，全程记事本保持前台；两指重叠时同时按下 2 个触点。
  - 私有内存 9.6MB，工作集 29MB；空闲 10 秒 CPU 增量 0ms；从启动进程到窗口可见 236ms；exe 361KB，只依赖系统 DLL。
  - AppBar：最大化的记事本底边从 1853 缩到 1333（键盘顶边 1320）；隐藏键盘和退出后工作区恢复为 (0,0)-(2880,1840)。
  - 托盘点两下分别隐藏、显示；边缘把手能呼出；麦克风键调起「正在聆听…」，焦点仍在记事本。
  - 200% 下渲染清晰（截图见下）。

## 未完成 / 已知问题
- 没测：运行中改 DPI、多显示器、旋转屏幕、Explorer 重启（这些代码路径都已写好）。
- 进程被强杀（TerminateProcess）时 AppBar 占的工作区不会释放，要等下一次有 AppBar 变化（比如再开一次点墨）才恢复。测试时请用 WM_CLOSE 关闭（e2e 脚本就是这样做的）。
- 每帧都整窗重绘（约 6–7ms CPU）。以后可以只重绘变化区域（见下面的接口需求）。
- ABN_FULLSCREENAPP 没处理：全屏应用打开时键盘仍然置顶。
- 托盘菜单里调用 SetForegroundWindow 会激活隐藏的托盘窗口（这是 Windows 的已知要求）；这时用户本来就已经离开了目标应用。
- 测试时 demo 由提权的计划任务启动，所以 UIPI 限制（普通权限的键盘向管理员窗口输入）没有覆盖到。

## 公开 API
- `run(view: Box<dyn View>, app: Box<dyn App>) -> windows::core::Result<()>`；`run_with(view, app, HostOptions)`；`enable_per_monitor_dpi()`
- `HostOptions { start_visible, appbar, tray, edge_handle, tray_tip, max_height_fraction, hardware_gpu }`（实现了 Default）
- `trait App { on_action(&mut self, UiAction, &mut dyn View, &mut HostControl) -> Response; on_event(Box<dyn Any+Send>, ..) -> Response /*有默认实现*/; on_visibility_changed(bool, ..) -> Response /*有默认实现*/ }`
- `HostControl`：`show / hide / toggle / is_visible / set_appbar / appbar_enabled / quit / proxy`（请求在回调返回后才生效）
- `HostProxy`（Send + Sync + Copy）：`post<T: Any+Send>(T)`（交给 `App::on_event`）、`show / hide / toggle / quit / hwnd`
- `SendInputSink`（实现 `TextSink`）、`send_text`、`send_edit_key`、`start_voice_typing`、`now_ms`
- `tabtip::{read_dword, write_dword, SystemKeyboardSettings::{read, apply, auto_invoke_enabled}, disable_system_keyboard_auto_invoke}`

## 运行
- 构建：`scripts/surface/build.sh win build --release -p dianmo-win --example demo`
- demo 参数：`demo.exe [--no-appbar] [--hidden] [--no-tray] [--no-handle]`；设置 `DIANMO_DEMO_LOG=<文件>` 会记录每个触点事件。
- 端到端测试：`crates/dianmo-win/tests/surface/run.sh`。脚本会启动记事本和 demo，注入触摸并截图，最后用 WM_CLOSE 关掉 demo、强杀记事本（不保存）。
- 服务器上的快速检查：`flock /tmp/heavy.lock nice -n 10 cargo check -p dianmo-win --examples --target x86_64-pc-windows-gnullvm`

## 下一步
1. 主程序 `dianmo` 用 `run_with` 把 dianmo-ui 的 View 和 InputController + SendInputSink 接起来（`UiAction::Input` → `handle` → `view.set_input_state`）。
2. 启动时调用 `disable_system_keyboard_auto_invoke()` 并保存快照，退出时 `apply()` 恢复。
3. M2：在 MTA 线程上用 UIA 监听焦点，通过 `HostProxy::post` 发到 UI 线程；按键气泡和动画用 DirectComposition（已经接好）。

## 接口变更需求（dianmo-ui，都是新增，不破坏现有代码）
- `canvas::Font` 加上 `#[derive(Hash)]`（目前在 canvas.rs 里自己映射，不急）。
- 可选：`Response` 加 `dirty: Option<Rect>`，实现局部重绘，把每次按键的 CPU 从约 13ms 降下来。
- 可选：按键气泡需要画到键盘窗口上方，要么由宿主提供一个 overlay（DComp 视觉 + 透明弹窗），要么在 View 契约里加一个「浮层」绘制入口。等 UI 那边定好再说。
