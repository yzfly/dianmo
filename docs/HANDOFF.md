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
| `dianmo-core` | ✅ 已提交 | `Engine` / `TextSink` 接口、`InputController`（手机输入规则），11 个测试在服务器和 Surface 上都通过 |
| `dianmo-ui` | 🔄 子 agent 开发中 | 键盘界面。接口 `canvas.rs` / `view.rs` 由主会话定义，已提交；其余见 `docs/status/dianmo-ui.md` |
| `dianmo-win` | 🔄 子 agent 开发中 | 窗口宿主、D2D Canvas、SendInput、Win+H、托盘、AppBar。见 `docs/status/dianmo-win.md` |
| `dianmo-rime` | 🔄 子 agent 开发中 | librime FFI 已写好，数据已下到 Surface。前两个 agent 因 API 内容过滤报错中断过，见下文。状态见 `docs/status/dianmo-rime.md` |
| `dianmo`（主程序） | ⏳ 未开始 | 主会话负责组装：`dianmo_win::run(KeyboardView, App)`，App 里放 `InputController<RimeEngine, SendInputSink>`；打包到 `%LOCALAPPDATA%\Dianmo` + 桌面图标「点墨」（已经跟用户说过这个位置） |

各子 agent 收尾时会把进度写到 `docs/status/<crate>.md`。那几个文件和它们改过的代码在第一阶段**没有提交**；下个会话先 `git status` 看一下，确认能编译以后再提交。

## 下一步

1. 看 `docs/status/*.md`，把三个 crate 补到能用（M1，DESIGN §7）。
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
- **待清理**：见 `TODO.md` 末尾（计划任务 `DianmoGui`、`C:\Users\wecode\claude`、R2 上的 `share/surface-setup-v3.ps1`）。
- **提交**：git 身份用 `yzfly <zphyix@gmail.com>`，提交信息不加 Co-Authored-By，也不加生成标记。仓库目前只在本地，还没有远程仓库。
