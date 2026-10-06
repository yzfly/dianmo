# 语音输入调研（TODO #27、#28）

2026-10-06。目标：点墨的麦克风键和「语音球」（点一下开始、再点一下结束）调用更准的语音引擎，替代 Windows 自带语音（Win+H）。

用户意见：「最好直接用现成的；本地小 ASR 太耗算力」。所以 **A 方向（调用现成产品的语音）是主线**，B 方向（本地模型）只做书面调研。

## 结论

1. **微信输入法（WeType 2.1.3.18）完全可用，作为默认引擎。** 实测 SendInput 注入的热键能触发语音（不过滤 LLKHF_INJECTED）：
   - 点一下开始、再点一下结束：发 `Ctrl+Win+Shift`。
   - 按住说话、松开结束：按住 `Ctrl+Win`。
   - 取消：`Esc`。
   - 语音浮窗约 0.9 秒内出现，麦克风开始收音；松开后识别结果直接上屏，带标点。
   - 管理员窗口也能用。
2. **不要求目标窗口的输入法是 WeType 才能开始收音**，但结果怎么交付取决于它：
   - 目标窗口的当前输入法是 WeType：结果直接上屏（走 TSF）。
   - 是别的输入法（实测搜狗）：WeType 照样收音，但结果只复制到剪贴板，并弹出提示「已复制 · 粘贴即可使用」。点墨能检测到剪贴板变了，自己把文字打进去（见下文「剪贴板兜底」）。
3. **和点墨上屏兼容。** 目标窗口用 WeType 中文模式时，点墨发的 Unicode 文字、退格、回车、方向键、空格都原样生效，WeType 不拦截。收音过程中点墨上屏也不会打断语音。
4. **「豆包」：Surface 上没有装豆包输入法。** 装的是 `DouBaoVoice`，一个第三方的独立语音工具（.NET + Avalonia 写的「豆包语音输入 Windows 版」）：
   - 调用火山引擎的豆包流式语音大模型接口，用的是用户自己的 AppId/Token，热键 F6，自带悬浮球。
   - 和输入法无关，对任何输入框都有效。
   - 实测注入 F6 能开始和结束录音（单击切换）。录音时房间里没人说话，所以文字输出这一步没验证到。
   - 平时没在运行，跑起来约 100MB 私有内存。
   - 可以作为第二个引擎。官方豆包输入法装好以后要再测一轮（见「豆包输入法」）。
5. **推荐**：设置里提供「语音引擎：微信输入法（默认）/ 豆包语音 / 系统」。
   - 选中的引擎不可用（没装、没在运行、发出热键后 1.5 秒内没开始收音）时，自动回退到 Win+H。
   - 接口建议见文末。

## A. 调用第三方的语音：实测

### 环境（Surface）

