# 点墨 Dianmo · 给 AI 编程助手的约定

- 先读 `TODO.md`（用户需求，唯一来源）和 `docs/DESIGN.md`（设计与技术选型）。设计要改先改 DESIGN.md。
- 核心目标：让 Surface（Windows 触屏、无实体键盘）上的中文输入像手机输入法一样好用。性能是硬约束：空闲 CPU≈0、内存小。
- Rust workspace。纯逻辑 crate（dianmo-core、dianmo-ui）在服务器上 `cargo test`；Windows crate 在服务器上只做 `cargo check --target x86_64-pc-windows-gnullvm`，真正构建和测试在 Surface 上（`scripts/surface/`）。
- 服务器 4 核 8G、磁盘紧：cargo 命令用 `flock /tmp/heavy.lock nice -n 10 cargo ...`。
- Surface 是用户正在用的机器（Win10 LTSC 21H2，2880x1920，200% 缩放）：
  - 构建用 `scripts/surface/build.sh <自己的名字> ...`（会自动排队、低优先级）；不要在 Surface 上开其它常驻进程。
  - GUI 测试用 `gui.sh` / `shot.sh`，要短，测完关掉自己启动的程序，不要留错误弹窗，不要动用户的其他窗口。
- 不要 commit，由主会话集成后提交。提交信息不加 Co-Authored-By、不加生成标记。
