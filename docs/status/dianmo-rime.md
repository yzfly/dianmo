# dianmo-rime 状态（2026-10-07，v0.2.0 引擎侧：模糊音、四种双拼、用户词库，Surface 实测通过）

librime 1.17.0（`rime.dll` 运行时加载）+ rime-ice（`da1fbe6`）。`RimeEngine` 实现 `dianmo_core::Engine`，打包时预编译，运行时直接用预编译结果。

## v0.2.0 公开 API（给 app / 设置界面接线用，签名已定）

```rust
// ---- 双拼方案（不依赖 Windows，dianmo_rime::ShuangpinScheme）
pub enum ShuangpinScheme { Flypy /*小鹤*/, Ziranma /*自然码*/, Mspy /*微软*/, Sogou /*搜狗*/ }  // Default = Flypy
impl ShuangpinScheme { pub const ALL; fn schema_id(self) -> &str; fn name(self) -> &str /*"小鹤双拼"…*/; fn uses_semicolon(self) -> bool /*微软、搜狗：ing 在 ; 键*/ }
pub struct Options { /* 原有字段 */, pub shuangpin: ShuangpinScheme }   // for_app() 默认 Flypy
impl RimeEngine {
    pub fn set_shuangpin(&mut self, scheme: ShuangpinScheme);  // 当前是双拼就立即切过去（60–80 ms，清空组字）
    pub fn shuangpin(&self) -> ShuangpinScheme;
}
pub fn rime_schema(schema: Schema, sp: ShuangpinScheme) -> &'static CStr;  // schema_id(schema) 仍在，双拼返回小鹤

// ---- 模糊音
pub type Fuzzy = [bool; 7];   // 顺序固定：z/zh, c/ch, s/sh, n/l, an/ang, en/eng, in/ing（= dianmo-ui FuzzyPair::index）
pub const FUZZY_LABELS: [&str; 7];
pub enum FuzzyChange { Unchanged, Reload, Deploy }
pub fn set_fuzzy(opts: &Options, fuzzy: Fuzzy) -> Result<FuzzyChange, Error>;  // 只写/删 <user>\<schema>.custom.yaml，不碰 librime
pub fn fuzzy(opts: &Options) -> Fuzzy;                  // 用户目录里当前写着的设置
pub fn needs_user_deploy(opts: &Options) -> bool;       // 有模糊音但没有匹配的用户 build（启动时也要查：新版本数据会让旧 build 失效）
pub fn deploy_user(opts: &Options) -> Result<DeployReport, Error>;  // 只在 `dianmo.exe --deploy-user` 子进程里调
impl RimeEngine {
    pub fn reload(&mut self, opts: &Options) -> Result<(), Error>;  // finalize + initialize + 重开会话，≈90–110 ms
    pub fn uses_user_build(&self) -> bool;                          // 模糊音是否已生效
}

// ---- 用户词库（rime_ice.userdb，所有方案共用）
impl RimeEngine {
    pub fn export_user_dict(&mut self, path: &Path) -> Result<usize, Error>;  // 条数；文本：词\t拼音\t次数（librime 格式，UTF-8）
    pub fn import_user_dict(&mut self, path: &Path) -> Result<usize, Error>;  // 合并导入；去 BOM/CRLF；非 UTF-8 报错
    pub fn clear_user_dict(&mut self) -> Result<(), Error>;                   // 删 userdb（期间重启 librime）
    pub fn user_word_count(&mut self) -> Option<usize>;                       // 设置页「已学习 N 个词」
}
// 同名自由函数：进程里没有 RimeEngine 时用（如 Rime 启动失败、还在 Basic 引擎）；有引擎时返回错误
pub fn export_user_dict(opts: &Options, path: &Path) -> Result<usize, Error>;
pub fn import_user_dict(opts: &Options, path: &Path) -> Result<usize, Error>;
pub fn clear_user_dict(opts: &Options) -> Result<(), Error>;
pub fn user_word_count(opts: &Options) -> Option<usize>;
```

### app 接线流程（建议）

