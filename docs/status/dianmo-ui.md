# dianmo-ui 状态（2026-10-06，已在 Surface 实机检查）

## 已完成
- `KeyboardView` 实现 `View`，纯逻辑，Linux 上能测；`cargo test -p dianmo-ui` 34 个测试全部通过。
- 布局（数据表在 `src/layout.rs`）：26 键全拼；小鹤双拼（键面小字标韵母，按官方小鹤表）；九宫格（左列：输入中显示 `set_t9_spellings` 给的拼音，空闲时显示常用标点；右列 ⌫ / 重输 / 回车）；英文 26 键（Shift 单击大写一个字母、双击锁定）；数字键盘；符号面板（中文 / 英文 / 表情三个页签，可上下滑动）；布局菜单（全拼、小鹤、九宫格、English、深色/浅色）。
- 底行：符号、123、布局（⌨）、空格（淡色显示方案名）、中/英、标点（中文「，」/ 英文「,」）、回车（强调色）。
- 候选条：拼音显示在左上角（小字），候选横向排列，拖动滚动，松手有惯性；点击发 `Select(绝对序号)`；滑到末尾或展开时发 `WantMoreCandidates`；点展开箭头进入全屏候选网格（右侧一列：收起、⌫、重输、回车）。不在输入时显示工具栏：🎤 语音、布局、表情、收起键盘。
- 手势：按指针分别跟踪（两只拇指交叠也不丢键；第二根手指按下时，第一根手指按着的字符先上屏，保证顺序）；字符键松手才触发；按下时有放大气泡；长按 350ms 或上滑输入右上角的副字符（qwerty 那一行是 1–0）；有多个候选字符的键长按弹出小面板，左右滑动选择；手指滑到相邻键时以松手位置的键为准；退格按下即删，400ms 后开始连删并逐渐加速，向左滑清空输入；空格左右拖动发 `Edit(Left/Right)`，长按 600ms 后松手触发语音；判定按键时同一行的键优先，其次取最近的键。
- 有浅色和深色两套主题（`ThemeKind`）；空闲时不开定时器，状态没变时不重绘。
- 开发用的预览渲染：`examples/preview.rs`（tiny-skia + ab_glyph，只作为 dev-dependency）。

## Surface 实机检查（D2D Canvas，雅黑，200%）
- 用主程序截图检查了：全拼 / 小鹤（键面韵母）/ 九宫格（深色，含输入中左列拼音）/ 符号中文页 / 表情页 / 数字面板 / 布局菜单 / 候选条 / 展开的候选网格。
- 雅黑字号合适，字母和汉字垂直居中正常；MDL2 码位都对（E752 Shift 箭头、E765 键盘、麦克风、表情、退格、回车、展开/收起箭头）；表情是彩色的；候选注释（如「词库未加载」）正常显示。
- 修正：全角标点「，。、．」的字形在 em 框左下角，居中画时看起来偏左下；现在键面、符号格、九宫格左列、气泡和长按面板都把它们右移、上移 0.24em（`draw::punct_centered`），实机看已居中。
- 弯引号 “ ” ‘ ’ 在雅黑里本来就是窄的斜撇，看起来像 ″，属于字体风格，没改。

## 公开 API
- `KeyboardView::new(KeyboardConfig { theme, schema, chinese })`，以及 `Default`。
- `set_theme(ThemeKind)`、`theme()`、`layout() -> Layout`。
- `key_center(name) -> Option<(f32, f32)>`：返回按键中心坐标，给 Surface 上的 GUI 自动化测试用。
- 宿主用：`set_height_scale(f32)`（0.7–1.5，乘在 preferred_height 上）/ `height_scale()`、`show_numbers()`（数字输入框）/ `show_letters()`，都返回是否有变化。
- `View::as_any_mut()`（默认 None，KeyboardView 返回自己）：App 通过 `&mut dyn View` 拿到 `KeyboardView`（托盘切主题、切面板）。
- `UiAction::ThemeChanged(ThemeKind)`：布局菜单里切了主题（已经应用），宿主只需保存。
- 常量 `MORE_BATCH`（每次请求 60 个候选）。导出的类型有 `Layout`、`SymTab`、`Theme`、`ThemeKind`。
- 宿主要处理 `set_input_state` / `set_more_candidates` / `set_t9_spellings` 返回的 `Response.actions`（`WantT9Spellings`、`WantMoreCandidates` 就是从这几处发出的）。
- 启动时宿主应先对 controller 执行 `SetSchema(config.schema)`；之后键盘以 `set_input_state` 传来的状态为准。

## 运行
- 测试：`cd ~/yzfly/dianmo && flock /tmp/heavy.lock nice -n 10 cargo test -p dianmo-ui`
- 预览：`flock /tmp/heavy.lock nice -n 10 cargo run -p dianmo-ui --example preview -- <输出目录>`（需要 Noto Sans CJK 和 DejaVu Sans；图标是近似画出来的，表情显示成黄点）。
- 现有 PNG（1440 DIP 宽、2 倍分辨率）：`/tmp/claude-1000/-home-ubuntu-yzfly-temp/e2a7d031-78b7-44af-89e3-a494d7f0a2a4/scratchpad/ui-preview/`，文件编号 01–14 为浅色，20–23 为深色，30–31 为竖屏。
- 注意：Cargo.lock 还没有加入 tiny-skia / ab_glyph，第一次构建时会自动加上。这期间别的 crate（dianmo-rime）的清单还不完整，所以我在 scratchpad 里用一个隔离的 workspace 跑的测试。

## 未完成 / 下一步
1. 上 Surface 实机验证（用 D2D Canvas）：字号和垂直居中（预览用的 Noto，和雅黑的度量不同）；MDL2 码位是否正确（E752 Shift、E765 键盘）；表情要用 `D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT` 才有颜色（由 dianmo-win 负责）。
2. 中文模式下的 Shift 目前直接输出大写字母；有没有必要像手机那样做「中文大写锁定」，看用户反馈。
3. ~~菜单里切换的主题没有持久化~~：已加 `UiAction::ThemeChanged`，主程序保存到 settings.ini。
4. 还没做：剪贴板、成对符号（“”自动把光标放到中间）、九宫格英文、分体/悬浮形态、按键音和震动、动画（气泡淡入淡出）。
5. 符号面板点一次符号后停在面板里，不会自动返回字母键盘（同微信）；如果用户希望点完就返回，再改。
6. clippy 有 7 条「可合并 if」的风格提示，没有影响。

## 接口变更（2026-10-06，都是新增）
- view.rs：`UiAction::ThemeChanged(ThemeKind)`、`View::as_any_mut()`（有默认实现）。canvas.rs 没有改。
- 以后可以考虑给 `Canvas` 加 `fill_gradient` 或阴影模糊（现在用叠几层半透明矩形近似）。
