# dianmo-rime 状态（2026-10-06，引擎完成，Surface 实测通过）

librime 1.17.0（`rime.dll` 运行时加载）+ rime-ice（`da1fbe6`）。`RimeEngine` 实现 `dianmo_core::Engine`，打包时预编译，运行时直接用预编译结果。

## 代码

| 文件 | 内容 |
|---|---|
| `src/ffi.rs` | `rime_api.h` 的 `#[repr(C)]` 镜像（98 个函数槽，布局有测试） |
| `src/api.rs` | 加载 dll（`LoadLibraryExW` + `rime_get_api`，检查 `data_size` 和要用的函数槽）、进程级状态（`setup` 只调一次）、`Session` 安全封装（所有返回值复制成 Rust 值后用 librime 的 `free_*` 释放） |
| `src/engine.rs` | 公开 API：`Options`（`for_app()`、`in_dir(dir)`）、`Error`、`deploy()`、`DeployReport`、`RimeEngine`、`schema_id()`、`shutdown()` |
| `src/t9.rs` | 九宫格纯逻辑：数字↔字母、未确认部分定位、左栏拼音、显示用 preedit、选拼音后的新 input（`pick_input`） |
| `src/comment.rs` | 候选注释显示规则 |
| `examples/probe.rs` | `probe deploy <shared>`、`probe run ...`：测时延、内存和各项行为 |
| `scripts/rime/stage.ps1` | 组装随包文件（见下） |

`cargo test -p dianmo-rime`：9 个测试（服务器）。`cargo check/clippy --target x86_64-pc-windows-gnullvm`：无警告。

## 设计要点（已实测确认）

- **运行时**：`staging_dir = prebuilt_data_dir = <shared>\build`，不跑 maintenance，启动不编译、不写安装目录。`build\` 不完整时退回到 maintenance，编译到 `<user>\build`（实测 20 s，只是开发兜底）。
- **deploy**：删掉旧 `build\` 全量重编；用户目录用临时目录，prebuilt 指向不存在的目录。每个进程只能跑一次，且不能和 `RimeEngine` 同进程（主程序 `--deploy` 是单独进程，符合）。
- **用户目录**（`%APPDATA%\Dianmo\rime`）只会出现 `rime_ice.userdb`（学习词频，三个方案共用）和 `user.yaml`。空目录直接能用。
- **文本词库不用复制**：`custom_phrase.txt`、`en_dicts\cn_en.txt`、`cn_en_flypy.txt` 由 librime 直接从共享目录读（日志 `tsv.cc: reading tsv file: <shared>\...`）。`lua\lunar.db` 也从共享目录读。
- **非 ASCII 路径**：librime 1.17 在 Windows 上按 UTF-8 解析 `char*` 路径。实测中文用户目录、中文安装目录都正常（lua、opencc、userdb 都没问题）。卷上有 8.3 短路径时优先用短路径（这台 Surface 的 C: 没有）。
- **注释**：rime_ice 的「［拼音］」会被 corrector.lua 清掉，候选不带注释；`comment::display` 兜底去掉残留的［］。九宫格的拼音注释只用来生成 preedit 和左栏，不显示在候选上。
- **默认候选永远是 0 号**：退格撤销部分选词后，librime 会把高亮留在原来选的词上，preedit 只显示那一段；`current()` 发现高亮不是 0 就 `highlight_candidate(0)`，preedit 恢复成「jin tian tian qi hen hao」。
- **预热**：选方案后的第一键要 30–50 ms（librime 懒加载词典、Lua）。`start` 和 `set_schema` 里先按一个键再清掉，把这段时间挪到启动/切方案里，用户的第一键只要 2–3 ms。
- **Engine 映射**：`input` → `process_key`；`backspace` → BackSpace（选过词时撤销上一次选词）；`select` → `select_candidate`（绝对下标，部分选词留下剩余部分）；`commit_raw` → 全拼/双拼发 Return（rime-ice 绑定 `commit_raw_input`：已选的字 + 剩余原文，如「今天tianqihenhao」），九宫格上屏 preedit 显示的拼音再清空；`candidates` → `candidate_list_from_index`；快照里放前 30 个候选。
- **九宫格选拼音**：`pick_t9_spelling` 用 `set_input` 改写 input（`64426` → `ni'426`）。librime 会保留改动位置之前已确认的分段，所以先部分选词再选拼音也正常（`64426` 选「你」→ 选 hao → input `64hao`、preedit「你hao」）。原计划的 `set_caret_pos` + Delete 方案不需要了。

## Surface 实测（Surface Pro 9 i7-1265U，release 版，从 stage.ps1 组装的目录运行）