- **启动**：`opts = Options::for_app()`，`opts.user_data_dir = data_dir\rime`（已有），`opts.shuangpin = 设置里的方案`；在 `RimeEngine::start` **之前**调 `set_fuzzy(&opts, prefs.fuzzy)`（设置是唯一来源，一致时返回 `Unchanged`，只读几个小文件）。引擎起来后若 `needs_user_deploy(&opts)` → 走下面的「部署」。
- **改模糊音**（`SettingsAction::SetFuzzy`）：`set_fuzzy(&opts, new)`：
  - `Unchanged` → 无事；`Reload` → `engine.reload(&opts)`（全部关掉时就是这样，删用户 build 不用部署）；
  - `Deploy` → 起子进程 `dianmo.exe --deploy-user`（带上 `--instance <name>`，如果有；`CREATE_NO_WINDOW`），后台线程等它退出；退出码 0 → 回 UI 线程 `engine.reload(&opts)`；之后再查一次 `needs_user_deploy`，期间用户又改了就再跑一次（子进程之间在 `build.lock` 上排队，不会互相踩）。
  - 部署期间引擎照常用旧设置打字，不用切 Basic。子进程自己降到低优先级。
- **改双拼方案**（`SetShuangpin`）：`engine.set_shuangpin(s)`，同时更新 app 保存的 `opts.shuangpin`（以后 `reload`/`start` 用）。dianmo-ui 的 `ShuangpinScheme { Xiaohe, Ziranma, Microsoft }` 需要加 `Sogou`；映射 Xiaohe→Flypy、Microsoft→Mspy。微软、搜狗双拼的 ing 在 `;` 键（`uses_semicolon()`），键盘要有这个键；`engine.input(';')` 已能用。
- **用户词库**：导出/导入/清空/计数都在 UI 线程同步调 `RimeEngine` 的方法即可（各 ≈85–115 ms；期间会关掉再重开 librime 会话，当前组字被清空）。没有 Rime 引擎时用同名自由函数。`user_words: Option<u32>` = `user_word_count().map(|n| n as u32)`，打开设置页时取一次。
- `fuzzy_supported` = Rime 引擎可用。
- `--deploy-user` 进程的 stderr 会有 librime 的 `E… dict_compiler.cc:86] source file '…rime_ice.dict.yaml' does not exist.`（每个方案一行），这是预期的（安装包不带词库源文件，复用预编译词典），不是失败。

## 代码

| 文件 | 内容 |
|---|---|
| `src/ffi.rs` | `rime_api.h` 的 `#[repr(C)]` 镜像（98 个函数槽）+ `RimeModule`、`RimeLeversApi`（32 个槽，`rime_levers_api.h`），布局有测试 |
| `src/api.rs` | 加载 dll（`LoadLibraryExW` + `rime_get_api`，检查 `data_size` 和要用的函数槽）、进程级状态（`setup` 只调一次）、`Session` 安全封装（所有返回值复制成 Rust 值后用 librime 的 `free_*` 释放） |
| `src/engine.rs` | 公开 API：`Options`（`for_app()`、`in_dir(dir)`）、`Error`、`deploy()`、`DeployReport`、`RimeEngine`（含 `reload`、`set_shuangpin`、用户词库方法）、`schema_id()`/`rime_schema()`、`shutdown()` |
| `src/user.rs` | 用户目录：模糊音 `set_fuzzy`/`needs_user_deploy`/`deploy_user`（用户 build）、用户词库（levers API）及其自由函数 |
| `src/custom.rs` | 纯逻辑（服务器可测）：`ShuangpinScheme`、模糊音规则和 `custom.yaml` 生成、用户 build 的 stamp |
| `src/t9.rs` | 九宫格纯逻辑：数字↔字母、未确认部分定位、左栏拼音、显示用 preedit、选拼音后的新 input（`pick_input`） |
| `src/comment.rs` | 候选注释显示规则 |
| `examples/probe.rs` | `probe deploy <shared>`、`probe run ...`：测时延、内存和各项行为；`probe v2/deploy-user/fuzzy/dict`：v0.2 功能 |
| `scripts/rime/stage.ps1` | 组装随包文件（见下） |

`cargo test -p dianmo-rime`：15 个测试（服务器）。`cargo clippy -p dianmo-rime --examples --target x86_64-pc-windows-gnullvm`：无警告。

## 设计要点（已实测确认）

