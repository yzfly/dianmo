# 本地小模型语音识别：调研与 Surface 实测（TODO #40）

2026-10-06。用户原话：「有没有那种几 MB、十几 MB 的 ASR 小模型？手机端是有的，看能不能部署，在点墨里面自己实现。」

前一轮调研（`docs/research/voice.md` 的 B 节）推荐的是 SenseVoice-Small：约 250MB，非流式，每次识别会让 CPU 满载约 1 秒。用户嫌太重。这一轮专门找小模型，并且全部在 Surface 上实测过。测试只用命令行，没有打开任何窗口，也没有用麦克风。

## 结论

1. **有，能跑，而且很省。** sherpa-onnx 的流式中文小模型 `streaming-zipformer-small-ctc-zh-int8-2025-04-01`：
   - 模型 25MB，7z 压缩后 19MB；运行库 20MB，压缩后 4MB。
   - 在 Surface Pro 9 上边说边识别：平均只占单核 6%，峰值 12%，相当于整机（12 线程）的 0.5%。私有内存约 90MB。
   - 加载 1.1–1.5 秒。用户点「停止」后，约 40ms 出最终结果。
2. **但准确率明显不如微信输入法，不建议当主力。**
   - 日常口语句子（TTS 合成）字错率约 5%；AISHELL-1 真人朗读约 9.5%，也就是大约每 10–20 个字错 1 个。
   - **没有标点。英文单词全部丢失**（输出 `<unk>`，PPT、OK 都没了）。人名、生僻词容易错成同音字。
   - 另一个「十几 MB」的候选 `zh-14M`（24MB）更差：AISHELL 约 17%。
3. **如果要做本地引擎，推荐 X-ASR（2026-06）。** 全称 `x-asr-480ms-streaming-zipformer-transducer-zh-en-punct-int8`，是上海交大等单位开源的流式中英模型，Apache-2.0：
   - 模型 161MB，压缩后 122MB，不符合「十几 MB」，但仍比 SenseVoice 小。
   - 流式，自带标点，能识别中英混说。我们的 8 句日常句子（正常语速和快速各一遍）只错 1 个字；AISHELL 5.9%。
   - 平均占单核 15%，峰值 27%（整机约 1.3%）。私有内存约 255MB，加载约 5.5 秒。
   - 和 SenseVoice 比：SenseVoice 是说完以后满载算 1 秒，X-ASR 是边说边算，负载很平。
4. **建议**：
   - 微信输入法仍然做默认引擎，本地引擎作为「离线 / 隐私」选项（`VoiceEngine::Local`）。
   - 模型不随安装包，用户启用时再下载：X-ASR 加运行库约 126MB。
   - 如果坚持「几十 MB 以内」：用 small-ctc 25MB 做流式预览，说完以后再用离线 small-ctc（60MB）重识别一遍定稿。实测 AISHELL 可以降到 3.7%，但仍然没有标点，英文仍然丢失。设置里可以作为「精简模型」提供，但要写明准确率较低。
5. **实现方式**：
   - 按需启动的辅助进程 `dianmo-voice.exe`：WASAPI 采集 16kHz，交给 sherpa-onnx 流式识别；识别中的文字显示在候选条或语音球的气泡里，说完上屏。
   - 空闲几分钟后退出进程，回收全部内存。
   - 原型 `crates/dianmo-asr` 已在 Surface 上跑通（识别 wav 文件，无界面）。
   - 完整接入约 5 个工作日（见文末）。

## 候选对比（调研）

