//! Content of each settings page (PRODUCT.md §3), built from the model as [`Block`]s.

use super::model::{
    CandidateSize, FuzzyPair, LayoutChoice, Level, LongPress, Page, SettingsAction as A, SettingsModel, ShuangpinScheme,
    Side, Status, ThemeChoice, VoiceEngineChoice, CLIP_LIMITS,
};
use super::model::InputMode;
use super::widgets::{Art, Block, Button, ButtonKind, Card, Chip, Control, Row, SliderSpec, TagKind};

pub(crate) fn build(page: Page, m: &SettingsModel) -> Vec<Block> {
    let mut blocks = build_page(page, m);
    if !m.coming_soon.is_empty() {
        for b in &mut blocks {
            if let Block::Group { rows, .. } = b {
                for r in rows.iter_mut().filter(|r| r.tag.is_none() && m.coming_soon.contains(&r.key)) {
                    r.tag = Some(("下个版本生效".into(), TagKind::Warning));
                }
            }
        }
    }
    blocks
}

fn build_page(page: Page, m: &SettingsModel) -> Vec<Block> {
    match page {
        Page::General => general(m),
        Page::Keyboard => keyboard(m),
        Page::Input => input(m),
        Page::Voice => voice(m),
        Page::Clipboard => clipboard(m),
        Page::About => super::about::build(m),
    }
}

fn group(title: &str, rows: Vec<Row>) -> Block {
    Block::Group { title: Some(title.to_string()), rows, note: None }
}

fn seg<T: Copy + PartialEq>(items: &[(&str, T)], current: T, act: fn(T) -> A) -> (Vec<(String, A)>, Option<usize>) {
    let opts = items.iter().map(|(l, v)| (l.to_string(), act(*v))).collect();
    let sel = items.iter().position(|(_, v)| *v == current);
    (opts, sel)
}

fn layout_row(m: &SettingsModel) -> Row {
    let (opts, sel) = seg(
        &[
            ("全拼", LayoutChoice::Pinyin),
            ("小鹤双拼", LayoutChoice::Shuangpin),
            ("九宫格", LayoutChoice::T9),
            ("English", LayoutChoice::English),
        ],
        m.layout,
        A::SetLayout,
    );
    let r = Row::new("layout", "默认布局").desc("键盘模式下每次弹出时使用的布局").segmented(opts, sel);
    if m.input_mode == InputMode::Keyboard { r } else { r.tag("键盘模式下生效", TagKind::Neutral) }
}

fn general(m: &SettingsModel) -> Vec<Block> {
    let (theme_opts, theme_sel) =
        seg(&[("跟随系统", ThemeChoice::System), ("浅色", ThemeChoice::Light), ("深色", ThemeChoice::Dark)], m.theme, A::SetTheme);
    let mut admin = Row::new("admin_task", "管理员窗口支持")
        .desc("在以管理员身份运行的程序里也能打字")
        .status(if m.admin_task.text.is_empty() { Status::new(Level::Unknown, "正在检测…") } else { m.admin_task.clone() });
    if matches!(m.admin_task.level, Level::Warn | Level::Error) {
        admin = admin.control(Control::Buttons(vec![Button::new("一键修复", ButtonKind::Primary, A::FixAdminTask)]));
    }
    let cards = InputMode::ALL
        .iter()
        .map(|&mode| Card {
            key: format!("mode/{}", mode.name()),
            title: mode.name().to_string(),
            desc: mode.blurb().to_string(),
            art: match mode {
                InputMode::Keyboard => Art::Keyboard,
                InputMode::VoiceBall => Art::VoiceBall,
                InputMode::PcKeyboard => Art::PcKeyboard,
            },
            selected: m.input_mode == mode,
            act: A::SetInputMode(mode),
        })
        .collect();
    vec![
        Block::Cards { title: Some("输入模式".into()), cards },
        Block::Group {
            title: None,
            rows: vec![
                layout_row(m),
                Row::new("auto_show", "点输入框时自动弹出键盘")
                    .desc("手指点到可以打字的地方就弹出，点别处自动收起")
                    .switch(m.auto_show, A::SetAutoShow),
            ],
            note: Some("也可以在托盘菜单、键盘工具栏和语音球上切换模式：长按语音球展开键盘，键盘上点「语音球」缩回去。".into()),
        },
        group(
            "启动",
            vec![
                Row::new("autostart", "开机自动启动").desc("登录 Windows 后在后台待命，不占用前台").switch(m.autostart, A::SetAutostart),
                admin,
            ],
        ),
        group(
            "外观",
            vec![
                Row::new("theme", "主题").desc("键盘和设置窗口的配色").segmented(theme_opts, theme_sel),
                Row::new("reserve_space", "为键盘留出屏幕空间")
                    .desc("键盘停靠在底部时，其他窗口自动让开，不被遮住")
                    .switch(m.reserve_space, A::SetReserveSpace),
            ],
        ),
        group(
            "其他",
            vec![Row::new("reset", "恢复默认设置").desc("所有设置回到刚安装时的样子，词库和剪贴板不受影响").control(
                Control::Confirm {
                    id: "reset",
                    label: "恢复默认".into(),
                    question: "确定把所有设置恢复为默认值？".into(),
                    confirm: "恢复".into(),
                    act: A::ResetDefaults,
                },
            )],
        ),
    ]
}