- **运行时**：`staging_dir = prebuilt_data_dir = <shared>\build`，不跑 maintenance，启动不编译、不写安装目录。`build\` 不完整时退回到 maintenance，编译到 `<user>\build.dev`（实测 20 s，只是开发兜底）。
- **deploy**：删掉旧 `build\` 全量重编；用户目录用临时目录，prebuilt 指向不存在的目录。每个进程只能跑一次，且不能和 `RimeEngine` 同进程（主程序 `--deploy` 是单独进程，符合）。`deploy_user` 同样是一次性的。
- **finalize 后可以再 initialize**（Weasel/Squirrel 重新部署也是这么做的）：`reload`、`clear_user_dict` 用它；`shutdown()` 之后也能再 `start`。实测 lua、userdb、opencc 都正常。
- **用户目录**（`%APPDATA%\Dianmo\rime`）：`rime_ice.userdb`（学习词频，所有方案共用）、`user.yaml`；开了模糊音再加 6 个 `<schema>.custom.yaml`、`build\`（0.6–0.8 MB）、`build.lock`（空文件）。空目录直接能用。

## 模糊音（实测确认）

- **做法**：`set_fuzzy` 给 6 个方案（`rime_ice`、`t9`、4 个双拼）各写一个 `<user>\<schema>.custom.yaml`，用 `"speller/algebra/@before NN": 'derive/…'` 把规则插到拼写运算**最前面**（作用在词典的全拼音节上）。这个位置对所有方案都对：全拼（rime-ice 自己注释掉的模糊音规则就在这里）、双拼（后面的 `xform` 把全拼变成双拼码，所以小鹤等不用另写规则）、九宫格（后面的 `derive/[abc]/2/` 把字母变数字）。librime 按排序后的键名应用 patch，`@before 00..13` 补零保证顺序。
- **规则**（两个方向都派生）：`^zh↔z`、`^ch↔c`、`^sh↔s`、`^n[aeiouv]↔^l…`（不动「嗯 n/ng」）、`an↔ang`（`juan/quan/xuan/yuan` 不派生 -ang，否则双拼里会和 jiang 等撞键）、`en↔eng`、`in↔ing`。见 `custom.rs` 的 `FUZZY_RULES`。实测编译后的 `t9.schema.yaml` 的 algebra 前 14 条就是这些规则，后面接 rime-ice 原有规则。
- **用户 build**：`deploy_user` 用 `deployer_initialize`（staging = `<user>\build.new`，prebuilt = `<shared>\build`）对每个有 custom 的方案 `deploy_schema(<shared>\<id>.schema.yaml)`：只重编方案 yaml 和 prism（`rime_ice.prism.bin`、`t9.prism.bin`、`double_pinyin*.prism.bin`），词典 `*.table.bin`/`*.reverse.bin` 从共享 build 复用（安装包没有 `*.dict.yaml`，librime 走「reuse existing table」）。部署后检查：每个方案都有 yaml + prism，且**不能**出现 table/reverse（没有词库源文件时重建会得到空词典），否则整个丢弃报错。最后写 `dianmo-build.txt`（stamp）。
- **stamp**：共享 build 的文件名+大小 + 各 custom 文件内容的 FNV 哈希。不匹配（换了新版数据包、设置改了还没部署）就**不用**用户 build（照常用共享 build，模糊音暂时不生效），`needs_user_deploy` 返回 true。
- **切换**：`start`/`reload` 在 `initialize` 之前（此时没有映射任何文件）把完成的 `build.new` 换成 `build`（换不了就直接用 `build.new`），删掉过期/不再需要的 build；有匹配的 build 时 staging = `<user>\build`、prebuilt = `<shared>\build`，librime 先找用户 build、找不到再回落到共享 build（`CreateDeployedResourceResolver`）。部署进程只写 `build.new`，不碰正在用的 `build`，所以部署时主进程可以照常打字。
- **安装包要带方案源文件**：`deploy_schema` 要读 `<shared>\*.schema.yaml`、`default.yaml`、`default.custom.yaml`、`symbols_*.yaml`，stage.ps1 现在会拷这些（共 ~0.2 MB，不含 `*.dict.yaml`）。
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

## v0.2.0 Surface 实测（2026-10-07，release probe，stage.ps1 组装的目录，测试用户目录在 `C:\dev\dianmo-rime2\target\` 下，没动真实 %APPDATA%）

| 项目 | 结果 |
|---|---|
| `deploy-user`（z/zh + n/l，6 个方案） | 进程内 0.78–0.98 s，整个子进程 0.8–1.0 s；`build` 637 KB（13 个文件） |
| `deploy-user`（7 组全开） | 0.85 s，779 KB；已是最新时再跑 1 ms（直接返回） |
| `reload`（含预热） | 88–107 ms |
| 启动（带用户 build vs 不带，热缓存） | 87–94 ms vs 90–95 ms：**没有差别** |
| 每键（全拼，7 组全开 vs 关） | 平均 1.65 vs 1.43 ms，p95 2.3 vs 2.0 ms，最大 3.9 ms |
| 切双拼方案 `set_shuangpin` | 60–78 ms |
| 用户词库 count / export / import / clear | 92–117 / 92–112 / 92–99 / 84–89 ms（含关、开会话和预热） |
| 内存 | 结束时私有 8.1 MB、工作集 15.1 MB（和 v0.1 一样） |
| 打包 deploy（7 个方案） | 17.2 s，`build\` 42.8 MB（+0.2 MB） |

行为（只看前 3 个候选；`pos(x)` = x 在前 100 个候选里的位置）：
- 模糊音关：全拼 `zi` → 子/自/字，pos(知) 无；`lan` → 蓝/烂/懒，pos(南) 无；九宫格 `526` → 看/👀/老，pos(南) 无；小鹤 `lj` → 蓝/烂/懒，pos(南) 无。
- 开 z/zh + n/l 并 reload 后：全拼 `zi` → 子/自/只，pos(知)=3；`lan` → 难/男/♂，pos(南)=3；九宫格 `526` pos(南)=6；小鹤 `zi` pos(知)=3、`lj` pos(南)=3；`nihao` → 你好/👋/利好。全部关掉 → `Reload` → `zi` 恢复 子/自/字，用户 build 被删掉。
- 四种双拼（模糊音开着）：小鹤 `nihc`/`ulpb`/`mktm`、自然码 `nihk`/`udpn`/`mytm`、微软 `nihk`/`udpn`/`m;tm`、搜狗同微软 → 首选分别是 你好/双拼/明天，preedit 显示全拼（「ni hao」「shuang pin」「ming tian」）。
- 用户词库：空库 count 0；打 4 个词各选首选 → 4；导出 4 条（文件 10 行，含注释头）；清空 → 0；导入 4 条 → 4，`nihao` 仍首选「你好」。自由函数版本（进程里没有引擎）：count 2 → export 2 → clear → 0 → import 2 → 2。
- 中文用户目录（`…\用户3\rime`）和中文导出路径（`导出.txt`）：部署、reload、导出导入清空都正常。
- 共享 build 变了（模拟新版本数据）：`needs_user_deploy` = true，启动时不用旧的用户 build（user build: false）；恢复后又自动认回来。

## 双拼键位（给 UI 的韵母提示用）

来源：各方案 yaml 的 `speller/algebra`（`<shared>\double_pinyin*.schema.yaml`，打包目录里也有）。四种方案的声母都一样：**zh = v、ch = i、sh = u**，其余声母就是本字母。韵母（从 xform 规则整理；单字母韵母 a o e i u 和 ü=v 就是本键）：

| 键 | 小鹤 flypy | 自然码 double_pinyin | 微软 mspy | 搜狗 sogou |
|---|---|---|---|---|
| q | iu | iu | iu | iu |
| w | ei | ia ua | ia ua | ia ua |
| r | uan | uan üan | er uan üan | er uan üan |
| t | ue üe | ue üe | ue üe | ue üe |
| y | un | ing uai | ü uai | ü uai |
| o | uo | uo | uo | uo |
| p | ie | un ün | un ün | un ün |
| s | ong iong | ong iong | ong iong | ong iong |
| d | ai | iang uang | iang uang | iang uang |
| f | en | en | en | en |
| g | eng | eng | eng | eng |
| h | ang | ang | ang | ang |
| j | an | an | an | an |
| k | ing uai | ao | ao | ao |
| l | iang uang | ai | ai | ai |
| z | ou | ei | ei | ei |
| x | ia ua | ie | ie | ie |
| c | ao | iao | iao | iao |
| v | ui (zh) | ui (zh) | ui ue üe (zh) | ui (zh) |
| b | in | ou | ou | ou |
| n | iao | in | in | in |
| m | ian | ian | ian | ian |
| ; | – | – | ing | ing |

零声母（a/o/e 开头的音节）：
- 小鹤、自然码：单韵母和 ang/eng 双写首字母（a→aa、o→oo、e→ee、ang→ah、eng→eg）；双字母韵母可以直接打（ai、an、ao、ei、en、er、ou），也可以首字母 + 韵母键（小鹤 ai→ad、an→aj、ao→ac、ei→ew、en→ef、ou→oz；自然码 ai→al、an→aj、ao→ak、ei→ez、en→ef、ou→ob）。
- 微软、搜狗：a/e 开头的首字母双写再接韵母键（a→aa、ai→al、an→aj、e→ee、er→er）；任何零声母音节也可以用 o 引导（ai→ol、ang→oh、er→or、o→oo）。

## 随包文件（stage.ps1 产出，共 48.5 MB，65 个文件；v0.1 是 48.1 MB / 44 个）

```
<Out>\rime.dll                         3.6 MB
<Out>\data\rime\build\                 42.8 MB  预编译结果（23 个文件）
    default.yaml, rime_ice / t9 / double_pinyin{_flypy,,_mspy,_sogou} / melt_eng / radical_pinyin .schema.yaml
    （每个双拼方案：编译后的 yaml ~50 KB + prism ~15 KB，共用 rime_ice 词典，所以 3 个新方案只多 ~0.2 MB）
    rime_ice.table.bin 27.7 MB, melt_eng.prism.bin 8.0 MB, radical_pinyin.*.bin 5.5 MB, 其余 < 1 MB
