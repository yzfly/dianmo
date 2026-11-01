# 最新交接：可选双击唤起键盘（2026-10-31）

> **已完成开发（2026-10-31，同日后续会话）**：代码、文档、T9 测试与构建、Surface 短时验收都已完成，结果和未验证项见 `docs/status/dianmo.md`「单击 / 双击打开键盘」。已作为 v0.2.3 提交、发布；没有替换 Surface 上的现用安装（它会自己检查到更新）。下文是开发前的交接说明，保留作记录。

用户先要求把点墨拉到本地并开发该功能，随后明确改为：**本会话不开发，只记录并给提示词，由另一个 agent 开发。** 此功能待接手实施，不代表整个历史路线图重新启动。

## 代码与参考

- 正式本地目录：`/home/ubuntu/workspace/dianmo`，origin 为 `https://github.com/yzfly/dianmo.git`。本轮基线为 main 的 `39d0f92`；该提交只在 README 登记社区改版参考。
- 需求来源：[issue #1](https://github.com/yzfly/dianmo/issues/1)。该 issue 已按「参考代码分享已登记」关闭，**不是功能已开发或已发布**。
- 社区源码：[Enderlessss/dianmo-branch-doubleclickfeaturetest](https://github.com/Enderlessss/dianmo-branch-doubleclickfeaturetest)。作者报告在自己的 Surface 上测试过；维护者只做了源码差异审阅，未独立构建/真机验收。
- 本次仅在 TODO.md 和本交接文档留下待办，尚无功能代码修改，也没有新安装包。

## 开发目标与验收

1. 设置中增加单击/双击选择，默认单击，兼容已有配置；重启后设置保留。
2. 双击模式下，一次触摸不弹键盘，同一输入框内两次有效触摸才唤起；点击不同输入框、超时、隐藏键盘或切换模式时重置状态。
3. 键盘已显示时，单击其他输入框仍正常更新焦点与网址/邮箱/数字提示，不要求再次双击；保留语音球、手动唤起及现有自动隐藏行为。
4. 同一次物理触摸产生的 UIA、raw touch、合成鼠标事件不能算成两次。参考实现用 field id、tap 序号、40–500 ms 时间窗口，不能只照搬而不核验事件去重。
5. 补充分离于 Win32 的判定测试、配置读写/默认值测试，并验证 Windows 触摸事件；至少覆盖相同/不同输入框、重复事件、超时、旧配置、模式切换及键盘显示时的切焦点。
6. 同步设计文档和使用说明；提交可审阅代码与验证结果。当前是开发任务，不承接历史文档中的自动安装/发布授权。

## 已审阅的参考改动位置

- `crates/dianmo-ui/src/settings/model.rs`：`AutoShowClick` 设置枚举与默认值。
- `crates/dianmo-ui/src/settings/pages.rs`：设置项。
- `crates/dianmo/src/settings.rs`：`auto_show_click=single|double` 读写及测试。
- `crates/dianmo/src/app.rs`：双击状态、窗口可见性与 focus 事件接线。
- `crates/dianmo/src/app/settings_ui.rs`：设置应用与状态重置。
- `crates/dianmo-win/src/focus.rs`：焦点标识、触摸序号、re-tap 事件。参考分支删除了原 250 ms 防抖，要检查对默认单击模式的回归影响。

社区分支含参考测试与文档，但不要整仓覆盖主仓库。保留贡献来源说明，确认所有 FocusEvent 调用点与平台编译兼容。

## 当前设备约束

先读 `~/.claude/CLAUDE.md`、`~/.agents/test-devices.md`，设备以 `/home/ubuntu/workspace/jiangshu-infra/machines.toml` 为准，然后读本仓库 CLAUDE.md、TODO.md 与 docs/DESIGN.md。

- 2026-10-31 已探活 `ssh t9` 与 `ssh surface`，后续使用前仍探活。
- **最新全局约定：重型编译/lint/全量测试只在 T9，用 `flock /tmp/heavy.lock nice -n 10`。** 下方历史记录和旧脚本中的 Surface 构建、15570 连接、`~/yzfly/dianmo` 路径已过时，不直接照用。
- T9 已有 Rust 1.98.0、`x86_64-pc-windows-gnu` 目标；本次探测未列出 gnullvm 目标，也未找到对应 mingw 编译器命令。先核对 Windows 交叉工具链再构建；本会话没有安装工具链或启动构建。
- Surface 只安排本任务的短时原生 GUI/触摸验证，走正式 `ssh surface` 和设备文档中的工具；不要终止用户进程、替换现用安装或恢复旧远控任务。

---

> **2026-10-07 更新：开发已结束**。最新版本 v0.2.2 已发布（GitHub Releases）并装在 Surface。各模块状态见 `docs/status/*.md`，需求见 `TODO.md`；本地语音识别的半成品在本地分支 `wip/local-asr`。下文是第一阶段的交接记录，保留作历史。

# 交接：第一阶段收尾（2026-10-06）

下一个会话从这里接着做。用户会开一个 tmux 会话（SSH 可能断，tmux 里不怕断）：

```bash
tmux new -s dianmo          # 断了以后用 tmux attach -t dianmo 接回
cd ~/yzfly/dianmo && claude # 新会话：先读 CLAUDE.md → TODO.md → docs/DESIGN.md → 本文件 → docs/status/*.md
```

## 现在在哪

- **需求**：`TODO.md` 共 16 条，进行中的是 #7–#9（点墨本体）和 #16（做完让用户在 Surface 上试用）。
- **设计和技术选型已定稿**：`docs/DESIGN.md`。Rust；Win32 + Direct2D/DirectWrite；`SendInput` 上屏；librime 1.17 动态加载 + rime-ice；语音用 Win+H；以后加 TSF 客户端。
- **代码**（Rust workspace）：

| crate | 状态 | 说明 |
|---|---|---|
| `dianmo-core` | ✅ | `Engine` / `TextSink` 接口、`InputController`（手机输入规则），11 个测试通过 |
| `dianmo-ui` | ✅ M1 完成 | `KeyboardView`：全拼 / 小鹤（键面标韵母）/ 九宫格 / 英文 / 数字 / 符号，候选条可滑动、可展开，气泡、长按、上滑、删除连删、空格移光标，深浅主题；32 个测试。效果图在 `docs/previews/`。未在 Surface 上实际渲染过，雅黑字体度量和 MDL2 图标码位待确认。详见 `docs/status/dianmo-ui.md` |
| `dianmo-win` | ✅ Surface 实测通过 | 不抢焦点（记事本一直在前台），多点触控，D2D + DirectComposition（默认用 WARP 软件渲染，私有内存 9.6MB），空闲 CPU 为 0，exe 361KB，托盘，左边缘把手，Win+H，AppBar。详见 `docs/status/dianmo-win.md` |
| `dianmo-rime` | 🟡 一半 | FFI（98 个函数位已对过头文件）、九宫格拼音逻辑（7 个测试）、数据已下到 Surface（rime-ice `da1fbe6`）。**`RimeEngine`、预编译部署和 probe 还没写**，下一步见 `docs/status/dianmo-rime.md` |
| `dianmo`（主程序） | ⏳ 未开始 | 主会话负责：`dianmo_win::run_with(KeyboardView, App, HostOptions)`，App 里放 `InputController<RimeEngine, SendInputSink>`；启动时 `SetSchema`，关掉系统键盘自动弹出，退出时恢复；打包到 `%LOCALAPPDATA%\Dianmo` + 桌面图标「点墨」 |

第一阶段的全部代码都已提交，各模块的进度和下一步写在 `docs/status/<crate>.md`。

## 下一步

1. 写完 `RimeEngine` 和预编译部署，跑 probe 测时延和内存（见 `docs/status/dianmo-rime.md`）。
2. 写 `crates/dianmo` 主程序，在 Surface 上构建，组装数据：`rime.dll` + `data\rime` + 预编译好的词库。
3. 实测：记事本 / 浏览器里打全拼、小鹤，测删除、回车、语音键；看内存和空闲 CPU。
4. 安装到 `%LOCALAPPDATA%\Dianmo`，桌面放「点墨」图标，**通知用户试用**（TODO #16）。
5. 清理 Surface 上的构建产物（TODO #10）：`scripts/surface/clean.sh <名字> --target`。

## Surface 速查

- **连接**：反向 SSH 隧道。服务器上 `ssh -p 15570 wecode@127.0.0.1`。用户那边点桌面「Claude Remote Debug」图标连上，窗口要一直开着。隧道断了就请用户再点一次。
- **工具**：`scripts/surface/` 下有 `ps.sh`、`sync.sh`、`build.sh`、`gui.sh`、`shot.sh`、`clean.sh`，用法见 DESIGN §6。
- **机器**：Surface Pro 9，i7-1265U，16G 内存，Win10 企业版 LTSC 21H2，2880×1920，200% 缩放。
- **工具链**：Rust stable gnullvm（`%USERPROFILE%\.cargo`）+ llvm-mingw（`C:\dev\tools\llvm-mingw`）；.NET 已删。
- **数据**：librime 和 rime-ice 在 `C:\dev\dianmo-data\`；各 agent 的工作目录是 `C:\dev\dianmo-<名字>`。
- **语音**：中文语音组件已装，在线语音识别已开启。

## 注意

- **内容过滤**：rime 子 agent 两次被 API 内容过滤打断，很可能是把词库原文或长串候选读进了上下文。不要 cat 词库文件，测试输出每个输入最多打印前 3 个候选。
- **Surface 是用户正在用的机器**：GUI 测试要短，测完关掉窗口，启动新 exe 之前先用 `llvm-objdump -p` 查依赖，别再弹出缺 DLL 的报错框。
- **待清理**：见 `TODO.md` 末尾（计划任务 `DianmoGui`、`C:\Users\wecode\claude`、R2 上的一次性安装脚本）。
- **提交**：git 身份用 yzfly，提交信息不加 Co-Authored-By，也不加生成标记。仓库已公开在 GitHub `yzfly/dianmo`（origin）；`scripts/surface/surface-setup.ps1`、`build-setup.py` 含个人服务器信息，只留在本地（已 .gitignore），不要提交。