| 项目 | 情况 |
|---|---|
| 微信输入法 | 2.1.3.18，`C:\Program Files\Tencent\WeType\2.1.3.18\`。TSF TIP 的 CLSID 是 `{86598FB9-66A2-463E-B9C2-AEB906D477AD}`，profile 是 `{607FDF85-FCC8-4DBD-A365-41296F980C9C}`。常驻进程：`wetype_server.exe`、`wetype_renderer.exe`、`wetype_update.exe`（**语音浮窗和录音都在这个进程里**）。已下载 2.1.4.6 补丁，还没装 |
| 搜狗拼音 | 16.6，`Get-WinDefaultInputMethodOverride` 是搜狗。但 `HKCU\Software\Microsoft\CTF\Assemblies\0x00000804\{34745C63-…}` 的 Default 是 WeType，实测新开的记事本默认就是 WeType |
| 「按应用窗口分别设置输入法」 | 已开启（`SPI_GETTHREADLOCALINPUTSETTINGS` = TRUE），切换输入法只影响当前窗口 |
| 豆包输入法 | **没有安装**：没有 TIP，也没有卸载项 |
| DouBaoVoice | `%USERPROFILE%\Downloads\DouBaoVoice-sucess.exe`，92MB，self-contained .NET 单文件，没有签名，2026-05-05 下载。配置 `%APPDATA%\DouBaoVoice\config.json`，字段：`AppId`、`AccessToken`、`Cluster=volcengine_streaming_common`、`HotKey=F6`、`SampleRate=16000`、`Channels`、`BitsPerSample`、`IsConfigured=true`。平时没在运行，也没设开机自启 |
| 微信客户端 | 没装（微信 4.1.8 以上也有 Ctrl+Win 全局语音，这里不涉及） |
| 讯飞输入法 | HKCU Run 里还有它的启动项，但 TIP 已不在，看起来是残留 |

WeType 的用户设置放在加密的 MMKV / leveldb 里，读不出热键配置。热键按官方默认值来测，结果全部命中，说明用户没改过。官方默认值：

- 按住 `Ctrl+Win` 说话；
- `Ctrl+Win+Shift` 切换开始和结束，不用一直按着。

来源：[IT之家](https://www.ithome.com/0/930/754.htm)、[光明网](https://m.gmw.cn/2026-03/20/content_1304383585.htm)。

### 方法

脚本：`scripts/voice/hotkey-probe.ps1`，用 `scripts/voice/probe.sh` 远程执行。

它在用户的桌面会话里新开一个记事本，用 SendInput 发热键，热键的发送方式和点墨一样（带扫描码，Win、右 Ctrl 加 EXTENDEDKEY 标志）。然后收集以下证据：

- 截图；
- 语音相关进程的可见窗口；
- 正在用麦克风的程序：读 `CapabilityAccessManager\ConsentStore\microphone`，`LastUsedTimeStop=0` 表示正在用；
- 记事本里上屏的**字数**（不打印内容：麦克风收到的是房间里的真实说话声）；
- 剪贴板序号有没有变；
- 前台窗口。

测完强制关掉记事本，不保存。

gui.sh 的计划任务以最高权限运行，所以测试用的记事本是**管理员权限**。点墨现在也是用最高权限启动的（#23），条件一致。

### 结果

| # | 热键 / 场景 | 结果 |
|---|---|---|
| t1 | 按住 `LCtrl+LWin` 3 秒后松开（记事本的输入法是 WeType） | 0.9 秒内出现语音浮窗（窗口类 `wetype.flutter.setting`、标题「语音输入」，进程 `wetype_update.exe`），麦克风在用的是 `wetype_update.exe`。松开后浮窗消失，**房间里的说话声被识别成一句带标点的中文，上屏到记事本**。没有弹出开始菜单，前台窗口没变 |
| t2 | 单击 `LCtrl+LWin+LShift` 开始，3 秒后再单击一次结束 | 松开按键后仍在收音（截图里浮窗显示声波），第二次单击后结束 |
| t2 | 兼容性：上屏「\|你好abc」→ 退格×2 → 回车 → `x` → ← → `y` → 空格 | 结果 `|你好a⏎y x`，和预期一致。WeType 中文模式下，点墨的 Unicode 上屏和退格、回车、方向键、空格都不受影响 |
| t3/t10 | 先用 Win+Space 把记事本切到**搜狗**，再发热键 | 照样弹出浮窗、照样收音。**松开后不上屏，提示「已复制 · 粘贴即可使用」，剪贴板序号变了**，提示浮窗还会停留一会儿。退格、回车测试同样正常 |
| t4 | 收音中点墨上屏 `[Z]` | `[Z]` 立刻上屏，收音继续，结束后识别结果接着上屏 |
| t5 | 收音中焦点切到另一个记事本 | 收音继续。结束后**结果上屏到结束时有焦点的窗口**（第二个记事本），第一个记事本里什么也没有 |
| t6 | 单击开始，然后发 `Esc` | 浮窗消失，麦克风释放，不上屏，相当于取消 |
| t9 | WeType 直接上屏的情况 | 剪贴板没变（只有输入法不是 WeType 时才走剪贴板） |
| t7/t8 | DouBaoVoice 在运行时，单击 F6 开始，4 秒后再单击 F6 | 悬浮球（左侧 56×56）移到屏幕上方中间，变成录音胶囊（带停止按钮），麦克风在用的是 `DouBaoVoice-sucess.exe`。第二次 F6 后显示「优化中…」，然后变回小球。这两次房间里没人说话，8 秒内没有上屏，剪贴板也没变，**所以文字输出这一步没验证到**（程序里同时引用了剪贴板和 SendInput） |

测试的副作用：t3、t10 两次被 WeType 覆盖了用户剪贴板的内容。记事本都已关掉，DouBaoVoice 测完也已结束进程，没有改任何设置。

### 豆包输入法（官方，Surface 上没装）

公开资料：2026-09 发布 Windows 版 V0.9.0。

- 语音热键可选「右 Alt」（长按）、「右 Alt + 空格」、「左 Ctrl + Win」。
- 支持离线语音。

来源：[腾讯新闻](https://news.qq.com/rain/a/20260908A0E01D00)。

开源工具 [doubao-ime-voice-bridge](https://github.com/fancy1234/doubao-ime-voice-bridge) 的经验：**豆包输入法要是当前输入法才能用语音**。所以它先用 Win+Space 切到豆包，点状态栏上的麦克风（窗口类 `OimeDirectUIWindow`），用完再切回原来的输入法。

用户装好以后，要用 `probe.sh` 测三件事：

- 注入的热键能不能触发；
- 输入法不是豆包时会怎样（不触发，还是像 WeType 一样复制到剪贴板）；
- 「右 Alt + 空格」是不是单击切换。

### 对「语音球」四个问题的回答

**1. 开始和结束要发什么（每一行都是一次 `SendInput`）**

| 引擎 | 点一下开始 / 点一下结束（同一个序列） | 按住说话（按下 / 松开） | 取消 |
|---|---|---|---|
| 微信输入法 | `LCtrl↓ LWin↓ LShift↓ LShift↑ LWin↑ LCtrl↑` | 按下发 `LCtrl↓ LWin↓`，松开发 `vkE8↓ vkE8↑ LWin↑ LCtrl↑` | `Esc↓ Esc↑` |
| DouBaoVoice | 发 config.json 里的 `HotKey`（目前是 `F6↓ F6↑`） | 不支持（单击切换） | 再单击一次 |
| 系统 | `LWin↓ H↓ H↑ LWin↑` | — | — |

细节：

- **Win 键要加 `KEYEVENTF_EXTENDEDKEY`**，并带扫描码（`MapVirtualKey`）。
- 按住说话松开时，先发一个未分配的虚拟键 **vkE8**，再抬起 Win。这样即使 WeType 没接住，也不会弹出开始菜单（AutoHotkey 的 menu mask 做法）。单击切换的序列里 Shift 夹在 Win 的按下和抬起之间，本身就不会弹开始菜单。
- 发热键之前，先把点墨自己锁住的修饰键（#22 的 Ctrl、Shift 锁定）抬起，再清掉候选条里没上屏的拼音，否则组合键会变。
- Esc 也会发到目标应用。WeType 收音时似乎会拦截 Esc（t6 里记事本没有反应），但不能保证。语音球的「取消」最好只在确认正在收音时才发 Esc。

**2. 是否要求目标窗口的输入法就是它**

- WeType：开始收音不要求，直接上屏要求。不是 WeType 时，结果进剪贴板。点墨这样兜底：
  1. 开始前记下 `GetClipboardSequenceNumber()`，并保存剪贴板里的文本（`CF_UNICODETEXT`）。
  2. 结束后，等麦克风释放，再最多等 1.5 秒。
  3. 如果序号变了，读出剪贴板文本，用 `TextSink`（Unicode SendInput）打进目标窗口，再恢复原来的剪贴板文本。
  - 点墨不去切换目标窗口的输入法：Win+Space 会依赖用户的输入法顺序，太脆弱。
  - 在设置里提示用户：把 WeType 设为默认输入法体验最好。
- DouBaoVoice：独立程序，和输入法无关。
- 豆包输入法：据资料要求是当前输入法，待实测。

**3. 怎么检测它是否可用、有没有真的开始收音**

| | 已安装 | 在运行 | 正在收音 |
|---|---|---|---|
| 微信输入法 | 存在 `HKLM\SOFTWARE\Microsoft\CTF\TIP\{86598FB9-66A2-463E-B9C2-AEB906D477AD}`（或卸载项「微信输入法」），并且这个 TIP 在用户的输入法列表里已启用 | 有 `wetype_server.exe` 和 `wetype_update.exe` 进程 | 麦克风记录里 `…\NonPackaged\C:#Program Files#Tencent#WeType#<ver>#wetype_update.exe` 的 `LastUsedTimeStop == 0`。辅助判断：有可见窗口，类名 `wetype.flutter.setting`、标题「语音输入」。只靠浮窗不行，剪贴板提示期间它也在 |
| DouBaoVoice | `%APPDATA%\DouBaoVoice\config.json` 里 `IsConfigured=true`（只读 `HotKey` 和 `IsConfigured`，不读 token）；exe 路径在设置里指定 | 有进程名以 `DouBaoVoice` 开头的进程 | 同样看麦克风记录，键名是这个 exe 的路径（`\` 换成 `#`） |
| 系统 | 一直可用 | — | — |

