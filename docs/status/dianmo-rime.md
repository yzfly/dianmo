# dianmo-rime 状态（2026-10-06，阶段性中断）

## 已完成并验证
- `src/ffi.rs`：librime 1.17.0 `rime_api.h` 的 `#[repr(C)]` 镜像。`RimeApi` 的 98 个函数槽已按头文件逐个比对，顺序一致；结构体布局有单元测试。
- `src/t9.rs`：九宫格的纯逻辑（字母和数字互转、未确认部分的定位、左栏拼音列表、显示用 preedit、回车上屏文本），7 个单元测试通过（`cargo test -p dianmo-rime`）。
- `src/lib.rs`：目前只导出 `ffi`、`t9`。`cargo check --target x86_64-pc-windows-gnullvm` 通过。
- `scripts/rime/fetch.ps1` 已在 Surface 上跑过，数据在 `C:\dev\dianmo-data\`：
  - `librime\`：`rime.dll`（3.7MB）、`rime_deployer.exe`、`rime_api.h`
  - `rime\`：rime-ice 的方案、词库、lua、opencc，以及 `default.custom.yaml`、`RIME_ICE_COMMIT`
  - 打包补丁：注释掉 tencent 大词库；从 t9 方案去掉 iOS 专有的 `t9_processor`（已确认 processors 里没有它了）
- rime-ice 锁定的提交：`da1fbe602e38f26db846fa10120ee64c2b0324c0`（2026-10-05）。
- `data/rime/default.custom.yaml`：方案列表 rime_ice / double_pinyin_flypy / t9，page_size 9。

## 未完成
1. 安全封装和 `RimeEngine`（实现 `dianmo_core::Engine`）还没写。
2. `deploy`（预编译）还没写，`build\` 也还没生成。
3. `examples/probe.rs` 还没写，所以 Surface 上还没有任何实测数据（耗时、内存都没有）。

## 下一步（已定的设计）
- **加载**：`LoadLibraryExW(rime.dll)` → `GetProcAddress("rime_get_api")`，检查 `data_size`。进程里只加载一次、只调一次 `setup`：glog 初始化两次会崩，用全局 `OnceLock` 保证。路径如果含非 ASCII 字符，先转成 `GetShortPathNameW` 短路径再传给 librime。
- **公开 API（计划）**：
  - `Options { dll, shared_data_dir, user_data_dir, log_dir, min_log_level }`；`Options::for_app()` 默认：dll = `<exe>\rime.dll`，共享数据 = `<exe>\data\rime`，用户数据 = `%APPDATA%\Dianmo\rime`。
  - `deploy(&Options)`：`deployer_initialize` + `deploy()`，用户目录指向临时目录，staging = `<shared>\build`，prebuilt 指向一个不存在的目录，强制全量编译。
  - `RimeEngine::start(&Options, Schema)`：`initialize`，不调 `start_maintenance`（直接用 `<shared>\build` 里的预编译结果）；`build\default.yaml` 不存在时才退回到 maintenance。之后 create_session + select_schema。
- **Engine 映射**：
  - 方案：Pinyin→`rime_ice`，Shuangpin→`double_pinyin_flypy`，T9→`t9`。
  - `input` → `process_key(ascii)`；`backspace` → 0xff08；`select` → `select_candidate`（绝对下标）。
  - `candidates` → `candidate_list_from_index` + `next`；首批候选 ≤30 个。上屏文字从 `get_commit` 取。
  - `commit_raw`：全拼、双拼发回车 0xff0d（rime-ice 回车绑定的是 `commit_raw_input`）；T9 上屏显示出来的拼音（`t9::raw_commit`），再 `clear_composition`。
  - T9 的 preedit 用 `t9::display_preedit`，显示首选候选注释里的拼音（`spelling_hints: 100`）。
  - `t9_spellings` 取前约 60 个候选的注释，交给 `t9::spellings`。
  - `pick_t9_spelling`：没有已确认的前缀时用 `set_input`；有已确认前缀时用 `set_caret_pos` + Delete + 逐个输入字母 + 把光标移回末尾。这一条要实测。
- **待实测**：
  - 不跑 maintenance、用户目录为空时能否正常出候选；
  - `custom_phrase.txt`、`en_dicts/cn_en.txt`（文本型用户词库）是否只在用户目录里找。如果是，首次启动时从共享目录复制过去；
  - rime_ice 的候选注释带「［］」，看 corrector.lua 会不会清掉，否则在封装层剥掉；
  - 私有内存要 < 80MB。
- **probe**：
  - `probe deploy <shared>`：预编译，打印耗时和 build 的大小；
  - `probe run [--shared D] [--user D]`：分别测加载、首键、每键平均和最大耗时、切换方案耗时，以及私有内存（`GetProcessMemoryInfo`）；
  - 输入只用固定的几组：`nihao`、`nh`、`jintiantianqihenhao`；小鹤 `nihc`；T9 `64426`。每组最多打印前 3 个候选。
  - 还要测：选词（含部分选词）、退格、commit_raw、T9 选拼音、候选网格。
- **App 要随包发的内容（计划，待实测确认）**：`rime.dll`，以及 `data\rime\` 下的 `build\`、`lua\`、`opencc\`、`custom_phrase.txt`、`en_dicts\*.txt`。方案和词库的源 yaml 不用发。

## 在 Surface 上跑 probe（写好以后）
```
scripts/surface/build.sh rime build --release -p dianmo-rime --example probe
# 把 C:\dev\dianmo-data\librime\rime.dll 复制到 target\x86_64-pc-windows-gnullvm\release\examples\
# 先用 llvm-objdump -p 检查导入表，再通过 scripts/surface/ps.sh 运行 probe.exe deploy / run
```
注意：不要 cat、grep 词库和词表文件（*.dict.yaml、custom_phrase.txt、opencc、lua、en_dicts），只看文件名、大小、行数。之前两次因为内容过滤被中断。