fn height_label(v: f32) -> String {
    format!("{}%", (v * 100.0).round() as i32)
}

fn keyboard(m: &SettingsModel) -> Vec<Block> {
    let (lp_opts, lp_sel) =
        seg(&[("短", LongPress::Short), ("中", LongPress::Medium), ("长", LongPress::Long)], m.long_press, A::SetLongPress);
    let (side_opts, side_sel) = seg(&[("靠左", Side::Left), ("靠右", Side::Right)], m.ball_side, A::SetBallSide);
    let mut side = Row::new("ball_side", "悬浮球位置").desc("语音球默认停靠在屏幕哪一边，拖动后会记住").segmented(side_opts, side_sel);
    if !m.ball {
        side = side.disabled();
    }
    vec![
        group(
            "尺寸",
            vec![
                Row::new("height", "键盘高度").desc("觉得按键太小或挡住内容时调整").control(Control::Slider(SliderSpec {
                    value: m.keyboard_height,
                    min: 0.7,
                    max: 1.5,
                    step: 0.05,
                    act: A::SetKeyboardHeight,
                    format: height_label,
                    ends: ("矮", "高"),
                })),
                Row::new("edit_area", "横屏显示编辑区")
                    .desc("宽屏时在键盘右侧放撤销、复制、粘贴等常用编辑键")
                    .switch(m.edit_area, A::SetEditArea),
            ],
        ),
        group(
            "按键",
            vec![
                Row::new("key_popup", "按键气泡").desc("按下时在手指上方放大显示字母，确认没按错").switch(m.key_popup, A::SetKeyPopup),
                {
                    let mut r = Row::new("key_sound", "按键音")
                        .desc("按键时发出轻微的声音")
                        .tag("即将推出", TagKind::Neutral)
                        .control(Control::Switch { on: m.key_sound });
                    r.enabled = false;
                    r
                },
                Row::new("long_press", "长按时长").desc("长按按键输入上方小字、弹出更多字符所需的时间").segmented(lp_opts, lp_sel),
            ],
        ),
        group(
            "悬浮球",
            vec![
                Row::new("ball", "显示悬浮球").desc("收起键盘后在屏幕边缘留一个小球，点一下说话，长按展开键盘").switch(m.ball, A::SetBall),
                side,
            ],
        ),
    ]
}

fn input(m: &SettingsModel) -> Vec<Block> {
    let (cs_opts, cs_sel) = seg(
        &[
            ("小", CandidateSize::Small),
            ("标准", CandidateSize::Standard),
            ("大", CandidateSize::Large),
            ("特大", CandidateSize::ExtraLarge),
        ],
        m.candidate_size,
        A::SetCandidateSize,
    );
    let chips = FuzzyPair::ALL
        .iter()
        .map(|&p| Chip { label: p.label().to_string(), on: m.fuzzy[p.index()], act: A::SetFuzzy(p, !m.fuzzy[p.index()]) })
        .collect();
    let mut fuzzy = Row::new("fuzzy", "模糊音").desc("分不清平翘舌或前后鼻音时打开，例如输入 zi 也能出「知」").chips(chips);
    if !m.fuzzy_supported {
        fuzzy = fuzzy.tag("下个版本生效", TagKind::Warning);
    }
    let sp = Row::new("shuangpin", "双拼方案").desc("选择「小鹤双拼」布局时使用；自然码、微软双拼即将支持").control(Control::Segmented {
        options: vec![
            ("小鹤".into(), A::SetShuangpin(ShuangpinScheme::Xiaohe), true),
            ("自然码".into(), A::SetShuangpin(ShuangpinScheme::Ziranma), false),
            ("微软".into(), A::SetShuangpin(ShuangpinScheme::Microsoft), false),
        ],
        selected: Some(match m.shuangpin {
            ShuangpinScheme::Xiaohe => 0,
            ShuangpinScheme::Ziranma => 1,
            ShuangpinScheme::Microsoft => 2,
        }),
    });
    let dict_status =
        if m.dictionary.text.is_empty() { Status::new(Level::Unknown, "正在加载词库…") } else { m.dictionary.clone() };
    let words = match m.user_words {
        Some(n) => format!("记住了你打过的 {n} 个词，换电脑时可以导出带走"),
        None => "记住你打过的词，换电脑时可以导出带走".to_string(),
    };
    vec![
        group(
            "候选",
            vec![
                Row::new("candidate_size", "候选字号").desc("候选栏里文字的大小").segmented(cs_opts, cs_sel),
                Row::new("space_first", "空格上屏首选")
                    .desc("按空格输入第一个候选；关闭后空格只输入空格")
                    .switch(m.space_commits_first, A::SetSpaceCommitsFirst),
                Row::new("full_width", "中文时用全角标点")
                    .desc("中文状态下输入，。？！等中文标点")
                    .switch(m.full_width_punct, A::SetFullWidthPunct),
            ],
        ),
        group("拼音", vec![fuzzy, sp]),
        group(
            "词库",
            vec![
                Row::new("dictionary", "系统词库").desc("雾凇拼音词库，覆盖常用词、网络用语和成语").status(dict_status),
                Row::new("user_dict", "用户词库").desc(words).control(Control::Buttons(vec![
                    Button::new("导入", ButtonKind::Secondary, A::ImportUserDict),
                    Button::new("导出", ButtonKind::Secondary, A::ExportUserDict),
                ])),
                Row::new("clear_dict", "清空用户词库").desc("忘掉所有学到的词，系统词库不受影响").control(Control::Confirm {
                    id: "clear_dict",
                    label: "清空".into(),
                    question: "清空后无法恢复，建议先导出备份。".into(),
                    confirm: "确定清空".into(),
                    act: A::ClearUserDict,
                }),
            ],
        ),
    ]
}