做法：

- 麦克风记录在 `HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone`，Win10 1903 以上都有。
- 用 `RegNotifyChangeKeyValue` 监听这个键，事件驱动，空闲时不占 CPU。
- `start()` 之后 1.5 秒内没看到麦克风被占用，就判定失败：回退到 Win+H，这次会话内标记该引擎不可用，并在语音球上提示。

**4. 收音过程中的冲突**

- 点墨上屏：互不干扰（t4）。点墨不抢焦点，点语音球也不会改变焦点。
- 焦点切到别的输入框：收音继续，结果上屏到结束时有焦点的窗口（t5）。点墨不用特别处理，语音球保持收音状态即可。如果想要「换输入框就结束」，可以在 UIA 焦点事件里调用 `stop()`。
- 管理员窗口：WeType 的结果能上屏（测试用的记事本就是管理员权限）。剪贴板兜底由点墨来打字，点墨本身就是最高权限。
- 用户用点墨的「电脑键」按 `Ctrl+Win+←/→`（切换虚拟桌面）时，WeType 可能把 Ctrl+Win 当作按住说话的开始。需要实测；必要时点墨发这类组合键时把 Win 放在 Ctrl 前面按下。

## B. 本地小模型（只做书面调研，没有在 Surface 上实测）

| 方案 | 中文准确率 | 体积（int8） | CPU 速度 | 备注 |
|---|---|---|---|---|
| **sherpa-onnx + SenseVoice-Small** | AISHELL-1 CER 约 3%，带标点，中英粤日韩 | 模型 228MB + onnxruntime 约 15MB | 非流式。官方数据：Cortex-A55 单线程 RTF 约 0.44，4 线程约 0.18；x86 更快，10 秒音频估计 0.5–1.5 秒 | Apache-2.0（模型另有 FunASR 许可）。sherpa-onnx 有预编译的 Windows x64 共享库和 C API，Rust 可以像加载 rime.dll 一样动态加载 |
| sherpa-onnx + Paraformer-zh（流式） | 和 SenseVoice 接近，标点要另加模型 | 约 220MB | 流式，边说边出字 | 更像手机输入法，但更吃 CPU |
| whisper.cpp small/medium | 中文明显不如前两者，标点和简繁也不稳 | 190MB–500MB | 慢 | 不推荐 |
| FunASR / Fun-ASR-Nano、Qwen3-ASR 等 2025–26 年的新模型 | 更准 | 0.6B–1B+ 参数 | 在 Surface 的 CPU 上偏重 | 暂不考虑 |

