# dianmo 主程序与打包 · 状态（2026-10-06）

## 已完成并在 Surface 实测（Win10 LTSC 21H2，2880x1920，200%）
- **主程序 `crates/dianmo`**（`dianmo.exe`，windows 子系统，621KB，只依赖系统 DLL，嵌了「墨」图标和版本信息）：
  - `dianmo_win::run_with(KeyboardView, DianmoApp, HostOptions)`；`DianmoApp` 里是 `InputController<AnyEngine, SendInputSink>`。
  - 事件循环：`UiAction::Input` → `controller.handle` → `view.set_input_state`（返回的 Response 交给宿主继续处理）；`WantMoreCandidates` → `engine.candidates` → `set_more_candidates`；`WantT9Spellings` → `set_t9_spellings`；`Voice` → Win+H；`Hide` → `host.hide()`；`ThemeChanged` → 存设置。启动时 `SetSchema(设置里的方案)`，中英也按设置恢复。
  - **引擎**：窗口先用内置的「字母原样上屏」引擎起来（候选注释写「词库加载中」），`on_start` 里开后台线程（先等 250ms，免得 rime.dll 的加载锁拖慢窗口的 D2D 初始化）启动 `RimeEngine`，好了以后 post 回 UI 线程换上（正在组字时等组完再换；换时方案不一致会补一次 SetSchema）。`rime.dll` 不存在或启动失败就一直用内置引擎，候选注释和托盘菜单「关于」里写「词库未加载（原因）」。退出时先关 session 再 `dianmo_rime::shutdown()`。
  - **系统触摸键盘**：启动时把原值写进 `settings.ini`（`saved_*`）**之后**才把 `EnableDesktopModeAutoInvoke`、`TouchKeyboardTapInvoke` 设为 0；正常退出 / WM_ENDSESSION（App 的 Drop）/ panic hook 都会恢复；异常退出后下次启动发现 `saved_*` 还在，就把它当原值。实测：运行中 0/0，退出后恢复为原来的 1/（不存在）。
  - **单实例**：命名 mutex `Local\Dianmo.SingleInstance`；第二次启动找到消息窗口 `DianmoInstance`（HWND_MESSAGE）发消息，已有实例 `HostProxy::show()`，自己退出（host 还没就绪时记下，就绪后再显示）。普通权限进程打不开提权实例的 mutex（ACCESS_DENIED）也算「已在运行」；提权实例用 `ChangeWindowMessageFilterEx` 放行这条消息，所以普通权限的再次启动仍能呼出键盘。
  - **管理员窗口 / 提权运行**（TODO #23，`elevate.rs`）：安装时注册计划任务 `Dianmo`（当前用户、Interactive、最高权限、不限时、电池上也运行、按需运行、已在运行时不新开、优先级 4 = 普通；动作 `dianmo.exe --task`）。`dianmo.exe` 以普通权限启动（快捷方式、双击、旧的 Run 项）时，如果任务存在且指向本 exe，就经任务计划程序 COM（`ITaskService` → `GetTask` → `Run`）启动任务、等提权实例的消息窗口出现后让它显示键盘（`--hidden` 时不显示），然后自己退出；任务不存在 / 指向别的 exe / 启动失败 / 10 秒内没起来，就以普通权限继续运行，只记日志。`--task` 和 `--no-elevate` 不再转发，防止循环。提权实例另外用 `ChangeWindowMessageFilter` 放行 Explorer 发来的 `TaskbarCreated` 和托盘 / AppBar 回调消息（`WM_APP+3`、`WM_APP+4`，与 dianmo-win/host.rs 对应，那边改编号要同步）。
  - **设置** `%APPDATA%\Dianmo\settings.ini`（key=value，临时文件 + rename 写入）：schema、theme、chinese、appbar、autostart、auto_show、height，外加 `saved_*`。开机自启：任务 `Dianmo` 指向本 exe 时，就是给任务加 / 删「登录时」触发器（当前用户；托盘开关读写它，需要提权实例）；启动时发现还有旧的 `HKCU\...\Run\Dianmo`，就转成登录触发器并删掉 Run 项（没提权转不了时保留 Run 项，它启动 exe 后照样转发到任务）。没有任务时仍用 Run 项（exe 换位置后启动时自动改过来）。
  - **托盘菜单**（App 项在宿主自带的「显示/隐藏、让出屏幕空间、退出」上面）：全拼 / 小鹤双拼 / 九宫格、深色主题、键盘高度 ▸（矮 0.85 / 标准 / 高 1.15 / 更高 1.3）、点输入框时自动弹出、开机自动启动、关于（版本 + 词库状态）。改高度会隐藏再显示一次以重新停靠。
  - **自动弹出/收起**（`start_focus_watcher`）：`Editable{by_touch:true}` → 显示；`Number` 输入框自动切到数字面板，离开后切回字母；`NotEditable{by_touch:true}` → 350ms 后收起（期间有新的焦点事件或手动显示/隐藏就取消）；`by_touch:false` 不弹不收。手动呼出（托盘、把手、再次启动）后 1.5s 内不因「非输入框」收起（点托盘图标本身会把焦点给任务栏）。键盘隐藏时清掉未上屏的组字。
  - 日志 `%APPDATA%\Dianmo\dianmo.log`（追加，>512KB 时轮换为 `.old`），从不弹对话框。
  - 参数：`--hidden` / `--autostart`（隐藏启动）、`--task`（任务启动：隐藏、不再转发）、`--no-elevate`、`--register-task` / `--unregister-task`（需提权；注册时保留已有的登录触发器，有旧 Run 项就转成触发器）、`--instance <名字>`（测试用：独立的 mutex、消息窗口类、数据目录 `%APPDATA%\Dianmo-<名字>`（含 Rime 用户目录）、任务名 `Dianmo<名字>`、Run 项名）、`--deploy <shared_dir>`（调 `dianmo_rime::deploy`，单独进程）、`--version`。环境变量 `DIANMO_ENGINE=basic` 强制内置引擎；`DIANMO_KEYMAP=<文件>` 每次显示时写出按键中心坐标（测试用）。
  - feature `rime`（默认开）；`--no-default-features` 可不带 dianmo-rime 编译。
- **实测数据**（`crates/dianmo/tests/surface/run.sh`，向经典记事本注入真实触摸）：
  - 从启动进程到键盘可见 171–264ms；librime 在启动后约 0.45–0.7s 就绪（start 本身 176–451ms）。
  - 私有内存：带 rime 24–28MB（工作集约 60MB）；不带 rime 15.6MB。空闲 10 秒 CPU 增量 0ms。
  - 全拼 `nihao`+空格 → 你好；`women`+2（选第 2 个候选）→ 我们；小鹤 `nihc` → 你好；九宫格 `64426` 出「你好」，左列 ni/mi/m/n，选 mi 后锁定。全程记事本保持前台。
  - 单实例、隐藏键再启动呼出、WM_CLOSE 退出后工作区和系统键盘设置都恢复。
  - 自动弹出：点记事本输入区 → 弹出；点任务栏 → 收起。
  - 托盘：切深色/浅色、开机自启（写入 Run）实测生效。
  - **提权（2026-10-06，`package.sh elevate` 的 dist + `--instance Test`，任务 `DianmoTest`）**：`--register-task` 生成的任务：Highest / Interactive / 优先级 4 / PT0S / IgnoreNew / 电池上可运行。用 RunLevel Limited 的临时任务启动 exe → 67–83ms 内转发完成并退出，提权实例（elevated=True、优先级 Normal）从开始到键盘可见 250–310ms；旧 Run 项启动时被转成登录触发器（用户 `DESKTOP-…\wecode`）并删除。点隐藏键后再次以普通权限启动 → 键盘出来（UIPI 放行生效）。在管理员 PowerShell 和管理员记事本之间来回触摸点击 3 轮，每次键盘都弹出、前台是被点的窗口；在管理员 PowerShell 的 `Read-Host` 里点 `n i h a o 空格 回车` → 收到「你好」。没有任务时以普通权限运行（日志写明原因），启动不受影响。
  - 高度 1.3：键盘高 869px（标准 668px）。