<Out>\data\rime\lua\                   0.8 MB   rime-ice 的 Lua 脚本 + lunar.db
<Out>\data\rime\opencc\                1.1 MB   emoji、简→繁
<Out>\data\rime\custom_phrase.txt, en_dicts\cn_en{,_flypy,_double_pinyin,_mspy,_sogou}.txt (各 ~15 KB)
<Out>\data\rime\*.schema.yaml, default.yaml, default.custom.yaml, symbols_v.yaml, symbols_caps_v.yaml  ~0.2 MB
                                       方案源文件：只有开模糊音时 deploy_user 会读
<Out>\data\rime\RIME_ICE_COMMIT
```
词库源文件（`*.dict.yaml`、`cn_dicts\`）不发；运行时只靠预编译结果，用户部署也复用预编译词典。随包体积 +0.4 MB（3 个新双拼方案 ~0.2 MB + 方案源文件 ~0.2 MB）。

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
probe v2 --shared <dir> --user <dir> [--dll …] [--file <export.txt>]   模糊音（含子进程部署 + reload）、四种双拼、用户词库，全流程
probe deploy-user --shared <dir> --user <dir>                           = dianmo.exe --deploy-user
probe fuzzy 1001000 --shared <dir> --user <dir>                         set_fuzzy，打印 FuzzyChange
probe dict --shared <dir> --user <dir> --file <txt>                     用户词库自由函数（无引擎）
```
`--dll` 默认是 exe 旁边的 rime.dll；`run` 不给 `--user` 时用一个新的临时目录（测的就是「空用户目录、不跑 maintenance」），结束后删掉。

