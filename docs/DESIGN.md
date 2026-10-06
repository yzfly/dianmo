# 点墨 Dianmo · 设计

> 指尖一点，落字成墨。让 Surface 等 Windows 触屏设备上的中文输入，像手机输入法一样顺手。

需求来源见 `TODO.md`。本文是实现的依据；改设计先改这里。

## 1. 问题

Surface 拆掉键盘后只剩触屏，而 Windows 的输入体验是为实体键盘设计的：

- 系统触摸键盘又宽又大，两只手要大范围移动；没有九宫格；第三方输入法的候选框是给鼠标用的小浮窗，手指很难点中。
- 豆包、微信等成熟产品的语音输入，要靠长按右 Alt、Ctrl+Win 这类**实体键**触发，纯触屏用不了。
- 没有现成的开源方案；安卓输入法也不能在 Windows 上跑（WSA 已停止支持）。

手机屏幕更小，输入却更好用，靠的是：候选词就在键盘上、按键有反馈、手势多、九宫格和双拼、一键语音、键盘弹出时页面自动让位。点墨要把这些搬到 Windows 上。

## 2. 体验设计（对标手机输入法）

**形态**
- 默认**停靠在屏幕底部、全屏宽**，横屏时高度约占屏幕的 38%（可调）。之后会加悬浮和左右分体（拇指模式）两种形态。
- **不遮挡**：键盘注册为 AppBar，占用屏幕底部的工作区，最大化的窗口会自动缩上去，就像手机把页面顶上去。
- **不抢焦点**：点键盘不会让目标应用失去焦点和光标。

**候选条**（键盘最上面一行）
- 左边是预编辑区，显示正在输入的拼音（如 `ni hao`），右边是横向滑动的候选词，最右边是展开箭头，点开是全屏候选网格。
- 没在输入时，候选条换成工具栏：语音、布局切换、符号、剪贴板（以后加）、收起键盘。

**布局**
| 布局 | 说明 |
|---|---|
| 26 键全拼 | 默认 |
| 26 键小鹤双拼 | 键面小字标注韵母，方便记忆 |
| 九宫格拼音 | 左侧一列显示可能的拼音（选了就锁定音节）和常用标点 |
| 英文 26 键 | Shift 单击是下一个字母大写，双击锁定大写 |
| 数字九宫格 / 符号面板 | 中文模式下默认全角标点 |

**手势与反馈**
- 按下时弹出放大的气泡；长按或上滑一个键，输入它右上角标的数字或符号（q→1 …）。
- 删除键：长按连续删除，向左滑清空正在输入的拼音。
- 空格键：左右滑动移动光标；输入时按空格上屏第一个候选。
- 回车：输入中按回车，原样上屏拼音或字母；没在输入时就是普通回车。
- 输入中点标点，先上屏第一个候选，再输入这个标点。

**Surface 宽屏布局（TODO #19，2026-10-06）**

Surface 横屏是 1440×960 DIP（13 英寸，1 DIP≈0.19mm）。手机布局拉到全宽后，每个键约 95 DIP（18mm），比实体键还宽，宽度全浪费在把键拉宽上。宽屏时（键盘宽度 ≥ 1100 DIP）参照 iPad Pro 12.9 的大屏键盘，把空间用来放更多的键；窄屏和竖屏仍用手机布局。
- **26 键（全拼 / 小鹤 / 英文）改五行**：
  - 第一行是常驻数字行：`1–0`，加 `-`、`=`、⌫；长按或上滑出 `！@#¥%…&*（）` 等。
  - 第二行：`Tab q–p`，加中文 `【】`（英文 `[]`）。
  - 第三行：`a–l`，加 `；'`、⏎。
  - 第四行：两侧 ⇧，中间 `z–m`、`，。/`（中文全角）。
  - 底行：符号、`Ctrl`、布局、空格、中/英、收起。
  - 字母键角标从数字改成符号，数字不用再靠长按。
- **数字面板（123）**：数字区限宽、居中（123 在上，同手机）；左右两侧放常用运算符和标点（`+ - * / % ( ) : ,`），再加 Tab、方向键。宽度要固定在合适大小，不拉伸。
- **九宫格**：九宫格是拇指布局，宽屏下限宽，多出来的宽度放符号列和一个数字小键盘。
- **候选条**：
  - 字号比手机大，约 24 DIP，首选更醒目；
  - 宽屏一行能放下更多候选；
  - 简拼时首选旁边用小字注释全拼；
  - 展开后的候选网格格子更大，可以翻页。
- **快捷键**：
  - 不在输入时，候选条换成编辑工具栏：撤销、重做、全选、剪切、复制、粘贴、← →（长按到行首、行尾），右边是语音、布局、表情、收起。
  - 底行的 `Ctrl` 是粘滞键：点一下再点字母，就发出 Ctrl+字母（如 Ctrl+S、Ctrl+F、Ctrl+Z）。双击锁定。
  - Tab、Esc（长按 Tab）可以直接用。