算力估计：SenseVoice 每次识别会让 CPU 满载约 1 秒，内存高峰约 300–400MB。随包体积增加约 250MB。

如果以后要做：放在独立的辅助进程 `dianmo-voice.exe` 里，按需启动，空闲 60 秒后退出，不在点墨主进程里常驻加载模型。

- 用 WASAPI 共享模式采集 16kHz 单声道；
- 用 Silero VAD 自动断句；
- 通过管道把文字发给点墨，点墨用 `TextSink` 上屏。

这符合用户「太耗算力」的判断：**本地模型只作为离线时的远期选项**。

## 实现建议（可以直接照着写）

放在 `dianmo-win/src/voice.rs`；语音球界面在 dianmo-ui，通过 `Action` 驱动。

```rust
pub enum VoiceEngine { WeType, DoubaoVoice, System }      // 以后再加 DoubaoIme、Local

pub enum VoiceState { Idle, Starting, Listening, Finishing }

pub struct VoiceSession { /* engine, 剪贴板序号和保存的文本, 开始时刻 */ }

impl VoiceEngine {
    /// 已安装且在运行（WeType 看 TIP 和进程；DoubaoVoice 看 config 和进程）。
    pub fn available(self) -> bool;
    /// 读取或推导热键：WeType 用固定默认值，DoubaoVoice 从 config.json 的 HotKey 读。
    fn toggle_chord(self) -> Vec<KeyEvent>;
}

pub struct Voice { engine: VoiceEngine, session: Option<VoiceSession>, mic_watch: MicWatch }

impl Voice {
    /// 引擎不可用时直接走 System（Win+H）。返回实际使用的引擎。
    pub fn start(&mut self, sink: &mut dyn TextSink) -> VoiceEngine;
    /// 再发一次切换热键，然后等结果；WeType 不在当前输入法时走剪贴板兜底。
    pub fn stop(&mut self, sink: &mut dyn TextSink);
    pub fn cancel(&mut self);                 // WeType：Esc；DoubaoVoice：再按一次热键
    pub fn is_active(&self) -> bool;          // 看麦克风记录（MicWatch，RegNotifyChangeKeyValue）
    /// 宿主在 MicWatch 事件或定时器到期时调用：
    /// 1.5 秒内没开始收音 → 回退到 Win+H；用户在引擎自己的界面上停止 → 语音球同步回到空闲。
    pub fn poll(&mut self, sink: &mut dyn TextSink) -> VoiceState;
}
```

