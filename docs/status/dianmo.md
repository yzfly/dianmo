# dianmo 主程序与打包 · 状态（2026-10-06）

## 已完成并在 Surface 实测（Win10 LTSC 21H2，2880x1920，200%）
- **主程序 `crates/dianmo`**（`dianmo.exe`，windows 子系统，621KB，只依赖系统 DLL，嵌了「墨」图标和版本信息）：
  - `dianmo_win::run_with(KeyboardView, DianmoApp, HostOptions)`；`DianmoApp` 里是 `InputController<AnyEngine, SendInputSink>`。
  - 事件循环：`UiAction::Input` → `controller.handle` → `view.set_input_state`（返回的 Response 交给宿主继续处理）；`WantMoreCandidates` → `engine.candidates` → `set_more_candidates`；`WantT9Spellings` → `set_t9_spellings`；`Voice` → Win+H；`Hide` → `host.hide()`；`ThemeChanged` → 存设置。启动时 `SetSchema(设置里的方案)`，中英也按设置恢复。
  - **引擎**：窗口先用内置的「字母原样上屏」引擎起来（候选注释写「词库加载中」），`on_start` 里开后台线程（先等 250ms，免得 rime.dll 的加载锁拖慢窗口的 D2D 初始化）启动 `RimeEngine`，好了以后 post 回 UI 线程换上（正在组字时等组完再换；换时方案不一致会补一次 SetSchema）。`rime.dll` 不存在或启动失败就一直用内置引擎，候选注释和托盘菜单「关于」里写「词库未加载（原因）」。退出时先关 session 再 `dianmo_rime::shutdown()`。
  - **系统触摸键盘**：启动时把原值写进 `settings.ini`（`saved_*`）**之后**才把 `EnableDesktopModeAutoInvoke`、`TouchKeyboardTapInvoke` 设为 0；正常退出 / WM_ENDSESSION（App 的 Drop）/ panic hook 都会恢复；异常退出后下次启动发现 `saved_*` 还在，就把它当原值。实测：运行中 0/0，退出后恢复为原来的 1/（不存在）。
  - **单实例**：命名 mutex `Local\Dianmo.SingleInstance`；第二次启动找到消息窗口 `DianmoInstance`（HWND_MESSAGE）发消息，已有实例 `HostProxy::show()`，自己退出。
  - **设置** `%APPDATA%\Dianmo\settings.ini`（key=value，临时文件 + rename 写入）：schema、theme、chinese、appbar、autostart、auto_show、height，外加 `saved_*`。开机自启以注册表为准（`HKCU\...\Run\Dianmo = "<exe>" --autostart`；exe 换位置后启动时自动改过来）。
  - **托盘菜单**（App 项在宿主自带的「显示/隐藏、让出屏幕空间、退出」上面）：全拼 / 小鹤双拼 / 九宫格、深色主题、键盘高度 ▸（矮 0.85 / 标准 / 高 1.15 / 更高 1.3）、点输入框时自动弹出、开机自动启动、关于（版本 + 词库状态）。改高度会隐藏再显示一次以重新停靠。
  - **自动弹出/收起**（`start_focus_watcher`）：`Editable{by_touch:true}` → 显示；`Number` 输入框自动切到数字面板，离开后切回字母；`NotEditable{by_touch:true}` → 350ms 后收起（期间有新的焦点事件或手动显示/隐藏就取消）；`by_touch:false` 不弹不收。手动呼出（托盘、把手、再次启动）后 1.5s 内不因「非输入框」收起（点托盘图标本身会把焦点给任务栏）。键盘隐藏时清掉未上屏的组字。
  - 日志 `%APPDATA%\Dianmo\dianmo.log`（追加，>512KB 时轮换为 `.old`），从不弹对话框。
  - 参数：`--hidden` / `--autostart`（隐藏启动）、`--deploy <shared_dir>`（调 `dianmo_rime::deploy`，单独进程）、`--version`。环境变量 `DIANMO_ENGINE=basic` 强制内置引擎；`DIANMO_KEYMAP=<文件>` 每次显示时写出按键中心坐标（测试用）。
  - feature `rime`（默认开）；`--no-default-features` 可不带 dianmo-rime 编译。
- **实测数据**（`crates/dianmo/tests/surface/run.sh`，向经典记事本注入真实触摸）：
  - 从启动进程到键盘可见 171–264ms；librime 在启动后约 0.45–0.7s 就绪（start 本身 176–451ms）。
  - 私有内存：带 rime 24–28MB（工作集约 60MB）；不带 rime 15.6MB。空闲 10 秒 CPU 增量 0ms。
  - 全拼 `nihao`+空格 → 你好；`women`+2（选第 2 个候选）→ 我们；小鹤 `nihc` → 你好；九宫格 `64426` 出「你好」，左列 ni/mi/m/n，选 mi 后锁定。全程记事本保持前台。
  - 单实例、隐藏键再启动呼出、WM_CLOSE 退出后工作区和系统键盘设置都恢复。
  - 自动弹出：点记事本输入区 → 弹出；点任务栏 → 收起。
  - 托盘：切深色/浅色、开机自启（写入 Run）实测生效。
  - 高度 1.3：键盘高 869px（标准 668px）。

## 打包与安装
```
scripts/surface/package.sh <name> [--install] [--start] [--keep-target] [--no-build]
scripts/surface/package.sh <name> --stop        # 只用 WM_CLOSE 关掉运行中的点墨
scripts/surface/package.sh <name> --uninstall   # 关掉、删快捷方式/开机自启/%LOCALAPPDATA%\Dianmo（保留设置和日志）
```
1. `build.sh <name> build --release -p dianmo`（同步 + 低优先级构建）；
2. `llvm-objdump -p` 检查导入表只含系统 DLL；
3. 组装到 `C:\dev\dianmo-dist\<name>\Dianmo\`（不放在工作目录里：sync 会清空工作目录）：`dianmo.exe` + `scripts\rime\stage.ps1 -Out`（rime.dll + data\rime，含预编译 build；target 里有 probe.exe 时带 `-Probe`，build 过期会重建）。共 46 个文件、48.7MB；
4. `--install`：在桌面会话里用 WM_CLOSE 关掉正在运行的点墨（等最多 8 秒，关不掉就放弃安装，不强杀），robocopy /MIR 到 `%LOCALAPPDATA%\Dianmo`，桌面和开始菜单建「点墨」快捷方式；
5. `--start`：经 explorer 以普通权限启动安装好的点墨；
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
- `scripts/surface/package.sh` 的导入表白名单是手写的，dianmo-win 以后新增系统 DLL 依赖时要加进去。
