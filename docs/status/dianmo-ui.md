# dianmo-ui 状态（2026-10-06，已在 Surface 实机检查）

## 已完成
- `KeyboardView` 实现 `View`，纯逻辑，Linux 上能测；`cargo test -p dianmo-ui` 48 个测试全部通过（手机布局的测试在 960 宽下跑，宽屏的在 1440 宽下跑）。
- 布局（数据表在 `src/layout.rs`）：26 键全拼；小鹤双拼（键面小字标韵母，按官方小鹤表）；九宫格（左列：输入中显示 `set_t9_spellings` 给的拼音，空闲时显示常用标点；右列 ⌫ / 重输 / 回车）；英文 26 键（Shift 单击大写一个字母、双击锁定）；数字键盘；符号面板（中文 / 英文 / 表情三个页签，可上下滑动）；布局菜单（全拼、小鹤、九宫格、English、深色/浅色）。
- 底行：符号、123、布局（⌨）、空格（淡色显示方案名）、中/英、标点（中文「，」/ 英文「,」）、回车（强调色）。
- 候选条：拼音显示在左上角（小字），候选横向排列，拖动滚动，松手有惯性；点击发 `Select(绝对序号)`；滑到末尾或展开时发 `WantMoreCandidates`；点展开箭头进入全屏候选网格（右侧一列：收起、⌫、重输、回车）。不在输入时显示工具栏：🎤 语音、布局、表情、收起键盘。
- 手势：按指针分别跟踪（两只拇指交叠也不丢键；第二根手指按下时，第一根手指按着的字符先上屏，保证顺序）；字符键松手才触发；按下时有放大气泡；长按 350ms 或上滑输入右上角的副字符（qwerty 那一行是 1–0）；有多个候选字符的键长按弹出小面板，左右滑动选择；手指滑到相邻键时以松手位置的键为准；退格按下即删，400ms 后开始连删并逐渐加速，向左滑清空输入；空格左右拖动发 `Edit(Left/Right)`，长按 450ms 进入触控板（见下）；判定按键时同一行的键优先，其次取最近的键。
- 有浅色和深色两套主题（`ThemeKind`）；空闲时不开定时器，状态没变时不重绘。
- 开发用的预览渲染：`examples/preview.rs`（tiny-skia + ab_glyph，只作为 dev-dependency）。

## 宽屏布局与电脑按键（TODO #19 / #22 / #26，2026-10-06）
设计见 DESIGN.md §2「Surface 宽屏布局」「电脑按键」（已按实现更新）。键盘宽度 ≥ 1100 DIP（`layout::WIDE_MIN_WIDTH`）用宽屏布局，`Metrics { wide, rows }`；窄屏/竖屏仍是手机布局，原有手机测试不变。
- **26 键五行**（`build_letters_wide`）：Esc 数字行 `- =` ⌫ / Tab q–p 【】、 / 大写 a–l ；“ ⏎ / ⇧ z–m ，。？ ⇧ / Ctrl Win Alt Fn 符号 空格 中/英 ← ↑↓ → 收起；右侧两列常驻编辑区（撤销 重做 / 全选 清空 / 复制 粘贴 / 剪切 删词 / 行首 行尾）。键宽单位 = 字母区 / 14.5，开着编辑区约 86 DIP。符号键 shift 换上档；字母角标是符号；小鹤韵母保留。
- **修饰键**（`KeyAction::Mod(Modifier)`，`latch: [Latch; 5]` + 触点按住）：点一下单次、350ms 内双击锁定、按住 + 另一指点键 = 组合（用过的按住不会松手后亮起）；Win 单点 = 按 Win，长按亮起；Ctrl/Win 亮起时字母键下方用强调色小字提示快捷键。Fn 层：数字行 → F1–F12，方向 → Home/End/PgUp/PgDn，⌫ → Del，Esc → `` ` ``。
- **组合键**：有 Ctrl/Alt/Win 时字母/数字/标点（全角标点按所在物理键，`Key::code`）、空格、⏎、⌫、Tab、方向键都发 `Action::Key(KeyChord)`；只有 Shift 时方向键/Tab/⏎ 也带 Shift。
- **按下即发**：⌫、方向键、Del、行首行尾按下就发并连发（`Touch::repeat_action`）；带 `Key::hold` 的键松手才发，长按发 hold 动作（Tab → Esc，工具栏 ← → 行首行尾）。
- **数字面板**（宽屏）：限宽居中，左运算符 3×3，右 Tab/@/#/¥/&/_/行首/行尾 + 方向键。123 面板在宽屏下从符号面板「123」页签进入（也可 `show_numbers()`）。
- **九宫格**（宽屏）：九宫格本体不拉伸，右边加 2×4 标点和数字小键盘（数字直接上屏）。
- **电脑键面板**（`Panel::PcKeys`，窄屏和九宫格的工具栏「电脑键」按钮打开）：F1–F12 / Esc Tab Home End PgUp PgDn Del ⌫ / ⇧ Ctrl Alt Win PrtSc 空格 ↑ ⏎ / 返回 `` ` - = [ ] \ ' `` ← ↓ →。
- **工具栏**：空闲时左边编辑工具（撤销…粘贴，窄屏/九宫格再加 ← →），右边语音、布局、电脑键、表情、收起；宽屏 26 键开着编辑区时只留右边。放不下时从右往左丢编辑工具。
- **候选条**（宽屏）：候选 24 DIP、首选强调色加粗 + 浅色圆角底；注释小字 14 DIP；候选网格格子 2 倍键高宽、右栏加「上页 / 下页」（按整行翻页）。
- **高度**：`preferred_height` 宽屏 = 候选条 + 5 × 行高（行高 × 高度档位，不低于 52 DIP）；1440 宽标准档 353 DIP（屏高 37%），0.7 档键高仍 52。