## 宽屏布局 / 电脑按键接入（TODO #19 / #22 / #26，2026-10-06）
- `Action::Key(KeyChord)` 由 controller 先上屏组字、再调 `SendInputSink::send_chord`（dianmo-win `sink.rs`）：一次 `SendInput` 发 左Ctrl/左Shift/左Alt/左Win 按下 → 键 → 倒序抬起；字母数字用 VK 码、标点用美式 OEM 码，不经过输入法；单按 Win 发 LWin 按下抬起。另有 `Action::KeyDown/KeyUp(KeyCode)` → `SendInputSink::key_event` / `dianmo_win::send_key_event`（单个键按下或抬起，带扫描码），给 #31 直通布局用（已接上，见下一节）。
- 设置 `edit_area`（默认 true）：宽屏右侧编辑区；托盘菜单「横屏显示编辑区（复制、粘贴…）」切换并保存；启动时在 `on_start` 里应用（main.rs 不用改）。
- 高度：宽屏五行由 `KeyboardView::preferred_height` 决定（1440 宽标准 353 DIP），高度档位仍生效，键高不低于 52 DIP；宿主的 `max_height_fraction` 0.5 不变。
- `DIANMO_KEYMAP` 测试钩子多导出 ctrl alt win fn caps esc tab del 方向键 home end undo redo selectall copy paste cut delword clear pc symbols numbers。
- **实机状态**（GUI 回归已在下一节「电脑键盘、剪贴板历史」里用 pcclip 包跑过）：已在 Surface 上跑过 `dianmo-win` 的 sink 单元测试（含组合键事件构造，3 个通过），并 `package.sh layout` 打包到 `C:\dev\dianmo-dist\layout\Dianmo`（848KB exe，导入表只有系统 DLL）。**GUI 实测还没做**：21:28–22:00 期间用户自己的点墨（`%LOCALAPPDATA%\Dianmo`）一直在运行，单实例会冲突，按约定没有启动。待用户关掉后运行：
  `crates/dianmo/tests/surface/run.sh 'C:\dev\dianmo-dist\layout\Dianmo' 'n i h a o space 2 0 2 6 TEXT SHOT selectall copy right paste TEXT ctrl z TEXT SHOT'`
  预期：`你好2026` → 全选/复制/→/粘贴后 `你好2026你好2026` → Ctrl+Z 后 `你好2026`；截图看候选条和编辑区。还要确认 MDL2 新图标（撤销 E7A7、重做 E7A6、全选 E8B3、剪切 E8C6、复制 E8C8、粘贴 E77F、左右 E76B/E76C）在雅黑/MDL2 下显示正确。
- e2e（`crates/dianmo/tests/surface/e2e.ps1`）：启动 dist 的 exe 时带 `--no-elevate`（不交给已安装点墨的计划任务）；新步骤 `TEXT` 打印记事本当前内容。

## 电脑键盘、剪贴板历史（TODO #31 / #32，2026-10-06）
- **电脑键盘**：`UiAction::PcKeyboard(bool)` → 设置 `pc_keyboard`（启动时 `on_start` 里 `set_pc_keyboard` 恢复）；托盘菜单布局组加「电脑键盘（按键直通）」，选全拼/小鹤/九宫格会退出电脑键盘。`Action::KeyDown/KeyUp` 经 controller → `SendInputSink::key_event`（dianmo-win 的 sink.rs 这轮没改）。
- **剪贴板**（`src/clipboard.rs`，main.rs 只加了 `mod clipboard;`）：`ClipStore`（纯逻辑，Linux 上有测试）+ Windows 监听线程（message-only 窗口 `DianmoClipboard`、`AddClipboardFormatListener`），`ClipEvent::{Text, NoText, Private, PasteFailed}` 经 `HostProxy::post` 回到 UI 线程。App：新文字 → `ClipStore::add` → `set_clips` / `set_paste_preview`，键盘显示着就 `notify_copied`；密码框焦点期间（`FocusEvent::Editable { kind: Password }`，要开着自动弹出）不记录；固定的条目写 `clips.txt`（和 settings.ini 同目录，`--instance` 自动分开）。`UiAction::Paste`：不超过 2000 字且不含换行就 `Action::Text`（Unicode SendInput），否则 `ClipboardWatcher::paste`（监听线程里：保存全部内存块格式 → 写文字 → Ctrl+V → 600ms → 恢复，期间序号区间内的剪贴板变化不记录）。`CheckCopied` → 350ms 后（一次性线程）比较剪贴板更新计数，没变就 `enter_select_mode`。键盘隐藏时 `reset_transient`。
- `crates/dianmo/Cargo.toml` 的 windows features 加了 `Win32_System_DataExchange`、`Win32_System_Memory`。
- `DIANMO_KEYMAP` 测试钩子：除了显示时，每处理一个动作 / 事件后也重写一次（布局切换后坐标会变），并导出新键名。注意：键盘内部的面板切换（布局菜单、触控板结束）不经过宿主，不会触发重写。
- **e2e 改动**（`tests/surface/e2e.ps1`）：测试副本一律 `--instance Test --no-elevate`，按进程 id 找键盘窗口，只在 `$RunDir` 的 dianmo 已运行时中止（用户自己的点墨可以开着）；每次点键前重读 keymap 和窗口位置；新步骤 `CLIP`、`PAD:<dx>`、`PADSEL:<dx1>:<dx2>`（注入长按空格 + 拖动 + 第二指点按）、`SET:k=v`（启动前写测试实例的 settings.ini）；开始前备份剪贴板文字，结束后恢复。日志在 `%APPDATA%\Dianmo-Test\dianmo.log`。
- **Surface 实测**（`package.sh pcclip` → `C:\dev\dianmo-dist\pcclip\Dianmo`，931KB exe，导入表只有系统 DLL；用户的点墨同时在运行，没有动它）：
  - `run.sh 'C:\dev\dianmo-dist\pcclip\Dianmo' 'SET:pc_keyboard=true n i h a o SHOT space TEXT 1 2 3 TEXT ctrl a ctrl c right ctrl v TEXT CLIP pcback selectall copy WAIT300 SHOT right clip0 TEXT copy WAIT700 SHOT sel_done PADSEL:-28:-70 left SHOT sel_delete TEXT CLIP'`
  - 电脑键盘直通：记事本的输入法是微信输入法（新开记事本默认就是它，测试没有切换输入法），点 `n i h a o` 后记事本里出现微信输入法的组字 `ni'ha'o` 和候选窗（你哈哦 / 你哈偶 / 你哈 / 你 …），空格上屏「你哈哦」（微信输入法自己这样切分，不是点墨的问题），`1 2 3` → `你哈哦123`。Ctrl（单次）+A、Ctrl+C、→、Ctrl+V → `你哈哦123你哈哦123`，剪贴板是 `你哈哦123`。
  - 返回全拼后：全选 + 复制 → 键区中间「已复制」、候选条变成剪贴板卡片栏（两张卡片），粘贴键预览「你哈哦123…」；→ 后点第一张卡片 → 打出 `你哈哦123你哈哦123`（4 份）。
  - 没有选中时点「复制」→ 350ms 后自动进入选择模式（截图里是选择栏，编辑区「选择」键高亮）。
  - 触控板：长按空格拖动 → 光标左移；第二指点一下后再拖 → 记事本里选中了「你哈哦1」（截图：键区变成「选择中」触控板，手指位置有圆点）；松手后选择栏出现，再按 ← 扩一格、点「删除」→ 文字少了选中的部分。
  - 空闲 10 秒 CPU 增量 0ms；私有内存 33–42MB（上一轮 24–28MB；这轮还包含别的 agent 的悬浮球窗口改动，没单独拆分）。WM_CLOSE 退出码 0，系统键盘设置和工作区恢复，剪贴板文字恢复为测试前的内容。
  - 上一轮留下的回归：`n i h a o space 2 0 2 6 … selectall copy right paste … ctrl z` → `你好2026`、`你好2026你好2026` 都对；但 Ctrl+Z 之后记事本是空的，不是预期的 `你好2026`：经典记事本（Edit 控件）只有一级撤销，连续插入（打字 + 粘贴，中间光标没真正移动）被当成一次，所以一次撤销全没了。Ctrl+Z 本身发到了，不是点墨的问题，预期值写错了。
  - 测试期间用户自己的点墨（旧版本、自动弹出开着）也被测试的触摸带出来，停靠在我们的测试键盘下面；按约定没有去关它，测完它还显示着。