| 模型 | 体积（int8） | 参数量 | 流式 | 标点 | 中英混合 | 公开指标 | 许可 | 备注 |
|---|---|---|---|---|---|---|---|---|
| **sherpa-onnx streaming-zipformer-small-ctc-zh-int8-2025-04-01** | 25MB（压缩包 20MB） | 约 2200 万（由 fp32 87MB 推算） | ✅ | ❌ | ❌（英文输出 `<unk>`） | 官方没给 CER；文档 RTF 0.038 | 未写明（[issue #3915](https://github.com/k2-fsa/sherpa-onnx/issues/3915) 有人问，还没回复） | 实测 ① |
| **sherpa-onnx streaming-zipformer-zh-14M-2023-02-23** | encoder 21MB + decoder 1.8MB + joiner 1.7MB = 24MB | 1400 万 | ✅ | ❌ | ❌ | 官方没给 CER；用 WenetSpeech 训练 | Apache-2.0（icefall） | 实测 ② |
| **sherpa-onnx streaming-zipformer-ctc-multi-zh-hans-int8-2023-12-13** | 68MB | 约 6300 万 | ✅ | ❌ | 少量 | 约 1.4 万小时多数据集训练；文档 RTF 0.078 | Apache-2.0 | 实测 ③。句首丢字（见下） |
| **X-ASR zh-en punct int8 2026-06-05**（480ms 块；另有 160/960/1920ms 版本，权重相同） | encoder 148MB + decoder 11MB + joiner 2.5MB = 161MB | 0.16B | ✅ | ✅ | ✅ | 约 100 万小时。流式 480ms：LibriSpeech clean 3.14 / other 7.57、GigaSpeech 9.77、WenetSpeech meeting 7.38（离线 5.96） | Apache-2.0 | 实测 ④ |
| sherpa-onnx zipformer-ctc-small-zh-int8-2025-07-16 | 60MB | — | ❌ 非流式 | ❌ | ❌ | — | 未写明 | 实测 ⑤：作为「说完再精修」的第二遍 |
| Vosk vosk-model-small-cn-0.22 | 42MB | — | ✅ | ❌ | ❌ | CER：SpeechIO-02 23.5%、SpeechIO-06 38.3%、THCHS 17.2%（大模型 1.3GB 分别是 14.0 / 27.3 / 7.4） | Apache-2.0 | Kaldi，比 sherpa 小模型差很多，没有实测 |
| sherpa-onnx paraformer-zh-small-2024-03-09 | 约 79MB（压缩包 74MB） | — | ❌ | ❌ | 少量 | — | FunASR 模型许可 | 非流式，没有实测 |
| Moonshine base-zh（2026-02） | 压缩包 95MB | 6150 万 | sherpa 版本为非流式 | ❌ | ❌ | — | Moonshine 社区许可 | 没有中文 tiny 版本 |
| SenseVoice-Small int8 | 228MB | 2.34 亿 | ❌ | ✅ | ✅ | AISHELL-1 约 3% | FunASR 模型许可 | 上一轮推荐的方案 |
| 中文标点模型 ct-transformer zh-en int8 | 压缩包 61MB | — | — | — | — | — | — | 给无标点模型补标点，体积比小模型本身还大 |
| FunASR paraformer-zh-streaming、Fun-ASR-Nano、Qwen3-ASR、FireRedASR2 | 220MB 到 1GB 以上 | 0.2B 到 1B 以上 | 部分支持 | ✅ | ✅ | 更准 | — | 太大，不考虑 |
| WeNet u2pp conformer（WenetSpeech） | int8 约 110MB 以上 | — | ✅ | ❌ | — | — | Apache-2.0 | 没有 30MB 以下的中文成品 |

手机输入法的端侧方案都不能直接拿来用：

- 讯飞离线听写 SDK：商业授权，闭源。
- 豆包输入法：支持离线语音，闭源。
- Gboard、Apple 的端侧识别：Windows 上没有。

公开可用、带 Windows 预编译库和 C API 的，基本就是 k2-fsa 的 sherpa-onnx 这一系，它的 Android 演示 APK 用的就是上面这些小模型。

**运行库**：sherpa-onnx v1.13.8，包名 `win-x64-shared-MT-Release-no-tts`（22MB）。

- `sherpa-onnx-c-api.dll` 3.1MB，`onnxruntime.dll` 17MB（onnxruntime 1.28.2）。
- MSVC 编译，静态 CRT（/MT），不需要 VC++ 运行库。
- 我们用 gnullvm 编的 Rust 程序通过 `LoadLibraryExW` + C ABI 调用，实测没有问题（做法和 rime.dll 一样）。
- 两个 DLL 7z 压缩后共 4.3MB。

## Surface 实测

### 环境与方法

- **机器**：Surface Pro 9，i7-1265U（10 核 12 线程，2 个性能核 + 8 个能效核），16GB，Win10 LTSC 21H2，接电源，平衡模式。
- **位置**：全部在 `C:\dev\dianmo-asr\`。所有进程都用 `start /belownormal` 启动，没有打开任何窗口。
- **下载来源**：全部来自 GitHub releases `k2-fsa/sherpa-onnx`（tag `asr-models` 和 `v1.13.8`），Surface 上速度 4–5MB/s，不需要镜像。
  - Win10 自带的 tar 不支持 bz2，用 Surface 上已装的 7-Zip 先解开 .bz2。
  - AISHELL 样本通过 Hugging Face datasets-server API 下载（数据集 `AudioLLMs/aishell_1_zh_test`），见下。
- **测试程序**：`crates/dianmo-asr` 的 example `asr_wav.exe`。
  - 统计每句耗时、RTF、进程 CPU 时间（`GetProcessTimes`）、私有内存（`PrivateUsage`、`PeakPagefileUsage`）、模型加载时间。
  - 加 `--realtime` 时，按 100ms 一块、以真实时间的速度喂音频，模拟麦克风。这时额外统计首字延迟、说完后多久文字完整、点「停止」到出最终结果的耗时，以及每 0.5 秒窗口里的 CPU 峰值。
- **线程**：onnxruntime 都用 1 个线程（原因见「线程数和进程优先级」）。
- **字错率（CER）**：去掉标点和空格、英文统一转成大写后算编辑距离，各模型用同一口径。
- **测试音频**（都是公开内容或我们自己的固定句子，不涉及隐私）：
  - **TTS**：用 Windows 自带的中文 TTS（Microsoft Huihui Desktop，`System.Speech`）生成 16kHz 单声道 wav。8 句固定句子，包括日常口语、数字和英文词，每句生成正常语速（Rate 0）和快速（Rate +3）两遍，共 16 个文件、75 秒。脚本是 `scripts/asr/tts.ps1`，句子在 `scripts/asr/sentences.txt`。
  - **AISHELL-1 测试集（真人朗读）**：从 6920 条里每隔 179 条取 1 条，共 39 条、204 秒，覆盖约 20 位说话人。参考文本用官方标注。脚本是 `scripts/asr/aishell.ps1`。
  - **模型包自带的真人录音**：WenetSpeech 的 DEV/MEETING 片段，以及 X-ASR 附带的中英混说课堂录音。没有官方参考文本，只用来定性对比。
- **静音**：每个文件前面补 300ms 静音（真实使用时麦克风会先于说话打开）。识别结束（`finish()`）时再补一段静音，小模型补 500ms，X-ASR 补 1000ms。原因见「踩到的坑」。

### 准确率、速度、内存（文件模式，尽快识别，1 线程）

| 模型 | 模型体积 | 加载时间 | TTS 16 句 CER | AISHELL 39 句 CER | RTF | 平均 CPU（占单核，按音频时长算） | 私有内存（加载后 / 峰值） |
|---|---|---|---|---|---|---|---|
| ① small-ctc-zh-int8 | 25.1MB | 1.1–1.5 s | 5.17%（15/290） | 9.45%（53/561） | 0.041 | 4.1% | 74 / 93MB |
| ② zh-14M | 24.2MB | 1.1–1.2 s | 10.34% | 16.93% | 0.033 | 3.2% | 58 / 68MB |
| ③ ctc-multi-zh-hans-int8 | 67.6MB | 2.0 s | 6.21% | 7.31% | 0.090 | 8.9% | 126 / 144MB |
| ④ **X-ASR 480ms int8** | 161.4MB | 5.3–5.6 s | **0.34%**（1/290） | **5.88%** | 0.121 | 12.0% | 236 / 257MB |
| ⑤ 离线 small-ctc-zh-int8（非流式，官方 CLI） | 59.8MB | 4.3–5.8 s | 4.14% | **3.74%** | 0.09 | — | — |

### 流式（模拟麦克风，TTS 8 句正常语速，每句说完再停 600ms 后点「停止」）

| 模型 | 首字延迟（从开口算） | 说完到文字完整 | 点「停止」到出最终结果 | CPU 平均 / 峰值（占单核） | CER |
|---|---|---|---|---|---|
| ① small-ctc | 0.38–1.1 s（两档，和 CTC 的分块边界有关） | 0.46–0.86 s | 37–41ms | 6.3% / 12% | 4.83% |
| ② zh-14M | 0.60–0.69 s | 0.04–0.38 s | 13–32ms | 5.7% / 10% | 10.34% |
| ③ multi-zh-hans | 0.72–1.08 s | 0.46–0.72 s | 25–76ms | 11.7% / 19% | 7.59% |
| ④ X-ASR | 0.80–1.23 s | 0.68–1.09 s | 105–177ms | 15.2% / 27% | 0.00% |

「说完到文字完整」：从语音能量结束算起，到识别中的文字第一次和最终结果一致为止。小模型的最后一两个字，往往要等 0.5–1 秒的尾部静音，或者等用户点「停止」后才出来。点「停止」以后 `finish()` 很快（40–180ms），所以用户的体感是：**点停止，文字马上上屏**。

### 识别结果对比（TTS，正常语速）

| 原句 | ① small-ctc 25MB | ② zh-14M 24MB | ④ X-ASR 161MB |
|---|---|---|---|
| 今天下午三点在三号会议室开会，记得带上电脑。 | 今天下午三点在三号会议**时**开会记得带上电脑 | 今天下午三点在三号会议室开会记得带上电脑 | 今天下午三点在三号会议室开会，记得带上电脑 |
| 帮我订一张明天早上八点半去上海的高铁票。 | 帮我**定**一张明天早上八点半去上海的高铁票 | 帮我**定义**明天早上八点半去上海的高铁票 | 帮我订一张明天早上八点半去上海的高铁票 |
| 这个周末天气不错，我们一起去公园散步吧。 | 这个周末天气不错我们一起去公园散步吧 | 这个周末天气不错我们一起去公园散**布**吧 | 这个周末天气不错，我们一起去公园散步吧 |
| 今年的销售额比去年增长了百分之二十五。 | 今年的销售额比去年增长了百分之二十五 | 今年的销售额比去年增长了~~百分之~~二十五 | 今年的销售额比去年增长了百分之二十五 |
| 请把这份PPT发到我的邮箱里，谢谢。 | 请把这份~~PPT~~发到我的邮箱里谢谢 | 请把这份**理**发到我的邮箱里谢谢 | 请把这份PPT发到我的邮箱里，谢谢 |
| 我觉得这个方案还可以再优化一下，你看怎么样？ | 我觉得这个方案还可以再优化一下你看怎么样 | 我觉得~~这个~~方案还可以再优化一下你看怎么样 | 我觉得这个方案还可以再优化一下，你看怎么样？ |
| 这个功能已经OK了，明天就可以上线。 | 这个功能已经~~OK~~了明天就可以上线 | 这个功能已经**可以**了明天就可以上线 | 这个功能已经OK了，明天就可以上线 |
| 我的验证码是四七九二一六，你帮我输一下。 | 我的验证码是四七九二一六你帮我输一下 | 我的验证码是四七九二一六你帮我**梳**一下 | 我的验证码是四七九二一六，你帮我输一下 |

- 数字都输出成汉字。要转成「25%」「479216」，可以用 sherpa-onnx 的 ITN（`rule_fsts`，例如 `itn_zh_number.fst`）。
- X-ASR 原始输出里有多余的空格（`这份 P P T 发到`、`开会， 记得`），原型里的 `text::tidy` 会把它们去掉。

AISHELL 真人朗读上的典型错误：

- 人名、专名：「薛之谦的歌儿很棒」→ ① 识别成「徐知签了哥很棒」，④ 识别成「须知牵着歌很棒」；「朱婷」→「朱廷」。
- 同音字：「后市」→「后世」，「债券人」→「债权人」。
- 句尾丢字：常见于没补够尾部静音的时候，见下文。

中英混说的真人课堂录音（X-ASR 自带的 test_wavs）：

- ① 输出：「昨天是妈的`<unk><unk>` 礼拜二`<unk>`… 是星期」。
- ④ 输出：「昨天是 Monday， today is 礼拜二， the day after tomorrow 是…」。

小模型基本不能处理英文。

### 线程数和进程优先级

| 设置 | ① small-ctc 在 AISHELL 上（RTF / CPU 时间） |
|---|---|
| 1 线程，below normal | 0.043 / 9.1 s |
| 1 线程，normal | 0.035 / 7.4 s |
| 2 线程，below normal | **0.108 / 28.4 s**（CPU 时间是 1 线程的 3 倍，速度反而慢了 2.5 倍） |
| 2 线程，normal | 0.027 / 11.6 s |
| X-ASR 2 线程，below normal | **0.625 / 167.6 s**（单核占用率 78%） |

- onnxruntime 的多线程会自旋等待，在低优先级下被打断以后，CPU 时间大量浪费。**一律用 1 个线程**，小模型 1 个线程已经比实时快 10–25 倍。
- below normal 优先级在这颗大小核 CPU 上有时会被排到能效核：第一次用官方 CLI 时，同一句话 RTF 0.33，加载 4 秒；normal 优先级下 RTF 0.04，加载 1.2 秒。
- 产品里用户正在说话时，识别线程应该用普通优先级（只在说话期间占用，每秒约 60ms CPU）。不说话时进程不存在或者不工作，空闲 CPU 为 0。

### 踩到的坑（已在原型里处理）

1. **句尾丢字。** 流式模型要看到足够多的「右侧上下文」，才会输出最后一两个字。只调用 `InputFinished` 会丢字，sherpa-onnx 的官方示例也会先补 0.3–0.66 秒静音。
   - `finish()` 时补的静音长度对 AISHELL CER 的影响：
     - small-ctc：不补静音时，模型包里的录音常常丢掉最后一两个字；补 300、600、1000ms 时都是 9.45%；
     - X-ASR：300ms 时 10.34%，600ms 时 7.13%，1000ms 时 5.88%，1500ms 时也是 5.88%。
   - 原型默认补 500ms（`Options::tail_padding_ms`），X-ASR 要设成 1000ms。补静音只多花 40–120ms 计算。
2. **multi-zh-hans 句首丢字。** 加了 300ms 前置静音以后，仍然有大约 4 成句子丢第一个字（「公司将」→「司将」），疑似模型或 BPE 的问题，没有深究。去掉这个因素后，它的 AISHELL CER 约 4.6%，但它有 68MB，也不在目标体积内。
3. **`<unk>`。** 小模型遇到英文就输出 `<unk>`，原型的 `text::tidy` 会把它删掉。
4. **加载时间。** 加载时间主要花在 onnxruntime 建图和优化上，和模型大小大致成正比：25MB 约 1.2 秒，161MB 约 5.5 秒。解决办法：一开始就采集音频并先缓存，模型加载好以后再把缓存的音频一次性喂进去，用户不用等。

### 局限

- TTS 合成音比真人发音清晰，所以 TTS 上的 CER 偏乐观。AISHELL 是安静环境下的朗读新闻，也比真实听写容易，而且 k2 系列小模型的训练数据里可能有 AISHELL 训练集（领域相同）。
- 真实的口语、噪声、口音，以及用户自己的声音，都还没有测。
- 可以以后让用户在设置里用「本地识别测试」自己试一下，录音只在本机处理，不保存。
- 这次没有和微信输入法在同一批音频上对比（WeType 走云端，需要真麦克风）。不过 WeType 带标点、中英混说，听写体验明显更好。至少可以确定，小模型达不到它的水平。

## 在点墨里的实现方案

### 形态：按需启动的辅助进程

建议用按需启动的辅助进程 `dianmo-voice.exe`，不在主进程里加载 DLL。

- 内存能全部回收：onnxruntime 的内存池和线程池用 `FreeLibrary` 卸不干净，进程退出才能保证全部归还。X-ASR 在用时约 255MB，这符合「空闲内存小」的硬约束。
- 崩溃和高负载都隔离在辅助进程里，点墨主进程照常响应触摸。
- 由点墨启动，权限级别相同。上屏仍由点墨主进程做（复用 `TextSink`），不需要第二个进程去碰目标窗口。

### 流程

1. 用户按麦克风键或语音球，`VoiceEngine::Local::start()`：
   - 如果辅助进程不在，就启动它，参数是模型目录（stdin/stdout 用管道连接，`CREATE_NO_WINDOW`）。
   - 辅助进程**先打开麦克风**，再加载模型。加载期间的音频先缓存起来，用户可以马上开口。
   - 语音球进入「正在听」状态。
2. **采集**：WASAPI 共享模式，`AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUTOCONVERTPCM | SRC_DEFAULT_QUALITY`，直接要 16kHz 单声道 float，每 100ms 喂给识别器一次。
   - 读取失败、被拒绝（Win10 设置里关了「允许桌面应用访问麦克风」）时，回报错误，语音球给出提示。
3. **识别**：每次识别中的文字变化，就输出一行 `P <文字>`。点墨把它显示在候选条（灰色，像未上屏的拼音）或语音球的气泡里。
   - 断句：先用 sherpa-onnx 自带的端点检测（尾部静音 1.2 秒算一句，不需要额外模型）。每断一句就输出 `F <这句的最终文字>`，点墨立即上屏，然后清空候选条。这样长段听写时，候选条也不会越来越长。
   - silero VAD（`silero_vad.onnx` 0.6MB）是可选项，作用有两个：
     - 判断用户长时间没说话（例如 5 秒），自动结束；
     - 不把长段静音喂给识别器，进一步省 CPU。
4. **结束**：用户再点一次时，`stop()` 发出 `S`；辅助进程调用 `finish()`（补尾部静音），输出最后一个 `F` 和 `D`。点墨上屏，语音球回到空闲。
   - 取消：`C`，丢弃当前句。
5. **上屏前清理文字**：
   - 用 `text::tidy` 删掉 `<unk>`、去掉中英文之间的空格。
   - 可选：ITN 把汉字数字转成阿拉伯数字。
   - 没有标点的模型：在端点处补「，」，句末补「。」；以「吗、呢」结尾时补「？」。这是规则做法，比 61MB 的标点模型划算。
6. **退出**：空闲 N 秒后（默认 180 秒，可以设置）辅助进程自己退出，释放全部内存。
   - 在这期间再次语音，就省掉加载时间。
   - 点墨退出时，会关闭管道，辅助进程跟着退出。
7. **优先级**：辅助进程用普通优先级，onnxruntime 用 1 个线程。

协议是纯文本行（UTF-8）：

- 点墨发给辅助进程：`S`（停止）、`C`（取消）、`Q`（退出）。
- 辅助进程发给点墨：
  - `R`：就绪；
  - `L <0-100>`：音量，给语音球做动画，每 100ms 一次；
  - `P <文字>`：识别中；
  - `F <文字>`：一句的最终结果；
  - `D`：结束；
  - `E <错误>`：出错。

### 和现有语音设计的衔接

- `docs/research/voice.md` 里的 `VoiceEngine` 增加 `Local`，设置项 `voice.engine = "local"`。新增设置：`voice.local_model`（`x-asr` | `small`），以及空闲退出秒数。
- `available()`：模型目录和 DLL 都在，并且通过 SHA-256 校验。
- `start/stop/cancel` 走上面的管道协议，不需要 `MicWatch` 和剪贴板兜底。
- 识别中的文字显示：dianmo-ui 新增一个「语音识别中」的候选条状态，显示灰色文字和音量动画。由 `Action` 驱动，和 rime 的候选互斥。

### 随包体积与分发

| 内容 | 原始大小 | 7z 压缩 |
|---|---|---|
| 运行库 `sherpa-onnx-c-api.dll` + `onnxruntime.dll` | 20.1MB | 4.3MB |
| X-ASR int8（encoder/decoder/joiner/tokens） | 161.4MB | 121.6MB |
| small-ctc int8 | 25.1MB | 18.7MB |
| silero VAD（可选） | 0.6MB | 0.4MB |
| `dianmo-voice.exe` | 约 0.5MB | — |

- 建议**安装包不带模型**。设置里选「本地识别（离线）」时，再下载运行库和模型，并显示进度。
  - 下载源：GitHub release，再加我们 R2 上的镜像（`get.jiangshu.ai`），方便国内用户。
  - 下载后校验 SHA-256，固定 sherpa-onnx 1.13.8。
- 许可：
  - sherpa-onnx：Apache-2.0；onnxruntime：MIT；X-ASR：Apache-2.0。
  - small-ctc 模型没有写明许可。如果要随包或从我们的镜像分发，先向上游确认。

### 工作量估计

| 项 | 工作量 |
|---|---|
| `dianmo-asr` 补全：端点/分句 API、可选 VAD、无标点模型的补标点规则、ITN、单元测试 | 0.5–1 天 |
| `dianmo-voice` 辅助进程：WASAPI 采集、缓存音频的同时加载模型、管道协议、空闲退出 | 1.5 天 |
| 点墨接入：`VoiceEngine::Local`、语音球和候选条显示识别中的文字、上屏、设置页、模型下载和校验 | 2 天 |
| Surface 真麦克风联调（用户本人试用）、调参（尾部静音、端点、空闲时长） | 0.5–1 天 |
| **合计** | **约 5 天** |

后续可以做的优化（不在这 5 天内）：

- X-ASR 换成 160ms 块的导出版本：首字更快，但 CER 会升高，需要实测。
- 让 onnxruntime 保存优化后的模型，缩短 5 秒的加载时间。sherpa-onnx 的 C API 目前没有提供这个接口，需要改上游或者自己编译。
- 支持热词（transducer 模型配 `modified_beam_search`）。

## 原型：`crates/dianmo-asr`

- 运行时用 `LoadLibraryExW` 加载 `sherpa-onnx-c-api.dll`，按名字解析 13 个 C 函数，不链接 MSVC。
  - `ffi.rs` 里的 `#[repr(C)]` 结构是照 1.13.8 的 `c-api.h` 写的。用 gcc 对头文件核对过 `sizeof`：`OnlineRecognizerConfig` 272 字节，有单元测试守着。
- API：
  - `Recognizer::new(model_dir)` / `with_options(model_dir, &Options { threads, dll_dir, endpoint, tail_padding_ms })`；
  - `accept_waveform(&[f32])`：16kHz，喂进去后把凑够的块都识别掉；`accept_waveform_at(rate, …)` 接受其他采样率；
  - `partial()`：识别中的文字，经过 `text::tidy`；`raw_partial()`：模型原始输出；
  - `is_endpoint()`、`finish()`（补尾部静音，返回最终文字，开始新的一句）、`reset()`。
- 模型目录里的文件按名字自动识别（`ModelFiles::detect`）：
  - transducer：`encoder*/decoder*/joiner*.onnx`；
  - zipformer2 CTC：`model*.onnx` / `ctc*.onnx`；
  - 优先用 int8。上面实测的 4 个流式模型都能直接加载。
- `wav.rs`：最小的 WAV 读取，支持 PCM 8/16/24/32 位和 float，多声道取第一个声道。`eval.rs`：CER 计算。
- `examples/asr_wav.rs`：命令行识别 wav，统计上面表格里的各项指标，没有界面。
- 服务器上：`cargo test -p dianmo-asr` 12 个测试全部通过；`cargo clippy --target x86_64-pc-windows-gnullvm` 没有警告。
- Surface 上：`scripts/asr/build.sh` 构建 release 版，`asr_wav.exe` 366KB，已经跑通全部测试。

## 脚本（`scripts/asr/`）

| 脚本 | 作用 |
|---|---|
| `fetch.ps1` | 在 Surface 上下载 sherpa-onnx 运行库、模型和 silero VAD，放到 `C:\dev\dianmo-asr\{sherpa,models}` |
| `tts.ps1` + `sentences.txt` | 用 Windows TTS 生成测试句子的 wav 和 `wavs\list.tsv` |
| `aishell.ps1` | 下载 AISHELL-1 测试集样本（39 条），生成 `wavs\aishell.tsv` |
| `build.sh` | 在 Surface 上构建 `asr_wav.exe`，装到 `C:\dev\dianmo-asr\bin`。构建目录用 `C:\dev\dianmo-asr-build`，**不能用** `build.sh asr`：sync 会清空 `C:\dev\dianmo-asr` 里除 target 以外的所有内容，模型会被删掉 |
| `bench.ps1` / `run.sh` | 用低优先级批量运行 `asr_wav.exe`。例如 `scripts/asr/run.sh -List wavs\\aishell.tsv -Models <模型目录名> [-Realtime -TailMs 600] [-PadMs 1000]`。结果写在 `C:\dev\dianmo-asr\results\` |
| `offline.ps1` | 用官方 `sherpa-onnx-offline.exe` 测非流式模型的 CER |

## 来源

- sherpa-onnx 预训练模型和运行库：[GitHub releases asr-models](https://github.com/k2-fsa/sherpa-onnx/releases/tag/asr-models)、[v1.13.8](https://github.com/k2-fsa/sherpa-onnx/releases/tag/v1.13.8)
- [流式 Zipformer-CTC 模型文档](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/online-ctc/zipformer-ctc-models.html)、[流式 Zipformer-transducer 模型文档](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/online-transducer/zipformer-transducer-models.html)、[小模型列表](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/small-online-models.html)
- small-ctc 的许可问题：[k2-fsa/sherpa-onnx#3915](https://github.com/k2-fsa/sherpa-onnx/issues/3915)
- X-ASR：[GilgameshWind/X-ASR-zh-en](https://huggingface.co/GilgameshWind/X-ASR-zh-en)（指标、许可、参数量）、[cstr/x-asr-zh-en-GGUF](https://huggingface.co/cstr/x-asr-zh-en-GGUF)
- Vosk 模型和 CER：[alphacephei.com/vosk/models](https://alphacephei.com/vosk/models)
- Moonshine：[Flavors of Moonshine](https://arxiv.org/pdf/2509.02523)、[Moonshine v2](https://arxiv.org/abs/2602.12241)
- 中文开源 ASR 概览：[腾讯云开发者社区](https://cloud.tencent.com/developer/article/2642961)
- AISHELL-1 测试集样本：[AudioLLMs/aishell_1_zh_test](https://huggingface.co/datasets/AudioLLMs/aishell_1_zh_test)