- 实现：`dianmo_core` 新增快捷键动作（如 `Action::Shortcut`），由 `TextSink` 发组合键；先上屏正在组字的内容，再发快捷键。

**语音**
- 麦克风键（工具栏上，也可以长按空格）调起 **Windows 自带的语音输入**（Win+H），不另外装模型，也不额外占性能。
- 离线语音（如 sherpa-onnx + SenseVoice）放到以后，作为可选项。

**自动弹出 / 收起**
- 输入框一获得焦点，键盘自动出现，离开输入框就收起（用 UI Automation 监听焦点）。
- 屏幕边缘有一个小把手，托盘也有图标，可以手动呼出。
- 关掉 Windows 系统触摸键盘的自动弹出，避免两个键盘同时出来。

## 3. 技术选型（2026-10-06 调研后定稿）

调研了两个方向（界面框架、输入架构），结论和理由如下；冒烟测试已在 Surface 上通过。

**语言：Rust**。没有运行时，内存小，启动快，符合「Surface 性能有限」（TODO #10、#14）。

**界面：原生 Win32 + Direct2D / DirectWrite（`windows` crate 0.62.2），动画以后用 DirectComposition**
- 窗口过程完全自己写，不抢焦点的做法可控：`WS_EX_NOACTIVATE|WS_EX_TOPMOST|WS_EX_TOOLWINDOW`，`WM_MOUSEACTIVATE` 返回 `MA_NOACTIVATE`，`WM_POINTERACTIVATE` 返回 `PA_NOACTIVATE`，显示时用 `SW_SHOWNOACTIVATE`。
- `WM_POINTER` 原生多点触控，两只拇指同时按也不会丢键。
- DirectWrite 渲染中文清晰；`GetMessage` 阻塞，空闲时 CPU 为 0。
- 冒烟测试结果：exe 264KB，私有内存约 45MB，空闲 CPU 0，前台窗口不变。
- 不选的方案和原因：
  - Slint、egui：底层 winit 不处理 `WM_MOUSEACTIVATE`，而且把第一根手指当鼠标，第二根手指按下时会让第一根「抬起」，双拇指打字会出错。
  - Tauri / WebView2：一点就抢焦点，还要额外 100–200MB 内存。
  - Freya、GPUI：需要 MSVC 或 Skia。

**输入架构：近期用「键盘进程 + `SendInput` 上屏」，长期加一个轻量的 TSF 客户端**
- **M1–M3**：组字只在点墨的候选条里进行，上屏用 `SendInput(KEYEVENTF_UNICODE)`；退格、回车、方向键发真实的虚拟键（方向键要加 `KEYEVENTF_EXTENDEDKEY`）。Win32、UWP、Chromium、Electron、Office、终端都能用。
  - 限制：管理员权限的窗口收不到（UIPI 限制）；只读扫描码的程序（游戏、远程桌面）不行；没有内嵌在应用里的预编辑。
- **长期**：写一个很薄的 Rust TSF 输入法 DLL（参考 windows-chewing-tsf），只当客户端，librime 仍然跑在点墨进程里。好处：
  - 应用里能显示内嵌预编辑；
  - 能拿到可靠的「输入框获得焦点」信号，以及输入框类型（数字、密码、网址）；
  - 管理员窗口也能用。

  所以现在的代码要按「点墨进程是服务端」来写。
- 不选「复用小狼毫 Weasel」：它的候选词拿不到点墨的键盘上，除非 fork 它的 C++ 代码。

**引擎：librime 1.17.0 官方 MSVC x64 版（`rime-33e7814-Windows-msvc-x64.7z` 里的 `rime.dll`）**
- 这个 `rime.dll` 已经静态链接了 lua、octagram、predict 插件，只依赖系统 DLL。
- 运行时用 `LoadLibrary` 加载，再调 `rime_get_api()` 拿函数指针表；绑定代码自己写，不用现成的 crate（都不活跃）。注意设置 `data_size`，返回的对象只能用 librime 自己的 `free_*` 释放。
- 方案来自雾凇拼音 rime-ice：`rime_ice`（全拼）、`double_pinyin_flypy`（小鹤）、`t9`（九宫格）。
  - rime-ice 的 t9 方案从 PR #1451 起依赖 iOS 专有的 `t9_processor`，要用之前的版本（2025-01-14），或者去掉这个处理器。
  - 九宫格的键盘显示要用候选注释里的拼音，不显示数字。
- 词库在打包时预编译，首次启动不用等；OpenCC 数据一起发。

**语音：Windows 自带的语音输入**
- 一次 `SendInput` 发出 Win↓ H↓ H↑ Win↑。点墨不抢焦点，所以文字会进入目标输入框。
- 依赖：要联网，要打开「在线语音识别」，当前输入语言要是中文。这台 Surface（Win10 LTSC）已装中文语音组件 `Language.Speech~~~zh-CN`，在线语音识别也已开启（2026-10-06 确认）。