## 打包与安装
```
scripts/surface/package.sh <name> [--install] [--start] [--keep-target] [--no-build]
scripts/surface/package.sh <name> --stop        # 只用 WM_CLOSE 关掉运行中的点墨
scripts/surface/package.sh <name> --uninstall   # 关掉、删快捷方式/开机自启/%LOCALAPPDATA%\Dianmo（保留设置和日志）
```
1. `build.sh <name> build --release -p dianmo`（同步 + 低优先级构建）；
2. `llvm-objdump -p` 检查导入表只含系统 DLL；
3. 组装到 `C:\dev\dianmo-dist\<name>\Dianmo\`（不放在工作目录里：sync 会清空工作目录）：`dianmo.exe` + `scripts\rime\stage.ps1 -Out`（rime.dll + data\rime，含预编译 build；target 里有 probe.exe 时带 `-Probe`，build 过期会重建）。共 46 个文件、48.7MB；
4. `--install`：在桌面会话里用 WM_CLOSE 关掉正在运行的点墨（等最多 8 秒，关不掉就放弃安装，不强杀），robocopy /MIR 到 `%LOCALAPPDATA%\Dianmo`，桌面和开始菜单建「点墨」快捷方式（指向 dianmo.exe，由它转发到任务），然后（gui 任务本身是提权的）`dianmo.exe --register-task` 注册 / 更新计划任务 `Dianmo`；`--uninstall` 会删掉任务和旧 Run 项；
5. `--start`：经 explorer 以普通权限启动安装好的点墨（它会转去启动任务 `Dianmo`，以提权实例运行）；
6. 默认最后 `clean.sh <name> --target` 清掉构建产物（TODO #10），`--keep-target` 跳过。

2026-10-06 已经执行过 `package.sh app --install --start`：安装在 `C:\Users\wecode\AppData\Local\Dianmo`，桌面 / 开始菜单有「点墨」；验证后已用 WM_CLOSE 关闭（没有留在后台运行），开机自启是关的。

## 测试
- 服务器：`flock /tmp/heavy.lock nice -n 10 cargo test -p dianmo`（设置解析、日志时间戳、内置引擎，5 个）；`cargo check -p dianmo --target x86_64-pc-windows-gnullvm`（也试 `--no-default-features`）。
- Surface：`scripts/surface/build.sh app build --release -p dianmo`，把 exe 复制到 `C:\dev\dianmo-app\target\run\` 并 `stage.ps1 -Out` 同目录，然后 `crates/dianmo/tests/surface/run.sh [run_dir] '<步骤>'`。步骤可以是按键名（keymap 里的）、`SHOT`、`WAIT<ms>`、`VIS`、`@x,y`、`TRAYMENU`、`TRAYPICK<n>`、`RUNKEY`，见 `e2e.ps1` 开头。脚本最后会检查单实例、用 WM_CLOSE 退出、打印系统键盘设置和工作区是否恢复、本次日志。

## 已知问题 / 遗留
- 托盘图标在新路径的 exe 上常被 Windows 放进溢出区，`TRAYPICK`/`TRAYMENU` 测试会找不到图标（属于测试环境问题）；托盘菜单「关闭开机自启」这条路径没在实机上点到（代码与开启对称，写注册表的 RegDeleteKeyValueW）。
- 用 `TRAYMENU` 截图时，托盘菜单打开期间截到的画面里键盘不在、记事本是全高（菜单关掉后键盘和工作区都正常）。可能是托盘菜单把隐藏的托盘窗口设为前台时触发了 ABN_FULLSCREENAPP，或截图时机问题，请 dianmo-win 看一下。
- 焦点监听启动时第一条事件实测是 `NotEditable { by_touch: true }`（DIANMO_FOCUS_LOG 第一行，早于 "started"），与文档说的 by_touch:false 不一致；我们靠「手动显示后 1.5s 宽限」挡住了。另有一次「点任务栏没收起」没能复现，可能是 by_touch 判定偶尔超出 500ms 窗口。
- 数字输入框切数字面板、密码 / 网址框没有特殊处理（没有切英文），都还没在实机上测。
- 隐藏启动（`--autostart`）时 librime 也会在后台启动，占 24MB 左右；以后可以改成第一次显示时再启动。
- 提权（TODO #23）没实测的：托盘菜单在提权实例里的右键（图标在溢出区，测不到；已用 `ChangeWindowMessageFilter` 放行回调消息）、托盘「开机自动启动」关掉（走 `set_logon_trigger(false)`，与打开同一段代码）、真正注销再登录时登录触发器的启动。计划任务启动的进程拿的是注册表里的用户环境变量，不继承启动者的（`DIANMO_FOCUS_LOG` 之类要设在 `HKCU\Environment`）。
- 控制台窗口（conhost）获得焦点时先后报 `Editable(Text Area)` 和 `NotEditable(Console Window)`，顺序不固定；新开的控制台第一次点击里面（焦点没变）不会弹键盘，靠的是「触摸抬起在上次可编辑元素里重发」，而上次是 `Console Window`。点墨提权后在管理员控制台里来回切换是好的，这一点属于 dianmo-win 焦点分类，普通控制台同样存在。
- `scripts/surface/package.sh` 的导入表白名单是手写的，dianmo-win 以后新增系统 DLL 依赖时要加进去。

## 语音模块（TODO #27 / #29，2026-10-06，`src/voice.rs`；已接入主程序，见下一节）
按 `docs/research/voice.md` 实现：点墨模拟所选引擎的语音热键，看麦克风占用判断是否真的在收音，不行就退回 Win+H。

**接口**（Windows-only，一个实例放在 UI 线程）：
```rust
pub enum VoiceEngine { WeType, DoubaoVoice, System }   // as_setting()/from_setting(): wetype|doubao|system；label() 菜单文字
pub enum VoiceState { Idle, Starting, Listening, Finishing, Failed(String) }  // is_active()
let mut v = Voice::new(engine);
v.set_engine(e);                 // 换引擎（也清掉「连续失败 2 次就跳过」的标记），会取消进行中的会话
v.set_doubao_exe(Option<PathBuf>); v.set_doubao_launch(bool);   // 豆包语音 exe 路径（None=自动找）、没运行时是否自动启动（默认是）
v.set_fallback(bool);            // 失败时是否自动开 Win+H（默认是）
v.set_log(|m| crate::log::write(m));   // 一行诊断，不含识别文字和剪贴板内容
Voice::available(e) / v.unavailable_reason() -> Option<String>
v.toggle() / v.start() / v.stop() / v.cancel(); v.poll(); v.state(); v.needs_poll(); v.last_report()
```
- **WeType（默认）**：一次 SendInput 发 `LCtrl↓ LWin↓ LShift↓ LShift↑ LWin↑ LCtrl↑` 开始/结束；收音判断 = `CapabilityAccessManager\ConsentStore\microphone\NonPackaged\…#wetype_update.exe` 的 `LastUsedTimeStop==0`（且开始时间晚于热键）。可用 = TIP 已注册 + 在用户输入法列表（`HKCU\Control Panel\International\User Profile\*` 里有 `0804:{86598FB9…}`）+ `wetype_server.exe` 在运行；`wetype_update.exe` 没在运行（会崩，见下）时 `start` 先经 explorer 以普通权限把它拉起来，等它的窗口出现再发热键。
- **剪贴板兜底**：`start` 时记 `GetClipboardSequenceNumber` 并保存剪贴板全部内存型格式（≤32MB；CF_BITMAP/EMF 等 GDI 句柄格式不存，但 DIB 会被系统合成后存下）。结束后序号变了且是 WeType 放的（WeType 复制时**没有 owner 窗口**，所以判据是「owner 进程是 wetype* 或 无 owner 且 WeType 的语音/提示窗可见」），就读 CF_UNICODETEXT 用 `dianmo_win::send_text` 打进去，再把原剪贴板全部格式写回（带 `CanIncludeInClipboardHistory=0`，Win+V 历史不再多一条；WeType 自己那条历史删不掉）。别的进程改了剪贴板就不打字、不动它。
- **取消**：WeType 只在确认收音时发 Esc。目标输入法不是 WeType 时，Esc 后 WeType 仍会把结果「已复制」到剪贴板，所以 cancel 后约 2.5s 内 `needs_poll()` 仍为真（`state()` 已是 Idle），期间发现 WeType 的复制就恢复原剪贴板（不打字）。
- **DoubaoVoice**：只读 `%APPDATA%\DouBaoVoice\config.json` 的 `HotKey`/`IsConfigured`（不读 token）；exe 按设置 / 运行中进程 / `%USERPROFILE%\Downloads`、Desktop、`%LOCALAPPDATA%\Programs\DouBaoVoice` 等处最新的 `DouBaoVoice*.exe` 找；没运行时经 explorer 启动（普通权限），窗口出现 0.7s 后发热键。它自己上屏：**用剪贴板 + Ctrl+V，且不恢复剪贴板**，所以点墨同样在 start 时保存剪贴板，看到它改剪贴板 1s 后写回。没有真正的取消，cancel = stop。
- **System**：Win+H，状态不跟踪（直接回 Idle）。
- **失败回退**：引擎不可用，或热键发出 1.5s 内没开始收音（WeType 语音窗已出现则放宽到 3s），就开 Win+H，返回 `Failed("微信输入法没有开始收音，已改用系统语音输入")` 之类的提示；同一引擎连续失败 2 次后直接走 Win+H，直到 `set_engine`。

**集成方法（给主会话）**：
1. `settings.rs` 加 `voice_engine=wetype|doubao|system`（默认 wetype，`VoiceEngine::from_setting`）、可选 `voice_doubao_exe=`。
2. `DianmoApp` 里放一个 `Voice`；`UiAction::Voice`（麦克风键 / 语音球单击）→ 先把 #22 锁住的 Ctrl/Shift 抬起、清掉未上屏的组字 → `voice.toggle()`；返回的状态交给语音球 / 麦克风键显示（`is_active()` 时动画），`Failed(msg)` 显示提示。
3. `voice.needs_poll()` 为真时开 300ms 定时器调 `poll()`，为假就停（空闲 CPU 为 0）。键盘收起、换引擎时 `cancel()`。
4. 托盘「语音引擎 ▸」三项（`VoiceEngine::ALL`、`label()`），可用 `Voice::available(e)` 灰掉或在文字后注明「未安装」，选中后 `set_engine` 并存设置。
5. `voice.set_log(|m| crate::log::write(m))`。

