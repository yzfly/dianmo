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

## 语音模块（TODO #27 / #29，2026-10-06，`src/voice.rs`，还没接到 app.rs）
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