**自动弹出（M2）**
- 用 UI Automation 的焦点变化事件（放在 MTA 线程上，用 CacheRequest 一次取齐控件类型、只读、密码等属性），并且只在约 500ms 内刚有过触摸时才弹出，和系统键盘的规则一致。
- 关掉系统触摸键盘的自动弹出：Win10 设 `HKCU\Software\Microsoft\TabletTip\1.7\EnableDesktopModeAutoInvoke=0`，Win11 设 `TouchKeyboardTapInvoke=0`。不要停用 TabletInputService，Win+H 依赖它。
- AppBar 只在键盘显示时注册，退出时一定要发 `ABM_REMOVE`；Explorer 重启（`TaskbarCreated` 消息）和屏幕旋转后要重新注册。

**输入法冲突**：目标应用里如果开着微软拼音、搜狗，`SendInput` 发 Unicode 字符不会触发它们组字，基本不受影响。以后可以在中文语言下加一个美式键盘布局，键盘显示时切过去作兜底，这个待验证。

## 4. 代码结构

```
crates/
  dianmo-core/   纯逻辑   Engine / TextSink 接口、InputController（手机输入规则，唯一实现处）
  dianmo-ui/     纯逻辑   键盘界面：布局、候选条、手势、气泡；通过 Canvas 画，通过 View 被驱动
  dianmo-rime/   Windows  librime 动态加载 + Engine 实现；方案和词库的准备、预编译
  dianmo-win/    Windows  窗口宿主（不抢焦点、WM_POINTER、DPI）、Canvas 的 D2D 实现、SendInput、Win+H、AppBar、系统键盘设置
  dianmo/        Windows  主程序：把上面几块组装起来，托盘
data/rime/       方案、词库（脚本拉取，不进 git）
```

**模块之间的接口**（以代码为准）
- `dianmo_core::Engine` / `TextSink` / `InputController` / `Action`。
- `dianmo_ui::Canvas`：圆角矩形、文字、裁剪；坐标都是 DIP。`dianmo_ui::View`：resize / paint / pointer / timer / set_input_state …，返回 `Response { repaint, actions, timer_ms }`。

**宿主的事件循环**：触摸事件 → `View::pointer` → `UiAction::Input(a)` → `InputController::handle(a)` → `View::set_input_state(...)` → 需要时重绘。

依赖方向：`dianmo` → 其余四个；`dianmo-win`、`dianmo-rime`、`dianmo-ui` → `dianmo-core`；`dianmo-win` → `dianmo-ui`（只用 Canvas / View 接口）。

## 5. 性能（硬约束）

- 空闲时 CPU≈0：没有轮询，没有常驻动画，焦点监听用事件。
- 内存：私有内存目标 < 80MB（含 librime 和词库）。
- 按键到界面更新 < 16ms。
- 构建在 Surface 上进行：低优先级、`-j 2`，打包后清理 `target`。

## 6. 构建与开发流程

- **工具链**：
  - Surface：Rust stable `x86_64-pc-windows-gnullvm` + llvm-mingw（`C:\dev\tools\llvm-mingw`，提供 clang、lld、dlltool），不装 Visual Studio。`.cargo/config.toml` 开启 `+crt-static`，exe 只依赖系统 DLL。
  - 服务器：只做快速检查。纯逻辑 crate 用 `cargo test`；Windows crate 用 `cargo check --target x86_64-pc-windows-gnullvm`，不链接。
- 代码在服务器 `~/yzfly/dianmo`（唯一真源），Surface 只作构建和测试机，工作目录是 `C:\dev\dianmo-<名字>`。
- `scripts/surface/`：
  - `ps.sh`：在 Surface 上执行 PowerShell。
  - `sync.sh <名字>`：同步代码。
  - `build.sh <名字> [cargo 参数]`：同步后低优先级构建，排队执行。
  - `gui.sh [超时]`：在用户的桌面会话里执行，排队执行。
  - `shot.sh <out.png>`：截屏。
  - `clean.sh <名字> [--target]`：清理工作目录或 target。
- 用户正在用这台 Surface：GUI 测试要短，测完关掉自己开的窗口，不能留下错误弹窗。

## 7. 里程碑

- **M1 能用**：底部停靠的键盘窗口 + 26 键全拼 / 小鹤双拼 + 候选条 + 上屏 + 删除、空格、回车规则 + 语音键（Win+H）+ 托盘手动呼出。
- **M2 好用**：九宫格、数字和符号面板、长按和滑动手势、按键气泡、自动弹出和收起、AppBar 让位、关掉系统键盘的自动弹出。
- **M3 完整**：设置界面（布局、高度、主题、按键音）、悬浮和分体形态、开机自启、安装包、候选网格、剪贴板、可选的离线语音。