**测试入口**：`crates/dianmo/examples/voice_probe.rs`（`#[path]` 引入 `src/voice.rs`，不依赖 app.rs）。
```
scripts/surface/build.sh voicebuild build --release -p dianmo --no-default-features --example voice_probe
scripts/voice/engine-test.sh <本地截图目录> -Engine wetype|doubao|system [-Medium] [-ImeCycles 1] [-Secs 6] [-AfterMs 2000] [-Cancel]
    [-Speak '<TTS 朗读的句子>' -Keyword <关键字>] [-Extra --no-fallback]
voice_probe.exe --avail | --clip-roundtrip | --engine X --secs N [--cancel] [--no-fallback] [--no-launch]
```
engine-test 开自己的记事本（`-Medium` 用 explorer 以普通权限开），`-ImeCycles` 用 Win+Space 切该窗口输入法（按窗口设置输入法已开，只影响这个窗口），跑 voice_probe，可用 zh-CN TTS（Huihui）朗读一句让麦克风听到；只打印状态、字数、关键字是否出现、剪贴板指纹/是否不变，不打印识别文字和剪贴板内容；剪贴板文字变了会写回；最后强关记事本不保存。

**Surface 实测（2026-10-06）**：
- `--clip-roundtrip`：保存 → 覆盖 → 恢复，指纹一致（4 个格式）。
- WeType，目标输入法 = WeType：热键后 300ms 开始收音，结果由 WeType 直接上屏（13–14 字，含关键字），剪贴板没变。
- WeType，目标输入法 = 搜狗（Win+Space 切一次）：收音正常，停止后约 1.2s WeType「已复制 · 粘贴即可使用」，点墨打进 14 字（含关键字），剪贴板 6 个格式全部恢复、指纹一致。
- WeType 取消（4s 时 Esc，搜狗）：没上屏；WeType 随后仍复制了结果，被 Discarding 阶段恢复。
- DoubaoVoice：未运行时自动启动，约 5.4s 后发 F6，600ms 开始收音；停止后「优化中…」，约 1–2s 后它自己粘贴上屏 14 字（含关键字），并把剪贴板留成识别结果 → 点墨 1s 后恢复，指纹一致。测完已结束 DouBaoVoice 进程。
- 回退：WeType 没收音 → 1.5s 后 `Failed` + Win+H 面板「正在聆听」。

**发现的问题 / 注意**：
- **WeType 的语音进程会崩**：第一次实测成功上屏后 1s，`wetype_update.exe` 在 `GraphicsCapture.dll` 访问冲突退出（事件日志 1000，22:10:58），WeType 不会自己拉起，之后热键全部无效。已在 `start` 里处理（自动拉起）；但手动拉起的 `wetype_update` 热键也不灵，最后是结束 WeType 三个进程、在普通权限记事本里激活 WeType 让 TIP 重新拉起才恢复（期间约 10 秒用户的 WeType 不可用）。之后十几次会话没再崩。
- **管理员窗口**：WeType 进程以普通权限运行时（重新拉起后就是），前台是管理员窗口时它收不到注入的热键（UIPI），点墨会在 1.5s 后回退 Win+H。调研时 WeType 进程碰巧是提权的，所以当时管理员记事本也行。DoubaoVoice 同理（它的 Ctrl+V 也进不了管理员窗口）。
- Win+H 回退打开的系统语音面板在目标窗口关掉后仍停在屏幕顶部（空闲态），要用户点 ×。
- 电脑键 `Ctrl+Win+←/→`：WeType 的按住说话是按住 Ctrl+Win，布局发这类组合键时建议先按 Win 再按 Ctrl（或保持一次 SendInput 发完），见 voice.rs 模块文档；未实测。
- 测试副作用：用户剪贴板文字内容保持原样，但有几次是测试脚本用 `Set-Clipboard` 写回的文字，格式数从 4 变成 6（文字相同）。
- Surface 上 `C:\dev\dianmo-voicebuild\target`（389MB）保留着 voice_probe.exe，供主会话复测；不需要时 `scripts/surface/clean.sh voicebuild`。

## 语音 + 悬浮球接入主程序、发版前验证（TODO #29 / #30，2026-10-06）
设计见 DESIGN.md「悬浮球」「语音模式 / 语音球」（已按实现更新）。

**改动**
- `settings.rs`：`voice_engine=wetype|doubao|system`（默认 wetype，其他值忽略）、可选 `voice_doubao_exe=`、`voice_mode=true|false`、`ball_edge=left|right` + `ball_y=<0–1>`（两个都有才生效）。
- `app.rs`：
  - `DianmoApp` 里放一个 `Voice`（`set_log(log::write)`、豆包 exe 来自设置）。`UiAction::Voice`（工具栏、电脑键盘细栏的麦克风键）和语音模式下的 `BallEvent::Tap` → `voice_toggle`：清掉未上屏组字 → `voice.toggle()` → `voice_sync`。修饰键由 dianmo-ui 在发 `UiAction::Voice` 之前松开（见下）。
  - `voice_sync`：Starting/Listening → `set_ball_state(Listening)` + `KeyboardView::set_voice_active(true)`，其余 → Idle / false（只在变化时发）；新的 `Failed(msg)` 在键盘显示时 `show_toast(msg)`；`needs_poll()` 时在键盘窗口上 `SetTimer(0xD1A0, 300ms, TIMERPROC)`（单次：回调里 `KillTimer` 再 `HostProxy::post(VoiceTick)`，`on_event` 里 `poll()` 后需要就再装），不需要就不装，空闲 CPU 为 0。
  - 回退到 Win+H 后 30 秒内再点一下 → 只再发一次 Win+H（关掉系统语音面板）并回到 Idle，不再去试微信输入法（实测：第一次点 → 1.5 秒后回退并打开「正在聆听」面板；第二次点 → 面板关闭）。
  - 收起键盘（非语音模式）、换引擎时 `cancel()`；退出时如果还在收音也 `cancel()`（实测 WM_CLOSE 时 WeType 正在收音 → 麦克风随即释放）。
  - 剪贴板：`voice_running()`（`needs_poll` 或 `is_active`）期间以及最后一次语音状态更新后 3 秒内，`ClipEvent::Text/NoText/Private` 一律忽略（日志 `clipboard change during voice input: not recorded`），所以引擎借剪贴板交付的结果、我们恢复用户剪贴板引起的变化都不进历史、不弹「已复制」、不改粘贴预览。代价：这段时间里用户自己的复制也不记录。
  - `on_ball`：语音模式 Tap → 语音；其余 Tap / LongPress → `show()`；`Moved` → 存 `ball_edge/ball_y`。`main.rs` 把它们放进 `HostOptions::ball_pos`。
  - 语音模式下 `FocusEvent::Editable{by_touch}` 不再 `show()`（数字框切数字面板等仍照常）。`UiAction::VoiceBall`（布局菜单「语音球」）/ 托盘「语音模式」打开时：存设置并收起键盘。
  - 托盘：布局组下面加「语音引擎 ▸ 微信输入法 / 豆包语音 / 系统语音（Win+H）」（当前引擎打勾；不可用的写「（未安装）」或「（未运行）」）和「语音模式（悬浮球点一下说话）」。各引擎是否可用在后台线程里查（`Voice::why_unavailable`，启动时、换引擎后、每次会话结束后），结果 post 回来，托盘菜单不在 UI 线程上做进程快照。
  - 日志：`voice toggle (<engine>) -> <state>`、`voice: <state>`；`memory (started / keyboard shown / librime started)` 记私有内存（`K32GetProcessMemoryInfo`，kernel32 导出，导入表不变）。
  - 测试钩子：`DIANMO_NO=clipboard,ball,focus` 不启动对应部分（内存测量用）；`DIANMO_KEYMAP` 多导出 `voiceball`、`t9_1`…`t9_9`（九宫格按数字找键：宽屏九宫格右边有数字小键盘，`6` 是那里的键）。
- `voice.rs`：只加了 `Voice::why_unavailable(e)`（任意线程可调）；`#![allow(dead_code)]` 留着（报告、指纹等只有 voice_probe 用）。
- dianmo-ui（只为语音）：`UiAction::VoiceBall`；`KeyboardView::set_voice_active(bool) -> bool` / `voice_active()`（麦克风键画成红色圆底白话筒）；点麦克风键时先 `release_modifiers`（锁定的修饰键清零；电脑键盘里按着的键和修饰键发 `KeyUp`，排在 `UiAction::Voice` 前面）；布局菜单第 7 个磁贴「语音球」（MDL2 话筒图标）；`show_toast` 的时长按字数 1–4 秒（「已复制」仍 1 秒）；`key_center("t9_<n>")`。新测试 4 个（共 64 个）。
- dianmo-win（为了内存，见下）：`Renderer::release()`；键盘隐藏 5 秒后（`TIMER_TRIM`，单次）释放 D3D/WARP 设备、交换链、D2D 上下文；显示时 `InvalidateRect`，第一次绘制时重建。没有改公开 API。
- clippy：`collapsible_if` 的自动修复（dianmo-core 1 处、dianmo-ui 8 处、dianmo 3 处），`cargo clippy --workspace --all-targets`（Linux）和 `--target x86_64-pc-windows-gnullvm --examples --tests` 都干净。