## 遗留 / 以后可做

- 切方案要 64–165 ms（librime 每次切换都会重建方案的组件、重新映射词典）。如果以后频繁切换，可以给每个方案各开一个会话。
- 安装包体积：`melt_eng.prism.bin`（8 MB，英文混输）和 `radical_pinyin`（5.5 MB，部件拆字辅码）可以考虑砍掉，但要同时改方案配置，目前保持 rime-ice 原样。
- `shutdown()` 可选：退出前调用可以干净地关闭用户词典（LevelDB 不调用也不会丢数据）。
- 模糊音只做了这 7 组；rime-ice 注释里还有 f/h、r/l、g/k、ian/iang 单独开关等，要加只需在 `custom.rs` 加规则（设置模型的数组也要变长）。
- 英文混输（melt_eng）和部件拆字（radical_pinyin）没有跟着双拼方案改拼写（rime-ice 注释里说要手动改），影响很小，保持原样。
- 导入只接受 librime 自己的导出格式（`词\t拼音[\t次数]`）；搜狗/微软词库格式要另写转换。
- 清空用户词库只删 `rime_ice.userdb`；librime 的 sync 快照我们从不生成，不用管。
- 不要 cat、grep 词库和词表文件（*.dict.yaml、custom_phrase.txt、opencc、lua、en_dicts），只看文件名、大小、行数。之前两次因为内容过滤被中断。