## Surface 实机检查（D2D Canvas，雅黑，200%）
- 用主程序截图检查了：全拼 / 小鹤（键面韵母）/ 九宫格（深色，含输入中左列拼音）/ 符号中文页 / 表情页 / 数字面板 / 布局菜单 / 候选条 / 展开的候选网格。
- 雅黑字号合适，字母和汉字垂直居中正常；MDL2 码位都对（E752 Shift 箭头、E765 键盘、麦克风、表情、退格、回车、展开/收起箭头）；表情是彩色的；候选注释（如「词库未加载」）正常显示。
- 修正：全角标点「，。、．」的字形在 em 框左下角，居中画时看起来偏左下；现在键面、符号格、九宫格左列、气泡和长按面板都把它们右移、上移 0.24em（`draw::punct_centered`），实机看已居中。
- 弯引号 “ ” ‘ ’ 在雅黑里本来就是窄的斜撇，看起来像 ″，属于字体风格，没改。

## 公开 API
- `KeyboardView::new(KeyboardConfig { theme, schema, chinese })`，以及 `Default`。
- `set_theme(ThemeKind)`、`theme()`、`layout() -> Layout`。
- `key_center(name) -> Option<(f32, f32)>`：返回按键中心坐标，给 Surface 上的 GUI 自动化测试用。名字可以是键面文字（"a"、"1"、"，"、"F5"）或 shift ctrl alt win fn caps esc tab del left right up down home end undo redo selectall copy paste cut delword clear pc pageup pagedown symbols numbers backspace enter space layout toggle expand voice hide。
- `set_edit_area(bool) -> bool`（是否变化）/ `edit_area()`：宽屏编辑区开关（用户设置，默认开）；`is_wide()`。
- 新的 `Action::Key(KeyChord)` 会从 `UiAction::Input` 发出，宿主照常交给 controller。
- 宿主用：`set_height_scale(f32)`（0.7–1.5，乘在 preferred_height 上）/ `height_scale()`、`show_numbers()`（数字输入框）/ `show_letters()`，都返回是否有变化。
- `View::as_any_mut()`（默认 None，KeyboardView 返回自己）：App 通过 `&mut dyn View` 拿到 `KeyboardView`（托盘切主题、切面板）。
- `UiAction::ThemeChanged(ThemeKind)`：布局菜单里切了主题（已经应用），宿主只需保存。
- 常量 `MORE_BATCH`（每次请求 60 个候选）。导出的类型有 `Layout`、`SymTab`、`Theme`、`ThemeKind`。
- 宿主要处理 `set_input_state` / `set_more_candidates` / `set_t9_spellings` 返回的 `Response.actions`（`WantT9Spellings`、`WantMoreCandidates` 就是从这几处发出的）。
- 启动时宿主应先对 controller 执行 `SetSchema(config.schema)`；之后键盘以 `set_input_state` 传来的状态为准。