**内存回归的结论**（同样的 e2e 步骤 `n i h a o space`，私有内存，进程自己的 `PrivateUsage`）
| 版本 | 私有内存 |
|---|---|
| `dist\elevate`（提权那轮，宽屏之前） | 27.4MB |
| `dist\layout`（宽屏五行 + 电脑按键） | 31.9MB |
| `dist\pcclip`（+ 电脑键盘、剪贴板、悬浮球、Raw Input） | 32.4MB |
| 本轮 `final` | 32–35MB；跑过电脑键盘 + 剪贴板面板 + 触控板的长测试后 43MB |
| 本轮，`DIANMO_ENGINE=basic`（不带 librime） | 25MB |
| 本轮，`DIANMO_NO=clipboard,ball,focus` | 29.9MB |
| 本轮，键盘收起 5 秒后 | **15.5MB**（显示时 34.7MB） |
- 上一轮报的 33–42MB 主要是测量条件不同：那次是跑完一长串步骤（电脑键盘、剪贴板面板、触控板）之后量的。同样步骤下 pcclip 和宽屏那版都是 32MB 左右，宽屏那轮比之前多了约 4.5MB（更多按键文字、字号、图标字形的缓存），剪贴板 / 悬浮球 / Raw Input 这轮新增的部分合计不到 2MB。
- 最大的一块是 **WARP 软件渲染**：`VirtualQueryEx` 看私有提交，最大的几块是 7–9MB（≈ 2880×703×4 = 7.9MB，正好是整宽键盘的一个表面）、5–8MB、4MB、2MB，打过字后合计 16–19MB；librime 约 10MB；焦点监听（UIA）、剪贴板监听线程、悬浮球各 1–2MB（单独关掉的差别在测量噪声里）。
- 已做的低成本优化：键盘隐藏 5 秒后释放渲染设备（dianmo-win `TIMER_TRIM`）。实测隐藏后 34.7 → 15.5MB；再显示（点球）的 CPU 增量约 78ms（含点击处理和第一次绘制；不释放时约 20–30ms），100ms 后截图键盘完整，之后打字正常。键盘大部分时间是收起的（语音模式更是），这是收益最大的一步。显示期间的 16–19MB 是 WARP 必需的表面，要再降只能改用硬件 GPU（驱动反而多 47MB，见 dianmo-win 状态）或局部重绘 / 更小的表面，不在本轮做。

**Surface 实测**（`package.sh final --keep-target` → `C:\dev\dianmo-dist\final\Dianmo`，exe 1029KB，导入表只有系统 DLL；测试副本 `--instance Test --no-elevate`；用户自己的点墨（旧版，边缘把手）一直在运行，没有动它）
- 回归：全拼 `n i h a o space 2 0 2 6` → `你好2026`；全选 + 复制 → 剪贴板卡片栏，→ 粘贴 → `你好2026你好2026`；小鹤 `n i h c space` → 你好；九宫格 `t9_6 t9_4 t9_4 t9_2 t9_6`（截图：左列 ni/mi/m/n/o，候选你好）+ 空格 → 你好；电脑键盘直通 + Ctrl+A/C/V + 剪贴板卡片 + 选择模式 + 触控板选择删除（上一轮那串步骤）结果和上一轮一致。每次 WM_CLOSE 退出码 0，系统键盘设置、工作区、用户剪贴板文字都恢复；空闲 10 秒 CPU 0ms。
  - 宽屏 26 键里输入中点数字行（`w o m e n 2`）是先上屏首选再打数字（`我们2`），不是选第 2 个候选：宽屏数字行设计如此（「数字直接上屏」），不是本轮的回归。
- 语音（微信输入法，记事本用 explorer 以普通权限启动 = `MEDIUM`）：点麦克风键 → 热键后 300–320ms 麦克风占用（`wetype_update.exe` 的 `LastUsedTimeStop=0`）、麦克风键变红、WeType 的绿色语音条出现；再点 → finishing → 约 3 秒后 idle、麦克风释放。有一次 WeType 从环境声音里识别出「截图。」直接打进了测试记事本（WeType 是该窗口的输入法时由它自己上屏），说明整条链路是通的。
  - 记事本以管理员权限运行时（gui 任务默认）WeType 收不到热键 → 1.5 秒后提示「微信输入法没有开始收音，已改用系统语音输入」（截图里键区中间的提示）并打开 Win+H 面板；再点一下 → Win+H 面板关掉。
  - 托盘「语音引擎 ▸ 系统语音」→ `voice_engine=system`，点麦克风 → Win+H 面板「正在初始化…」，再点 → 关掉。
  - 剪贴板：收音中用另一个进程写剪贴板（`CLIPSET`）→ 日志 `not recorded`，voice 也判定「不是 WeType 的，不动它」；结束 3 秒后再写 → 正常进历史，剪贴板栏只有后面那一张卡片。
- 语音模式（`SET:voice_mode=true`）：收起键盘后点球 → 收音（球上粉色呼吸光圈，截图确认）；再点 → 结束；长按球 → 键盘出来；拖球向上 400px → `ball_edge=left; ball_y=0.2722`。`ball_edge=right, ball_y=0.3` 启动 → 球在右边缘，球心 y = 0.3 × 工作区高。语音模式下点记事本输入区 → 键盘不弹出（对照：非语音模式同样操作 → 弹出）。布局菜单「语音球」磁贴（截图）→ `voice_mode=true` 并收起键盘；托盘「语音模式」再关掉。
- 图标：托盘溢出区里测试副本是新图标（渐变圆角方形 + 白色书法点），用户在跑的旧版是蓝底「墨」；悬浮球截图边缘平滑。点墨键盘窗口是 TOOLWINDOW，没有任务栏按钮；桌面快捷方式要等安装新版后才换图标。
- 测完：没有留下弹窗、Win+H 面板或托盘溢出浮窗（最后截了一次整屏确认），WeType 麦克风已释放；用户的剪贴板文字每次都恢复。

**`C:\dev\dianmo-dist\final\Dianmo` 可以直接安装**：`scripts/surface/package.sh final --install --no-build`（会用 WM_CLOSE 关掉用户正在运行的点墨再复制）。设置是向后兼容的（新键缺省：微信输入法、非语音模式、球在左边 62%）。

**e2e 新步骤**（`tests/surface/e2e.ps1`）：`MEDIUM`（普通权限记事本）、`ENV:k=v`、`BALL` / `BALLTAP` / `BALLHOLD` / `BALLDRAG:dx:dy`、`MIC`、`INI`、`VMAP`（私有提交按分配列出）、`CPU`、`CLIPSET:<文字>`、`ESC`；`TRAYMENU[:<下>:<右>]` / `TRAYPICK<n>` / `TRAYPICK:<d r e 序列>` 改成给**测试副本自己的**托盘窗口发图标回调消息（以前按类名 `FindWindow('DianmoTray')`，可能找到用户的点墨）。九宫格键用 `t9_<n>`。

**遗留**
- 语音球形态（键盘收起）下引擎失败没有文字提示，只有系统语音面板；以后可以给球加气泡提示。
- 语音进行中及之后 3 秒内，用户自己复制的内容不进历史（为了排除引擎的结果和恢复动作）。
- 布局菜单里的磁贴切换不会触发 keymap 重写（键盘内部面板切换），自动测试点「语音球」磁贴用的是坐标。
- 豆包语音这轮没在主程序里实测（托盘里可选、可用；voice 模块上一轮单独测过）。
- 重新显示时重建 WARP 设备多花约 50ms CPU；如果用户觉得弹出变慢，可把 dianmo-win `host.rs` 的 `TRIM_AFTER_MS` 调大。

## 热修：语音模式退出不了、提权后微信输入法语音失灵（2026-10-06 晚）
用户报告：切到语音球后「退不出来，好多地方点不开」。日志：语音模式下反复「微信输入法没有开始收音，已改用系统语音输入」→「voice toggle after fallback: Win+H again」，每点一次球就开 / 关一次 Win+H 面板（TextInputHost 的全屏 CoreWindow），双击桌面图标也没让人找到出口。