fn voice(m: &SettingsModel) -> Vec<Block> {
    let rec = m.engines.recommended();
    let rows = VoiceEngineChoice::ALL
        .iter()
        .map(|&e| {
            let st = m.engines.get(e);
            let status = if st.detail.is_empty() {
                Status::new(Level::Unknown, "正在检测…")
            } else if st.available {
                Status::ok(st.detail.clone())
            } else {
                Status::warn(st.detail.clone())
            };
            let desc = if st.note.is_empty() { e.blurb().to_string() } else { format!("{}\n{}", e.blurb(), st.note) };
            let mut r = Row::new(format!("engine_{}", e.key()), e.name())
                .desc(desc)
                .status(status)
                .control(Control::Radio { selected: m.voice_engine == e })
                .tap(A::SetVoiceEngine(e));
            if e == rec && st.available {
                r = r.tag("推荐", TagKind::Accent);
            }
            if !st.available
                && let Some(url) = &st.download_url {
                    r = r.below(Control::Buttons(vec![Button::new("去下载", ButtonKind::Secondary, A::OpenUrl(url.clone()))]));
                }
            r
        })
        .collect();
    let path = if m.doubao_exe.is_empty() { "自动查找".to_string() } else { m.doubao_exe.clone() };
    let mut buttons = vec![Button::new("选择…", ButtonKind::Secondary, A::PickDoubaoExe)];
    if !m.doubao_exe.is_empty() {
        buttons.push(Button::new("自动查找", ButtonKind::Secondary, A::ResetDoubaoExe));
    }
    vec![
        Block::Group { title: Some("语音引擎".into()), rows, note: Some("点墨通过所选引擎的快捷键启动语音输入，识别由引擎完成，点墨不上传任何声音。".into()) },
        group(
            "更多",
            vec![
                Row::new("doubao_exe", "豆包语音的程序位置").desc(path).control(Control::Buttons(buttons)),
            ],
        ),
    ]
}

fn clipboard(m: &SettingsModel) -> Vec<Block> {
    let limits: Vec<(String, A)> = CLIP_LIMITS.iter().map(|&n| (format!("{n} 条"), A::SetClipLimit(n))).collect();
    let sel = CLIP_LIMITS.iter().position(|&n| n == m.clip_limit);
    let mut rows = vec![
        Row::new("clip_history", "记录剪贴板历史")
            .desc("复制过的文字保存在键盘的剪贴板面板里，点一下就能粘贴")
            .switch(m.clip_history, A::SetClipHistory),
        {
            let r = Row::new("clip_limit", "最多保存").desc("超出后自动删除最早的记录，固定的条目不算在内").segmented(limits, sel);
            if m.clip_history { r } else { r.disabled() }
        },
        Row::new("clip_passwords", "不记录密码框里的复制")
            .desc("从密码输入框复制的内容不进入历史")
            .switch(m.clip_skip_passwords, A::SetClipSkipPasswords),
        Row::new("clip_clear", "清空历史")
            .desc(format!("当前共 {} 条，只清除未固定的记录", m.clip_count))
            .control(Control::Confirm {
                id: "clip_clear",
                label: "清空".into(),
                question: "确定清空剪贴板历史？固定的条目会保留。".into(),
                confirm: "清空".into(),
                act: A::ClearClipboardHistory,
            }),
    ];
    if !m.clip_history {
        rows[2].enabled = false;
    }
    let pinned: Vec<Row> = if m.pinned_clips.is_empty() {
        vec![Row::new("pinned_empty", "还没有固定的条目").desc("在键盘的剪贴板面板里长按一条记录，选择「固定」")]
    } else {
        m.pinned_clips
            .iter()
            .map(|c| {
                Row::new(format!("pin_{}", c.id), crate::clip::clip_preview(&c.text, 40))
                    .icon("\u{E718}")
                    .control(Control::Buttons(vec![Button::new("取消固定", ButtonKind::Secondary, A::UnpinClip(c.id))]))
            })
            .collect()
    };
    vec![
        group("剪贴板历史", rows),
        Block::Group { title: Some("固定的条目".into()), rows: pinned, note: Some("固定的条目会一直保留，重启后也在。".into()) },
    ]
}