- 设置项：`voice.engine = "wetype" | "doubao_voice" | "system"`，默认 `wetype`；`voice.doubao_voice_exe` 是可选路径。
- 语音球：点一下 `start()`，再点一下 `stop()`；长按时如果引擎是 WeType，就用按住说话的序列。
- 引擎在它自己的界面上被停止（比如用户点了 DouBaoVoice 的停止按钮）时，`poll()` 发现麦克风释放了，语音球同步回到空闲。
- 工作量：`voice.rs` 加 `MicWatch` 约 300 行，加上设置项和语音球联调，约 1–1.5 天。官方豆包输入法装好后，实测一轮再补进来，约 0.5 天。

## 脚本

- `scripts/voice/hotkey-probe.ps1`：注入热键并收集证据。
  - 参数：`-Keys`、`-Mode hold|tap`、`-HoldMs`、`-AfterMs`、`-ImeCycles`（先用 Win+Space 切换记事本的输入法）、`-During type|switch|esc`、`-TypeTest`。
- `scripts/voice/probe.sh <本地截图目录> [参数]`：上传并在 Surface 桌面会话里执行，取回截图。
  - 例子：`scripts/voice/probe.sh /tmp/s -Keys LCtrl+LWin+LShift -Mode tap -HoldMs 3000 -TypeTest 1`。