**根因实测（Surface，WeType 2.1.3.18，`wetype_update.exe` 为 MEDIUM）**，同一个 `LCtrl↓ LWin↓ LShift↓↑ LWin↑ LCtrl↑`：
| 前台窗口 | 提权进程 SendInput | 普通权限进程 SendInput |
|---|---|---|
| 普通记事本（MEDIUM） | 283 ms 开始收音 | 收音 |
| 管理员记事本（HIGH） | 2.5 s 无反应 | 2.5 s 无反应 |
- 结论：「提权的点墨发热键微信输入法收不到」不成立（用户自己的日志 15:26:09 提权实例也是 302 ms 收音）；收不到只发生在**管理员窗口在前台**时，谁发都一样（UIPI 挡住了发往高完整性前台窗口的输入，MEDIUM 的 WeType 看不到）。管理员记事本里用 Win+Space 把输入法切换几轮（含 WeType）也一样。所以没做普通权限辅助进程（帮不上）。
- 用户当时多半是在管理员 PowerShell 里说话（桌面上有一个「管理员: Windows PowerShell」窗口）。

**修复**：
- 语音模式随时能退出：长按球 / 托盘 / 再次启动都显示键盘（实测：语音模式下第二次启动 → 键盘显示；长按球 → 显示）；键盘候选条最前面加强调色「退出语音模式」（电脑键盘细栏居中也有），布局菜单磁贴变「退出语音球」（`UiAction::VoiceBall` 在 app 里改为切换）。`KeyboardView::set_voice_mode(bool)`，keymap 名 `exitvoice`。
- 球可发现：进入语音模式（以及本次运行里语音模式下第一次出现球）时，球 10 秒不半隐（重设 dianmo-win 球窗口的 3 秒收边计时器，id 2，见 `bubble.rs::keep_ball_out`；dianmo-win 以后最好给一个正式接口），旁边弹 GDI 小气泡「点一下说话，长按展开键盘」（`src/bubble.rs`，不抢焦点，点一下或 10 秒后消失）。键盘收起时的语音失败原因也用这个气泡（5 秒）。
- 不再回退 Win+H（用户要求，`voice.set_fallback(false)`）：失败只提示原因；去掉「回退后 30 秒内再点 = Win+H」的逻辑；`fallback=false` 时也不再「失败两次就跳过引擎」，每次都重试。
- 发热键前看前台窗口完整性级别（`TokenIntegrityLevel`），高于引擎进程就立刻提示「微信输入法收不到管理员窗口里的语音快捷键（Windows 权限隔离），请在普通窗口里使用」，不等 1.5 秒、不计失败次数。
- 新引擎 `VoiceEngine::DoubaoIme`（豆包输入法，`voice_engine=doubao_ime`，托盘「豆包输入法」；第三方小工具改名「豆包语音（第三方）」）：TIP `{9D2B2E2B-3C93-4D2F-9D35-6EEB85F0D2B0}`，进程 `C:\Program Files\DoubaoIME\versions\<ver>\ImeService.exe`（以 HIGH 运行，是它自己的设计），配置 `%APPDATA%\DoubaoIme\conf\config.json` 的 `voice.enableVoiceShortcut` / `enableGlobalVoiceShortcut` / `voiceShortcutMode`（安装后是 `right_alt_space`，即「免按模式」右 Alt+空格；长按模式 `voiceLongPressShortcutMode=right_alt`）。热键 `RAlt↓ Space↓ Space↑ RAlt↑`（RAlt 扩展键）。可用条件：已装、在输入法列表、ImeService 在运行、语音快捷键开着、模式是 `right_alt_space`、**全局语音快捷键开着**（否则目标窗口输入法不是它时 Alt+空格会弹系统菜单）。收音判定：consent store 里路径含 `#DoubaoIME#` 的项，或本次会话开始后目标进程本身的项。剪贴板兜底同 WeType（owner 为 `imeservice.exe`）。
- 日志：`keyboard shown|hidden`、`auto-hide: touched outside a text field`、`voice mode hint (...)`。
- e2e 新步骤：`FGADMIN` / `FGMED`、`BUBBLE`、`SECOND`、`SPEAK:<t>`、`TEXTKW:<kw>`；`MIC` 也列豆包输入法。

**豆包输入法（官方）安装**：`https://shurufa.doubao.com/api/v1/app/download_url?platform=windows`（官网 shurufa.doubao.com/pc 的下载按钮用的接口）→ `lf-wave.doubaocdn.com/.../DoubaoIME_Installer_0.9.1.22_release.exe`，签名「北京春田知韵科技有限公司」（字节旗下，有效），Inno Setup，`/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-` 静默装到 `C:\Program Files\DoubaoIME`，安装包在 `C:\dev\dianmo-voice\installers\`。安装程序把自己设成了默认输入法并排到列表第一：已改回（默认仍是微软拼音，列表顺序 WeType、微软拼音、81D4…，豆包排最后）。

**实测（`C:\dev\dianmo-dist\hotfix\Dianmo`，从 gui 任务启动 = 提权，`elevated=true`）**：
- 语音模式启动 → 键盘在、`exitvoice` → `voice_mode=false` 键盘留着；托盘「语音模式」→ 键盘收起、球完全露出（4.5 秒后仍未半隐）、气泡显示；语音模式下 `SECOND` → 键盘显示；收起后长按球 → 显示。空闲 10 秒 CPU 0 ms，WM_CLOSE 退出码 0，剪贴板恢复。
- 豆包输入法（当前配置 `voiceShortcutMode=null`、全局快捷键关）→ 点球立刻在球旁气泡提示原因，不发任何热键。
- **未通过 / 未测**：这一轮 WeType 在普通记事本前台时也不收音了（点墨发、PowerShell 提权发都不行；停掉豆包输入法的 ImeService 也不行），而同一台机器 1 小时前同样的测试是 283 ms 收音 → 判断是 WeType 的语音进程又进入了「热键失灵」状态（见上面「WeType 的语音进程会崩」一条；恢复办法是结束 WeType 三个进程后在普通窗口里重新激活 WeType），没有在用户正在用的机器上动它。管理员窗口分支（立即提示）的逻辑没在点墨里实测（测试脚本没能把管理员记事本切到前台），判断依据是上面的 PowerShell 矩阵。豆包输入法的热键：注入 `右 Alt+空格`（扩展键）和按住右 Alt 都没让它收音（当时它是记事本的当前输入法、配置为 right_alt_space、全局关；可能与没完成首次引导或与搜狗「AI汪仔」抢右 Alt 有关——测试时 AI汪仔 侧栏弹了出来，已关掉），所以豆包输入法引擎是按配置实现的、未实测通过。Edge 输入框、TTS 上屏没测成（WeType 不收音）。

## 安装包、卸载、检查更新、反馈与诊断（PRODUCT.md P1 / P2 / P6 / P7，2026-10-06）
设计取舍见 DESIGN.md §6.1。

**安装包 `crates/dianmo-setup`**（`DianmoSetup-<版本>.exe`）
- stub `dianmo-setup.exe`（windows 子系统，manifest：asInvoker、PerMonitorV2、Common Controls v6；图标和版本信息复用 `crates/dianmo/res/dianmo.ico`）585KB，导入表只有系统 DLL；打包工具 `dianmo-pack <stub> <dist> <out.exe> [--version v]`（控制台，两线程压缩）。负载格式见 `src/lib.rs`：逐文件 zlib（miniz_oxide 0.8，纯 Rust，Adler-32 校验）追加在 stub 后面，尾部 32 字节索引。实测 dist 46 个文件 49.2MB → 安装包 **20.7MB**，打包 12.7s。
- 参数：`/S`（静默，升级用；装完以 `--hidden` 启动）、`/NORUN`、`--instance <名>`（测试：任务 / 卸载项 `Dianmo<名>`、快捷方式「点墨 <名>」、默认目录 `%LOCALAPPDATA%\Dianmo-<名>`）；环境变量 `DIANMO_INSTALL_ROOT` 指定安装目录（测试用）。日志 `%TEMP%\DianmoSetup.log`。
- 流程：解压到 `<目录>.new` → `<目录>.new\dianmo.exe --quit`（退出码 3 = 对方提权、消息被 UIPI 挡住 → 提权再跑一次）→ `<目录>`→`.old`、`.new`→`<目录>`（失败回滚；改名失败时覆盖复制）→ `<目录>\dianmo.exe --install [--quiet] [--no-run]`。界面：一个小窗口「点墨 · 正在安装…」+ 进度条（GDI 自绘，200% 下已截图确认），没有按钮；失败时一个 MessageBox 说明原因。拒绝 UAC 时仍装好，提示「暂时不能给管理员窗口输入，可以在设置里修复」。
- 为什么 asInvoker + 单独提权：见 DESIGN.md §6.1（拒绝 UAC 仍可用；从提权的点墨发起的升级不弹 UAC；重装时任务已指向同一 exe 则完全不弹；装完以用户身份启动点墨）。

**`dianmo.exe` 新参数**（`src/install.rs`，main.rs 只加了分支）
- `--quit`：找本实例的消息窗口，发 `WM_APP+0x52`（platform.rs `WM_APP_QUIT`，提权实例用 `ChangeWindowMessageFilterEx` 放行，和 show 消息一样）；4 秒没退出（旧版本不认识这条消息）就给该进程的 `DianmoKeyboard` 窗口发 `WM_CLOSE`；等进程句柄（打不开就等窗口消失）。退出码 0 = 没在运行或已退出，1 = 还在运行，3 = 被 UIPI 拒绝。从不强杀。
- `--install [--quiet] [--no-run]`：桌面和开始菜单「点墨」、开始菜单「点墨设置」（`--settings`）；HKCU `...\Uninstall\Dianmo`（DisplayName 点墨、DisplayIcon、DisplayVersion、Publisher 云中江树、URLInfoAbout、HelpLink、InstallLocation、InstallDate、UninstallString `"<exe>" --uninstall`、QuietUninstallString `... --uninstall --quiet`、EstimatedSize、NoModify、NoRepair）；计划任务已指向本 exe 就跳过，否则提权跑 `--register-task`（已提权就直接跑）；最后启动点墨。退出码 0 / 2（任务没注册）/ 1（快捷方式或卸载项失败）。
- `--uninstall [--keep-data | --delete-data] [--quiet | /S]`：不安静时先问（MessageBox 是/否/取消：保留个人数据 / 一并删除 / 不卸载，默认保留）→ 关掉点墨（被 UIPI 拒绝就提权 `--quit`）→ settings.ini 里还有 `saved_*`（崩溃留下的）就恢复系统触摸键盘设置 → 删计划任务（没权限就提权 `--unregister-task`）、旧 Run 项、快捷方式、卸载项 → 弹「点墨已卸载」→ 起一个隐藏的 `cmd`（工作目录 %TEMP%），每秒重试 `rmdir` 直到程序目录（和选了删除时的 `%APPDATA%\Dianmo`）删掉，最多 30 秒。程序目录只在等于登记的 InstallLocation / 默认目录 / `DIANMO_INSTALL_ROOT` 且含 dianmo.exe 时才删（从构建目录误跑不会删代码）。退出码 1602 = 用户取消。
- `--check-update`：打印检查结果（测试用；`DIANMO_UPDATE_REPO=owner/name` 换仓库；`DIANMO_UPDATE_TEST_DOWNLOAD="<url> <size> <sha256>"` 按升级的方式下载一个文件并校验）。
- `--diagnostics`：打印预填好的 issue 链接并导出诊断包。

**检查更新 `src/update.rs`**（给 app.rs，已在用 `update::Release`）
```rust
update::start_auto_check(proxy, settings.last_update_check); // 启动时一次：1 分钟后检查，之后每天最多一次
update::set_auto_check(settings.auto_update);                 // 「自动检查更新」开关
update::check_async(proxy);                                   // 「检查更新」（进行中再点会被忽略）
update::download_and_install(release, proxy);                 // 「立即更新」
// App::on_event: event.downcast::<update::UpdateEvent>()
enum UpdateEvent {
    Checked { auto: bool, at: u64, outcome: Result<CheckOutcome, String> }, // at → settings.last_update_check
    Progress(f32),   // → UpdateState::Downloading
    Installing,      // 安装包已以 /S 运行，它会 --quit 本实例；可以收起设置窗口
    Failed(String),  // → UpdateState::Failed
}
enum CheckOutcome { UpToDate { latest }, NoRelease, Available(Release) }
struct Release { version, tag, notes /* 纯文本，≤1200 字 */, url /* 空 = 没有安装包，打开 page */, size, sha256, page }
```
- WinHTTP（系统代理 `WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY`，超时 10/10/15/30s，UA `Dianmo/<ver>`）请求 `https://api.github.com/repos/yzfly/dianmo/releases/latest`；404（仓库不存在或没有 release）→ `NoRelease`，403/429 → 「GitHub 暂时限制了访问次数」，网络错误翻成中文。JSON 用内置小解析器（不引 serde_json）；版本按 semver 比较（预发布低于正式版）。说明文字去掉 Markdown / HTML 标记。
- 下载到 `%TEMP%\DianmoSetup-<ver>.exe.part`，校验大小和 GitHub 资产的 `digest`（SHA-256，CNG `BCRYPT_SHA256_ALG_HANDLE`），通过后改名并以 `/S`（带 `--instance`）运行。
- 自动检查线程：睡 60s，之后每小时醒一次看墙钟（休眠不会拉长「一天」），`AUTO` 关时什么都不做；每进程只起一个。没有 UI 线程定时器。
- 体积：dianmo.exe 增加约 126KB（WinHTTP、JSON、zip、安装 / 卸载），导入表多了 `winhttp.dll`（已加进 package.sh 白名单）。