| 项目 | 结果 |
|---|---|
| deploy（预编译） | 18.6–21.3 s，`build\` 42.6 MB |
| 启动（加载 dll + initialize + 会话 + 预热） | 热缓存 150–165 ms；冷缓存 320–670 ms |
| 第一键 | 2.4–3.5 ms |
| 每键（全拼 jintiantianqihenhao ×20） | 平均 1.8–2.6 ms，p95 3.2–4.2 ms，最大 4.9 ms |
| 每键（小鹤 / 九宫格） | 平均 1.5–2.7 ms / 0.9–1.4 ms，最大 < 6 ms |
| 切方案（含预热） | 64–165 ms |
| `t9_spellings` | 0.3–0.9 ms |
| 私有内存 | 启动后 7.5 MB，测完 8.7–9.7 MB（词典是只读 mmap，不算私有）；工作集 15–16 MB |

行为（每组只看前 3 个候选）：
- `nihao` → 你好 / 👋 / 拟好；退格 → preedit「ni ha」；commit_raw → `niha`；select(0) → 上屏「你好」
- `nh` → 你好 / 👋 / 女孩
- `jintiantianqihenhao` → 今天天气很好 / 今天天气 / 今天；部分选「今天」→ preedit「今天tian qi hen hao」，候选 天气很好…；退格 → 撤销选词；commit_raw → `今天tianqihenhao`
- 小鹤 `nihc` → 你好 / 👋 / 拟好；commit_raw → `nihc`
- 九宫格 `64426` → preedit「ni hao」，候选 你好 / 👋 / 你敢；左栏 `ni mi m n o`；选 ni → input `ni'426`，左栏 `hao gan gao han g h i`；再选 hao → `ni'hao`；commit_raw → `nihao`
- 候选总数：librime 本身给的，`n` 731 个、`ni` 87 个、`nihao` 30 个、`jintiantianqihenhao` 50 个；分页取到的结果和一次取的一致。注意 `nihao` 这种完整音节输入 librime 只给 30 个，候选网格翻不出更多，这是 librime 的行为，不是封装的问题。
- 日志：运行时只有一条「user.yaml 不存在」的警告（首次运行，正常）。deploy 有 rime-ice 自带的警告（melt_eng 循环依赖提示、英文词库重复词条），不影响使用。

## 随包文件（stage.ps1 产出，共 48.1 MB，44 个文件）

```
<Out>\rime.dll                         3.6 MB
<Out>\data\rime\build\                 42.6 MB  预编译结果（17 个文件）
    default.yaml, rime_ice / double_pinyin_flypy / t9 / melt_eng / radical_pinyin .schema.yaml
    rime_ice.table.bin 27.7 MB, melt_eng.prism.bin 8.0 MB, radical_pinyin.*.bin 5.5 MB, 其余 < 1 MB
<Out>\data\rime\lua\                   0.8 MB   rime-ice 的 Lua 脚本 + lunar.db
<Out>\data\rime\opencc\                1.1 MB   emoji、简→繁
<Out>\data\rime\custom_phrase.txt, en_dicts\cn_en.txt, en_dicts\cn_en_flypy.txt
<Out>\data\rime\RIME_ICE_COMMIT
```
方案和词库的源 yaml（`*.schema.yaml`、`*.dict.yaml`、`cn_dicts\`、`symbols_*.yaml`、`default*.yaml`）都不发；实测只靠上面这些文件就能正常运行，日志里没有缺文件。

## stage.ps1 用法

```powershell
powershell -ExecutionPolicy Bypass -File scripts\rime\stage.ps1 -Out <dir> [-Data C:\dev\dianmo-data] [-Probe <probe.exe>] [-Verify]
```
- `<Data>` 是 `fetch.ps1` 的产物。预编译结果放在 `<Data>\rime\build`：带 `-Probe` 时，如果 build 缺失或比任何源 yaml/txt 旧，就跑 `probe deploy` 重编（约 20 s，低优先级）；不带 `-Probe` 时要求 build 已是最新，否则报错。
- 会整个替换 `<Out>\data\rime`，`<Out>` 里的其他文件不动。带命名互斥锁，多个人同时调用会排队。
- `-Verify`（需要 `-Probe`）：对组装好的目录跑一遍 `probe run`。
- probe 的构建：`scripts/surface/build.sh rime build --release -p dianmo-rime --example probe`，产物在 `C:\dev\dianmo-rime\target\release\examples\probe.exe`（导入表只有系统 DLL；rime.dll 只依赖 dbghelp/kernel32/user32）。
- 注意：`build.sh` 同步代码时会清空工作目录里除 `target\` 外的所有东西，`-Out` 不要放在工作目录下（可以放到 `target\` 里或别处）。

## probe 用法

```
probe deploy <shared> [--dll rime.dll] [--log <dir>]
probe run [--shared <dir>] [--user <dir>] [--dll rime.dll] [--log <dir>] [--bench N]
```
`--dll` 默认是 exe 旁边的 rime.dll；`run` 不给 `--user` 时用一个新的临时目录（测的就是「空用户目录、不跑 maintenance」），结束后删掉。

## 遗留 / 以后可做

- 切方案要 64–165 ms（librime 每次切换都会重建方案的组件、重新映射词典）。如果以后频繁切换，可以给每个方案各开一个会话。
- 安装包体积：`melt_eng.prism.bin`（8 MB，英文混输）和 `radical_pinyin`（5.5 MB，部件拆字辅码）可以考虑砍掉，但要同时改方案配置，目前保持 rime-ice 原样。
- `shutdown()` 可选：退出前调用可以干净地关闭用户词典（LevelDB 不调用也不会丢数据）。
- 不要 cat、grep 词库和词表文件（*.dict.yaml、custom_phrase.txt、opencc、lua、en_dicts），只看文件名、大小、行数。之前两次因为内容过滤被中断。