## 运行
- 测试：`cd ~/yzfly/dianmo && flock /tmp/heavy.lock nice -n 10 cargo test -p dianmo-ui`
- 预览：`flock /tmp/heavy.lock nice -n 10 cargo run -p dianmo-ui --example preview -- <输出目录>`（需要 Noto Sans CJK 和 DejaVu Sans；图标是近似画出来的，表情显示成黄点）。
- 预览场景：01–19 为 1440 宽（宽屏，15 关编辑区、16 Ctrl 提示、17 Fn 层、18 英文 Shift、19 简拼），20–23 深色宽屏，30–33 为 960 宽竖屏（32 空闲工具栏、33 电脑键面板）。
- 关键图在 `docs/previews/`：改前 `wide-before-*.png`（手机布局拉宽），改后 `02-pinyin-composing.png`、`wide-idle-edit-area.png`、`wide-idle-toolbar.png`（关编辑区）、`wide-numbers.png`、`wide-t9.png`、`wide-ctrl-hints.png`、`wide-fn-layer.png`、`wide-candidate-grid.png`、`portrait-pc-keys.png`。
- 注意：Cargo.lock 还没有加入 tiny-skia / ab_glyph，第一次构建时会自动加上。这期间别的 crate（dianmo-rime）的清单还不完整，所以我在 scratchpad 里用一个隔离的 workspace 跑的测试。

## 未完成 / 下一步
0. 宽屏遗留：简拼候选的全拼注释要 dianmo-rime 提供（rime_ice 的 corrector 清掉了拼音注释，UI 有注释就会显示）；锁定 Alt 连点 Tab 只能在两个窗口间切（组合键是一次性发完的，见 DESIGN.md）；预览里的 MDL2 新图标（撤销/重做/全选/剪切/复制/粘贴/左右箭头）是近似画的，码位要在 Surface 实机上确认。
1. 上 Surface 实机验证（用 D2D Canvas）：字号和垂直居中（预览用的 Noto，和雅黑的度量不同）；MDL2 码位是否正确（E752 Shift、E765 键盘）；表情要用 `D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT` 才有颜色（由 dianmo-win 负责）。
2. 中文模式下的 Shift 目前直接输出大写字母；有没有必要像手机那样做「中文大写锁定」，看用户反馈。
3. ~~菜单里切换的主题没有持久化~~：已加 `UiAction::ThemeChanged`，主程序保存到 settings.ini。
4. 还没做：剪贴板、成对符号（“”自动把光标放到中间）、九宫格英文、分体/悬浮形态、按键音和震动、动画（气泡淡入淡出）。
5. 符号面板点一次符号后停在面板里，不会自动返回字母键盘（同微信）；如果用户希望点完就返回，再改。
6. clippy 有 7 条「可合并 if」的风格提示，没有影响。