**反馈与诊断 `src/diag.rs`**
- `diag::report_issue_url() -> String`：`https://github.com/yzfly/dianmo/issues/new?title=&body=…`，预填版本、系统（注册表 ProductName + DisplayVersion + 内部版本.UBR，build ≥ 22000 写 Windows 11）、屏幕（主显示器分辨率、缩放，多显示器注明个数）、管理员模式、语音引擎。
- `diag::export() -> Result<PathBuf, String>`：桌面 `点墨诊断-<日期>-<时间>.zip`（stored zip，UTF-8 文件名）：info.txt（以上信息 + exe 路径、实例、计划任务、数据目录文件清单）、dianmo.log、dianmo.log.old、settings.ini、安装日志；**不含** clips.txt 和 rime 用户词库；所有文件里的用户目录替换成 `%USERPROFILE%`。
- `diag::open_url(url)` / `diag::reveal(path)`：在短线程里打开；点墨提权时通过桌面 Explorer 的 `IShellDispatch2::ShellExecute` 打开（浏览器以普通用户身份运行，不会变成管理员浏览器），失败再退回 `ShellExecuteW`。
- `diag::system_info()` / `issue_body()` / `issue_url()` 是纯函数部分，Linux 上有测试。

**打包**：`scripts/surface/package.sh <名字> --setup [--keep-target]`：构建时多带 `-p dianmo-setup`，组装 dist 后检查 stub 导入表、打 `C:\dev\dianmo-dist\<名字>\DianmoSetup-<ver>.exe`（ver = 工作区版本），scp 回仓库 `dist/`（已加进 .gitignore）。

**测试**
- 服务器：`cargo test -p dianmo-setup`（负载往返、损坏检测、路径安全，3 个）；dianmo 新增测试 6 个（版本比较、JSON、release 解析 / 说明文字、issue 链接编码、脱敏、zip 结构；zip 另用 Python zipfile 验证过）。clippy（gnullvm）干净。
- Surface（用 `HEAD + 本轮改动` 的隔离副本构建，`--instance SetupTest`，`DIANMO_INSTALL_ROOT=C:\dev\dianmo-instest\Dianmo`，gui 任务 = 提权；用户自己的点墨一直在运行，没碰）：
  - 静默安装 2.2s（解压 0.5s）：46 个文件 / 49.2MB、任务 `DianmoSetupTest`（`--task --instance SetupTest`，Highest）、卸载项各值正确（见上）、三个快捷方式目标和参数正确。
  - 提权实例 + 用 RunLevel Limited 的临时任务跑 `--quit` → 1.2s 内正常退出（UIPI 放行生效，日志「asked to quit」→「exit」）。
  - 覆盖升级：运行中的实例被安装包 `--quit` 关掉，目录整体替换（旧目录里多放的文件消失，没有留下 `.old`），任务已指向同一 exe → 不再注册。
  - `--check-update`：rust-lang/rust → 「available 1.99.0」，说明文字正常；yzfly/dianmo（还不存在）→ `NoRelease`。下载：ripgrep 的一个 109 字节资产（302 跳转到 objects.githubusercontent.com）→ 成功；错的 SHA-256 → 「安装包校验失败」；错的大小 → 「下载不完整」；404 → 「下载失败（HTTP 404）」。
  - `--diagnostics`：issue 链接 1092 字符；zip 4 个文件，info.txt「Windows 10 Enterprise LTSC 2021 21H2（19044.7184）/ 2880×1920，缩放 200% / 管理员模式：是」，路径已脱敏；测完删掉。
  - 卸载（`--quiet --delete-data`，实例在运行）：退出码 0，实例退出，程序目录、数据目录、任务、卸载项、快捷方式全部清掉。
  - 交互安装（`/NORUN`）：进度窗口截图正常（200%，标题栏图标、「点墨 · 安装完成 / 版本 0.1.0 · 100%」、蓝色进度条），整个安装不到 1 秒。
  - 测完 Surface 上没有残留（`C:\dev\dianmo-setup`、测试 dist、测试目录、任务、快捷方式都删了）。

**遗留 / 没测**
- 没测普通权限下的 UAC 路径（安装时注册任务、卸载时删任务的提权弹窗；用户正在用机器，不弹 UAC），以及拒绝 UAC 的分支；逻辑是 `ShellExecuteEx runas`，`ERROR_CANCELLED` → 退出码 2 / 提示。
- 交互式卸载的 MessageBox（是/否/取消、完成提示）没在实机上点过。
- `download_and_install` 的最后一步（以 `/S` 运行真正的安装包、旧实例被关掉、新版 `--hidden` 启动）要等 yzfly/dianmo 有第一个 release 后实测；从当前已安装的旧版（不认识 `WM_APP_QUIT`）升级时，会走「4 秒后 WM_CLOSE」，若旧版是提权的则安装包要提权一次（弹一次 UAC）。
- 「点墨设置」快捷方式只在开始菜单；桌面只有「点墨」。package.sh 的 `--install` 段仍是原来的 robocopy 流程（另一个 agent 在改），以后可以改成直接运行安装包 `/S`。
- 安装包没有签名；SmartScreen 对没信誉的下载会拦一次（「仍要运行」）。

## 设置窗口、关于、首次引导接入主程序（TODO #33 / PRODUCT P3 P4 P5 P8 P9，2026-10-06）

**改动**
- `settings.rs`：新键 `input_mode=keyboard|voice|pc`（取代 `voice_mode` / `pc_keyboard`，旧键仍能读，写回时只写新键）、`theme=system|light|dark`（默认跟随系统）、`show_ball`、`key_popup`、`key_sound`、`long_press=short|medium|long`、`candidate_size`、`full_width_punct`、`space_commits_first`、`fuzzy=z-zh,n-l,…`、`shuangpin`、`clip_history`、`clip_limit`（默认 50）、`clip_skip_passwords`、`auto_update`、`last_update_check`、`onboarded`；`reset_preferences()`（恢复默认：保留开机自启、引导状态、球的位置、`saved_*`）。
- `prefs.rs`（纯逻辑，Linux 上有测试）：`model(&Settings, &SysState) -> SettingsModel`、`apply(&mut Settings, &SettingsAction) -> bool`、`engine_status`（引擎检测结果 → 状态点 / 说明 / 下载链接）、`effective_theme` / `window_dark`、`layout_schema`。测试保证「视图自己 apply 后的模型」和「主程序存进 ini 再读出来的模型」一致。
- `app/settings_ui.rs`（`DianmoApp` 的设置窗口部分）：
  - 设置窗口单例（960×680，可缩放，最小 420×420，再开就 `focus_window`，可指定分页）；引导窗口 760×560 不可缩放。
  - 窗口发来的 `UiAction::Settings(a)` 先 post 给自己（`SettingsCmd`），在 `on_event` 里拿着键盘 View 处理：存 ini → 立即作用到键盘（主题、高度、编辑区、按键气泡、长按时长、布局、输入模式）、宿主（让出屏幕空间、悬浮球开关 / 位置）、系统（开机自启、管理员任务）、语音引擎、剪贴板（条数上限、不记录、不记密码框、清空、取消固定）。
  - 每个回调结束时 `sync_windows`：模型变了就 `update_window` 推给打开的窗口（托盘、键盘布局菜单、拖球改了设置也会刷新）；处理完设置动作后强制推一次，所以没能生效的开关会弹回去。
  - 主题「跟随系统」：`system_dark_mode()` + dianmo-win 新的 `App::on_system_theme_changed`（键盘窗口收到 `WM_SETTINGCHANGE "ImmersiveColorSet"` 时回调，不轮询）。
  - 管理员窗口支持：打开设置时后台查计划任务；「一键修复」= 已提权就直接 `elevate::register`，否则 `ShellExecuteEx runas dianmo.exe --register-task`（UAC），完成后提示并重查。
  - 开机自启失败（任务存在但点墨没提权）时提示原因，开关弹回。
  - 豆包语音程序位置：后台线程里开系统的文件对话框（`shell::pick_exe`，IFileOpenDialog），不阻塞 UI 线程。
  - 关于页：链接、反馈问题、打开日志目录走 `diag::open_url`（提权时经 Explorer 以普通权限打开）；导出诊断包在后台线程里 `diag::export()`，完成后提示并在资源管理器里选中；检查更新 / 立即更新 / 自动检查接 `update.rs`（启动 1 分钟后自动检查，结果存 `last_update_check`；有新版时托盘「关于点墨（新版本 x）」）。
  - 下个版本才做的：按键音、用户词库导入 / 导出 / 清空 → 设置窗口底部提示「…下个版本提供」；候选字号、全角标点、空格上屏首选 → 保存并在行上标「下个版本生效」（`SettingsModel::coming_soon`）；模糊音同理（`fuzzy_supported=false`）。
  - 首次引导：`onboarded` 不是 true 时 `on_start` 打开；完成 / 跳过 / 关窗都写 `onboarded=true`；引导里选布局、语音引擎立即生效。`DIANMO_NO=onboarding` 可关掉（测试用）。
- **托盘（P8）**：显示/隐藏键盘、布局 ▸（全拼 / 小鹤双拼 / 九宫格 / English）、语音引擎 ▸、输入模式 ▸（键盘 / 语音球 / 电脑键盘）、设置…、关于点墨、退出点墨。dianmo-win 的内置项关掉了（`tray_builtins: false`）；高度、主题、编辑区、自动弹出、开机自启、让出屏幕空间都挪到设置里。
- **入口**：键盘工具栏 ⚙（`UiAction::OpenSettings`）；`dianmo.exe --settings`（已运行时发 `WM_APP+0x53` 给运行中的实例，提权实例放行这条消息；没运行就隐藏启动并打开设置；经计划任务转交时也转发）；开始菜单「点墨设置」（`package.sh` 安装段；`install.rs` 也会建）。
- **悬浮球**：`bubble::keep_ball_out`（重置 dianmo-win 内部计时器的 hack）删掉，改用 `HostControl::reveal_ball(ms)`；气泡在球出来之后再显示（post 一条 `ShowBubble`）。「显示悬浮球」关掉时球消失（`set_ball_enabled`），语音模式下总是显示。
- **资源**：exe 嵌入 RCDATA `app-icon`（`res/app-icon.png`，256px，由 `res/icon/dianmo.svg` 导出）；`build.rs` 设置 `DIANMO_BUILD_DATE`（UTC 日期，源码有改动就更新，可用同名环境变量覆盖）。
- `shell.rs`：`run_elevated`（UAC）、`pick_exe`（文件对话框）。Cargo features 加了 `Win32_UI_Shell`、`Win32_UI_Shell_Common`。
- e2e：测试副本默认写 `onboarded=true`（否则引导窗口会抢前台），`SET:onboarded=false` 可以覆盖。

**测试**
- 服务器：`cargo test --workspace` 136 个全过（dianmo 20 个，新增 settings 2 个、prefs 5 个；dianmo-ui 92 个，新增 7 个）；`cargo clippy --workspace --all-targets` 和 `--target x86_64-pc-windows-gnullvm --examples --tests` 都干净。
- Surface（`package.sh settings` → `C:\dev\dianmo-dist\settings\Dianmo`，exe 1373KB，导入表只有系统 DLL；`crates/dianmo/tests/surface/run-settings.sh <dist> <本地目录>`，测试副本 `--instance Test --no-elevate`，全程 17 秒，用户自己的点墨没动）：
  - 首次启动（`--hidden`，`onboarded=false`）319ms 后引导窗口出现；→ 键翻到第 2、3 屏；Esc = 跳过 → 窗口关闭、`onboarded=true`。
  - 第二个 `dianmo.exe --settings` 退出码 0，日志 `asked it to open the settings (delivered: true)`，运行中的实例打开设置窗口（工作区被用户的键盘占了一部分，客户区 960×533 DIP）。
  - 点「语音球」卡片 → `input_mode=voice`（日志 `voice mode hint`）；点「键盘」→ `input_mode=keyboard`；End 后点主题「深色」→ `theme=dark`，窗口和标题栏变深色（截图）；「浅色」→ `theme=light`；键盘页高度滑块点到 120% → `height=1.2`；Esc 关闭设置窗口；WM_CLOSE 退出码 0。
  - 截图（窗口区域，200%）：`docs/screenshots/settings-general.png`、`settings-general-dark.png`、`settings-keyboard.png`、`settings-voice.png`、`settings-voice-dark.png`、`about.png`、`onboarding.png`、`onboarding-layout.png`、`onboarding-voice.png`。实机上微信输入法「已就绪」，豆包输入法显示「没有设置『免按模式』语音快捷键…」，MDL2 新图标（⚙ E713、输入 E8D2、关于 E946、引导里的 E8BD / E765 / E720 / E7F8）都显示正常。

**没测 / 遗留**
- 真机上没点：一键修复（会弹 UAC）、开机自启开关、选择豆包语音程序（文件对话框）、检查更新 / 立即更新、导出诊断包、反馈问题、打开链接、恢复默认、跟随系统时切换 Windows 深浅色、键盘工具栏 ⚙、托盘新菜单（托盘图标在溢出区，测试脚本点不到）。逻辑都在服务器上测了映射，Windows 代码只做了 check / clippy。
- 语音球的「出来 + 气泡」（`reveal_ball`）这次截图被用户自己的键盘挡住了，只有日志。
- 托盘图标的「有新版」小红点没做（只改了菜单文字）。
- 候选字号、全角标点、空格上屏首选、模糊音、按键音、用户词库导入导出清空：界面已接，功能下个版本。