## 电脑键盘、选择、剪贴板（TODO #31 / #32，2026-10-06）
设计见 DESIGN.md §2「电脑键盘布局」「选择、复制与剪贴板」（已按实现更新）。`cargo test -p dianmo-ui` 60 个测试（新增 12 个）。
- **电脑键盘**（`src/pc.rs`，`layout::build_pc_keyboard`，`Layout::Pc`，`KeyAction::Raw(KeyCode)`）：六行 15 键宽，功能键行 3/4 高；`Metrics::pc` 保持窗口高度不变，细栏 28–38 DIP。Raw 键按下发 `Action::KeyDown`、松手 `KeyUp`，按住 400ms 后每 40ms（`PC_REPEAT_MS`）重发 `KeyDown`；Cancel / 离开布局会补发 `KeyUp`。修饰键：`Touch::engaged` + `pc_down[]` + `pc_sync()`——单次/锁定的修饰键在作用的键按下前 down、键松手后 up；手指按住的在第一个键按下时 down、松手 up；长按 Shift/Ctrl/Alt 350ms 直接 down；Win 单点 = `Key(win_alone)`、长按 = 亮起。Caps 键切 `pc_caps`（指示灯 + 字母大写）；Shift 时整块键面换上档；Fn：方向 → Home/End/PgUp/PgDn，Del → Ins。顶部细栏：返回（`KeyAction::PcBack`）、语音、收起。布局菜单加了第 6 个磁贴「PC 电脑键盘」。
- **触控板**（`src/clip.rs`）：长按空格 `TRACKPAD_PRESS_MS`=450 → `Mode::Trackpad`，键面隐去；拖动按 14 DIP/字、26 DIP/行发方向键，速度增益最多 5 倍，主轴优先；另一指（`Target::PadAux`）点一下开始选择（之后带 Shift），再点一下 Ctrl+Shift+→；松手若选过东西就进入选择模式。
- **选择模式**：`selecting`；选择栏（`build_select_bar`）按钮 `KeyAction::Press(KeyChord)` 按下即发、按住连发；`Sel(SelAct)` 复制/剪切/粘贴/删除/完成；选择中键盘的方向键、Home/End/PgUp/PgDn 自动加 Shift；打字退出（`typed()`）。编辑区重排为 撤销 重做 / 选择 全选 / 复制 粘贴 / 剪切 删词 / 清空 剪贴板；工具栏加「选择」工具和「剪贴板」图标（E762 / E81C；剪贴板 E81C、关闭 E711 和选择栏的箭头已在实机截图里确认，选择 E762、图钉 E718 还没在实机上看到）。
- **剪贴板栏 / 面板**：`clip_bar`（`notify_copied` 打开，打字 / ✕ / `reset_transient` 关闭）横向卡片 `clip_cards()`，点卡片 `UiAction::Paste(text)`；面板 `Panel::Clipboard` 网格（固定的在前），长按出现 固定/删除（`clip_menu`），`UiAction::PinClip/DeleteClip/ClearClips`；「已复制」toast 画在键区中间，`TOAST_MS` 单次计时器（`finish()` 把 toast 截止时间算进 `timer_ms`）。编辑区粘贴键 `sub` = `paste_preview`。
- **新公开 API**：`set_pc_keyboard(bool) -> bool` / `pc_keyboard()`；`set_clips(Vec<ClipItem>) -> bool` / `clips()`；`set_paste_preview(Option<&str>) -> bool`；`notify_copied(now_ms) -> Response`；`show_toast(text, now_ms) -> Response`；`enter_select_mode() -> bool` / `selecting()`；`reset_transient() -> bool`（键盘隐藏时调用）；`clip_preview(text, max)`。新类型 `ClipItem { id, text, pinned }`。新 `UiAction`：`PcKeyboard(bool)`、`Paste(String)`、`PinClip { id, pinned }`、`DeleteClip(u64)`、`ClearClips`、`CheckCopied`（复制按钮按下，宿主 350ms 后看剪贴板有没有变，没变就 `enter_select_mode`）。
- `key_center` 新名字：pcmode pcback prtsc back select clipboard clipclose clearclips sel_left/right/wordleft/wordright/up/down/home/end/copy/cut/paste/delete/done，`clip0`…（剪贴板栏或面板里的卡片）；Raw 键也能用 esc/tab/del/left/…/backspace/enter/space/caps 找到。
- 预览：40–53 号场景；关键图 `docs/previews/pc-keyboard.png`、`pc-keyboard-shift.png`、`trackpad.png`、`select-bar.png`、`clipboard-bar.png`、`clipboard-panel.png`。
- 遗留：竖屏（960 宽）电脑键盘键高只有约 37 DIP（窗口高度没为它加高）；媒体键、菜单键没做；Caps 指示是点墨自己记的状态。

## 接口变更（2026-10-06，都是新增）
- view.rs：`UiAction::ThemeChanged(ThemeKind)`、`View::as_any_mut()`（有默认实现）。canvas.rs 没有改。
- 以后可以考虑给 `Canvas` 加 `fill_gradient` 或阴影模糊（现在用叠几层半透明矩形近似）。
