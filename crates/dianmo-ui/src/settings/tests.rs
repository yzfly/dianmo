use super::*;
use crate::view::{PointerEvent, PointerPhase, UiAction, View};

/// The settings actions a response hands to the host.
fn settings_actions(r: &Response) -> Vec<SettingsAction> {
    r.actions
        .iter()
        .filter_map(|a| match a {
            UiAction::Settings(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

struct T {
    v: SettingsView,
    t: u64,
    acts: Vec<SettingsAction>,
}

impl T {
    fn new(w: f32, h: f32) -> Self {
        let mut v = SettingsView::new(SettingsModel::default(), ThemeKind::Light);
        v.resize(w, h);
        Self { v, t: 1000, acts: Vec::new() }
    }

    fn ev(&mut self, phase: PointerPhase, x: f32, y: f32) -> Response {
        self.t += 16;
        let r = self.v.pointer(PointerEvent { id: 1, phase, x, y, time_ms: self.t });
        self.acts.extend(settings_actions(&r));
        r
    }

    fn key(&mut self, vk: u32) -> Response {
        let r = self.v.key(vk, true);
        self.acts.extend(settings_actions(&r));
        r
    }

    fn take_actions(&mut self) -> Vec<SettingsAction> {
        std::mem::take(&mut self.acts)
    }

    fn tap_at(&mut self, x: f32, y: f32) -> Response {
        self.ev(PointerPhase::Down, x, y);
        self.ev(PointerPhase::Up, x, y)
    }

    fn tap(&mut self, name: &str) -> Response {
        assert!(self.v.scroll_into_view(name), "no element {name}");
        let (x, y) = self.v.element_center(name).unwrap();
        self.tap_at(x, y)
    }

    fn run_timers(&mut self) {
        for _ in 0..200 {
            self.t += 16;
            if self.v.timer(self.t).timer_ms.is_none() {
                break;
            }
        }
    }
}

#[test]
fn tapping_a_switch_row_toggles_and_animates() {
    let mut t = T::new(960.0, 680.0);
    assert!(t.v.model().autostart);
    let r = t.tap("autostart");
    assert!(r.repaint);
    assert_eq!(r.timer_ms, Some(crate::scroll::FRAME_MS), "switch animation runs");
    assert_eq!(t.take_actions(), vec![SettingsAction::SetAutostart(false)]);
    assert!(!t.v.model().autostart);
    t.run_timers();
    t.t += 16;
    assert_eq!(t.v.timer(t.t).timer_ms, None, "animation stops when done");
    t.tap("autostart");
    assert_eq!(t.take_actions(), vec![SettingsAction::SetAutostart(true)]);
    assert!(t.take_actions().is_empty());
}

#[test]
fn input_mode_cards_and_default_layout_on_general_page() {
    for w in [960.0, 560.0] {
        let mut t = T::new(w, 680.0);
        assert_eq!(t.v.page(), Page::General);
        t.tap("mode/语音球");
        assert_eq!(t.take_actions(), vec![SettingsAction::SetInputMode(InputMode::VoiceBall)]);
        assert_eq!(t.v.model().input_mode, InputMode::VoiceBall);
        t.tap("mode/键盘");
        assert_eq!(t.take_actions(), vec![SettingsAction::SetInputMode(InputMode::Keyboard)]);
        t.tap("layout/九宫格");
        assert_eq!(t.take_actions(), vec![SettingsAction::SetLayout(LayoutChoice::T9)]);
    }
}

#[test]
fn segmented_choice_sends_action_and_updates_theme() {
    let mut t = T::new(960.0, 680.0);
    t.tap("theme/深色");
    assert_eq!(t.take_actions(), vec![SettingsAction::SetTheme(ThemeChoice::Dark)]);
    assert_eq!(t.v.theme(), ThemeKind::Dark);
    assert_eq!(t.v.model().theme, ThemeChoice::Dark);
    // 跟随系统: the host resolves the actual theme.
    t.tap("theme/跟随系统");
    assert_eq!(t.take_actions(), vec![SettingsAction::SetTheme(ThemeChoice::System)]);
}

#[test]
fn nav_switches_pages() {
    let mut t = T::new(960.0, 680.0);
    let (x, y) = t.v.element_center("键盘").unwrap();
    assert!(x < 232.0, "left navigation in a wide window");
    t.tap_at(x, y);
    assert_eq!(t.v.page(), Page::Keyboard);
    assert!(t.take_actions().is_empty());
    t.tap("long_press/长");
    assert_eq!(t.take_actions(), vec![SettingsAction::SetLongPress(LongPress::Long)]);
    let (x, y) = t.v.element_center("关于").unwrap();
    t.tap_at(x, y);
    assert_eq!(t.v.page(), Page::About);
}

#[test]
fn narrow_window_uses_top_tabs() {
    let mut t = T::new(600.0, 800.0);
    assert!(t.v.is_narrow());
    let (_, y) = t.v.element_center("语音").unwrap();
    assert!(y < 60.0, "tabs on top");
    let (x, y) = t.v.element_center("语音").unwrap();
    t.tap_at(x, y);
    assert_eq!(t.v.page(), Page::Voice);
    // Rows still work (controls stacked under their text when needed).
    t.tap("engine_system");
    assert_eq!(t.take_actions(), vec![SettingsAction::SetVoiceEngine(VoiceEngineChoice::System)]);
}

#[test]
fn drag_scrolls_with_momentum_and_does_not_tap() {
    let mut t = T::new(960.0, 480.0);
    t.v.set_page(Page::Keyboard);
    assert!(t.v.max_scroll() > 0.0, "keyboard page is taller than 480");
    let (x, y) = t.v.element_center("edit_area").unwrap();
    t.ev(PointerPhase::Down, x, y);
    for i in 1..=6 {
        t.ev(PointerPhase::Move, x, y - i as f32 * 20.0);
    }
    let r = t.ev(PointerPhase::Up, x, y - 120.0);
    assert!(t.take_actions().is_empty(), "a drag is not a tap");
    assert!(t.v.scroll_offset() > 100.0);
    assert!(r.timer_ms.is_some(), "fling continues");
    let before = t.v.scroll_offset();
    t.run_timers();
    assert!(t.v.scroll_offset() > before);
    assert!(t.v.scroll_offset() <= t.v.max_scroll());
}

#[test]
fn wheel_and_keys_scroll() {
    let mut t = T::new(960.0, 480.0);
    t.v.set_page(Page::Input);
    assert!(t.v.wheel(600.0, 300.0, 100.0).repaint);
    assert_eq!(t.v.scroll_offset(), 100.0);
    // Outside the content (navigation) the wheel does nothing.
    assert!(!t.v.wheel(100.0, 300.0, 100.0).repaint);
    t.key(0x23); // End
    assert_eq!(t.v.scroll_offset(), t.v.max_scroll());
    t.key(0x24); // Home
    assert_eq!(t.v.scroll_offset(), 0.0);
    t.key(0x1B); // Esc
    assert_eq!(t.take_actions(), vec![SettingsAction::Close]);
}

#[test]
fn danger_action_needs_inline_confirmation() {
    let mut t = T::new(960.0, 680.0);
    t.v.set_page(Page::Clipboard);
    t.tap("清空");
    assert!(t.take_actions().is_empty(), "first tap only asks");
    // The row now shows 取消 / 清空 in place.
    assert!(t.v.element_center("取消").is_some());
    t.tap("取消");
    assert!(t.take_actions().is_empty());
    assert!(t.v.element_center("取消").is_none());
    t.tap("清空");
    t.tap("清空");
    assert_eq!(t.take_actions(), vec![SettingsAction::ClearClipboardHistory]);
    assert!(t.v.element_center("取消").is_none(), "confirmation closes after acting");
}

#[test]
fn tapping_elsewhere_closes_confirmation() {
    let mut t = T::new(960.0, 680.0);
    t.tap("恢复默认");
    assert!(t.v.element_center("恢复").is_some());
    t.tap("autostart");
    assert_eq!(t.take_actions(), vec![SettingsAction::SetAutostart(false)]);
    assert!(t.v.element_center("恢复").is_none());
    t.tap("恢复默认");
    t.key(0x1B);
    assert!(t.v.element_center("恢复").is_none(), "Esc cancels the confirmation first");
    assert!(t.take_actions().is_empty());
}

#[test]
fn slider_drag_sends_value_on_release() {
    let mut t = T::new(960.0, 680.0);
    t.v.set_page(Page::Keyboard);
    t.v.scroll_into_view("height");
    let (x, y) = t.v.element_center("height").unwrap();
    t.ev(PointerPhase::Down, x, y);
    t.ev(PointerPhase::Move, x + 30.0, y);
    t.ev(PointerPhase::Move, x + 400.0, y + 3.0);
    assert!(t.take_actions().is_empty(), "nothing sent while dragging");
    t.ev(PointerPhase::Up, x + 400.0, y + 3.0);
    assert_eq!(t.take_actions(), vec![SettingsAction::SetKeyboardHeight(1.5)]);
    assert_eq!(t.v.model().keyboard_height, 1.5);
}

#[test]
fn fuzzy_chips_toggle() {
    let mut t = T::new(960.0, 680.0);
    t.v.set_page(Page::Input);
    t.tap("fuzzy/z = zh");
    assert_eq!(t.take_actions(), vec![SettingsAction::SetFuzzy(FuzzyPair::ZZh, true)]);
    t.tap("fuzzy/z = zh");
    assert_eq!(t.take_actions(), vec![SettingsAction::SetFuzzy(FuzzyPair::ZZh, false)]);
}

#[test]
fn admin_task_fix_button_only_when_needed() {
    let mut t = T::new(960.0, 680.0);
    assert!(t.v.element_center("一键修复").is_none());
    let m = SettingsModel { admin_task: Status::warn("计划任务未注册，管理员窗口里键盘不能打字"), ..SettingsModel::default() };
    assert!(t.v.set_model(m.clone()));
    assert!(!t.v.set_model(m));
    t.tap("一键修复");
    assert_eq!(t.take_actions(), vec![SettingsAction::FixAdminTask]);
}

#[test]
fn about_page_links_and_update_states() {
    let mut t = T::new(960.0, 680.0);
    t.v.set_page(Page::About);
    t.tap("homepage");
    assert_eq!(t.take_actions(), vec![SettingsAction::OpenUrl(HOMEPAGE.into())]);
    t.tap("检查更新");
    assert_eq!(t.take_actions(), vec![SettingsAction::CheckUpdate]);
    for a in ["feedback", "diagnostics", "logs", "onboarding"] {
        t.tap(a);
    }
    assert_eq!(
        t.take_actions(),
        vec![SettingsAction::ReportIssue, SettingsAction::ExportDiagnostics, SettingsAction::OpenLogDir, SettingsAction::ShowOnboarding]
    );
    let mut m = t.v.model().clone();
    m.update = UpdateState::Checking;
    t.v.set_model(m.clone());
    assert!(t.v.element_center("正在检查…").is_none(), "disabled while checking");
    m.update = UpdateState::Available { version: "0.4.0".into(), notes: "新增设置界面\n修复若干问题".into() };
    t.v.set_model(m);
    t.tap("立即更新");
    assert_eq!(t.take_actions(), vec![SettingsAction::InstallUpdate]);
}

#[test]
fn every_hit_target_is_at_least_44_dips() {
    for w in [960.0, 600.0] {
        let mut t = T::new(w, 680.0);
        let m = SettingsModel {
            admin_task: Status::warn("未注册"),
            pinned_clips: vec![crate::view::ClipItem { id: 7, text: "固定".into(), pinned: true }],
            ..SettingsModel::default()
        };
        t.v.set_model(m);
        for p in Page::ALL {
            t.v.set_page(p);
            for h in &t.v.laid.hits {
                assert!(h.rect.h >= 44.0 && h.rect.w >= 44.0, "{p:?} {} too small: {:?}", h.name, h.rect);
            }
        }
    }
}

#[test]
fn every_page_paints() {
    struct Null(usize);
    impl crate::canvas::Canvas for Null {
        fn clear(&mut self, _: crate::canvas::Color) {}
        fn fill_rect(&mut self, _: Rect, _: f32, _: crate::canvas::Color) {
            self.0 += 1;
        }
        fn stroke_rect(&mut self, _: Rect, _: f32, _: f32, _: crate::canvas::Color) {}
        fn text(&mut self, _: &str, _: Rect, _: TextStyle) {
            self.0 += 1;
        }
        fn measure_text(&mut self, t: &str, s: TextStyle) -> f32 {
            widgets::text_w(t, s.size)
        }
        fn push_clip(&mut self, _: Rect) {}
        fn pop_clip(&mut self) {}
    }
    for kind in [ThemeKind::Light, ThemeKind::Dark] {
        let mut v = SettingsView::new(SettingsModel::default(), kind);
        v.resize(960.0, 680.0);
        for p in Page::ALL {
            v.set_page(p);
            let mut c = Null(0);
            v.paint(&mut c);
            assert!(c.0 > 20, "{p:?}");
        }
        let mut o = OnboardingView::new(SettingsModel::default(), kind);
        o.resize(760.0, 560.0);
        for i in 0..ONBOARDING_PAGES {
            o.set_page(i);
            let mut c = Null(0);
            o.paint(&mut c);
            assert!(c.0 > 10);
        }
    }
}

// ---- Onboarding

struct O {
    v: OnboardingView,
    t: u64,
    acts: Vec<SettingsAction>,
}

impl O {
    fn new() -> Self {
        let mut v = OnboardingView::new(SettingsModel::default(), ThemeKind::Light);
        v.resize(760.0, 560.0);
        Self { v, t: 1000, acts: Vec::new() }
    }

    fn ev(&mut self, phase: PointerPhase, x: f32, y: f32) -> Response {
        self.t += 16;
        let r = self.v.pointer(PointerEvent { id: 1, phase, x, y, time_ms: self.t });
        self.acts.extend(settings_actions(&r));
        r
    }

    fn take_actions(&mut self) -> Vec<SettingsAction> {
        std::mem::take(&mut self.acts)
    }

    fn tap(&mut self, name: &str) {
        let (x, y) = self.v.element_center(name).unwrap_or_else(|| panic!("no {name}"));
        self.ev(PointerPhase::Down, x, y);
        self.ev(PointerPhase::Up, x, y);
        self.settle();
    }

    fn settle(&mut self) {
        for _ in 0..100 {
            self.t += 16;
            if self.v.timer(self.t).timer_ms.is_none() {
                break;
            }
        }
    }
}

#[test]
fn onboarding_next_choose_and_finish() {
    let mut o = O::new();
    assert_eq!(o.v.page(), 0);
    o.tap("下一步");
    assert_eq!(o.v.page(), 1);
    o.tap("九宫格");
    assert_eq!(o.take_actions(), vec![SettingsAction::SetLayout(LayoutChoice::T9)]);
    o.tap("下一步");
    o.tap("系统语音");
    assert_eq!(o.take_actions(), vec![SettingsAction::SetVoiceEngine(VoiceEngineChoice::System)]);
    o.tap("上一步");
    assert_eq!(o.v.page(), 1);
    o.tap("下一步");
    o.tap("下一步");
    assert_eq!(o.v.page(), 3);
    assert!(o.v.element_center("跳过").is_none(), "no skip on the last card");
    o.tap("开始使用");
    assert_eq!(o.take_actions(), vec![SettingsAction::FinishOnboarding]);
}

#[test]
fn onboarding_swipes_between_cards() {
    let mut o = O::new();
    o.ev(PointerPhase::Down, 600.0, 300.0);
    for i in 1..=8 {
        o.ev(PointerPhase::Move, 600.0 - i as f32 * 40.0, 302.0);
    }
    let r = o.ev(PointerPhase::Up, 280.0, 302.0);
    assert_eq!(o.v.page(), 1);
    assert!(r.timer_ms.is_some(), "snaps with an animation");
    o.settle();
    // Swiping right past the first card bounces back.
    o.v.set_page(0);
    o.ev(PointerPhase::Down, 200.0, 300.0);
    o.ev(PointerPhase::Move, 400.0, 300.0);
    o.ev(PointerPhase::Up, 400.0, 300.0);
    o.settle();
    assert_eq!(o.v.page(), 0);
    assert!(o.take_actions().is_empty());
}

#[test]
fn onboarding_skip_finishes() {
    let mut o = O::new();
    o.tap("跳过");
    assert_eq!(o.take_actions(), vec![SettingsAction::FinishOnboarding]);
}

#[test]
fn actions_reach_the_host_in_the_response() {
    let mut t = T::new(960.0, 680.0);
    assert!(t.v.scroll_into_view("autostart"));
    let (x, y) = t.v.element_center("autostart").unwrap();
    t.ev(PointerPhase::Down, x, y);
    let r = t.ev(PointerPhase::Up, x, y);
    assert_eq!(r.actions, vec![UiAction::Settings(SettingsAction::SetAutostart(false))]);
    let r = t.key(0x1B);
    assert_eq!(r.actions, vec![UiAction::Settings(SettingsAction::Close)]);
}

#[test]
fn coming_soon_rows_get_a_tag() {
    let model = SettingsModel { coming_soon: vec!["candidate_size".into()], ..SettingsModel::default() };
    let blocks = pages::build(Page::Input, &model);
    let rows: Vec<&widgets::Row> = blocks
        .iter()
        .filter_map(|b| match b {
            Block::Group { rows, .. } => Some(rows.iter()),
            _ => None,
        })
        .flatten()
        .collect();
    let tag = |key: &str| rows.iter().find(|r| r.key == key).and_then(|r| r.tag.clone()).map(|t| t.0);
    assert_eq!(tag("candidate_size").as_deref(), Some("下个版本生效"));
    assert_eq!(tag("space_first"), None);
}

#[test]
fn toast_shows_and_goes_away() {
    let mut t = T::new(960.0, 680.0);
    let r = t.v.show_toast("下个版本提供", 5000);
    assert!(r.repaint);
    let ms = r.timer_ms.expect("hides itself");
    assert!((1500..=5000).contains(&ms));
    assert_eq!(t.v.toast(), Some("下个版本提供"));
    #[derive(Default)]
    struct Rec {
        texts: Vec<String>,
    }
    impl crate::canvas::Canvas for Rec {
        fn clear(&mut self, _: crate::canvas::Color) {}
        fn fill_rect(&mut self, _: Rect, _: f32, _: crate::canvas::Color) {}
        fn stroke_rect(&mut self, _: Rect, _: f32, _: f32, _: crate::canvas::Color) {}
        fn text(&mut self, t: &str, _: Rect, _: TextStyle) {
            self.texts.push(t.to_string());
        }
        fn measure_text(&mut self, t: &str, s: TextStyle) -> f32 {
            widgets::text_w(t, s.size)
        }
        fn push_clip(&mut self, _: Rect) {}
        fn pop_clip(&mut self) {}
    }
    let mut c = Rec::default();
    t.v.paint(&mut c);
    assert!(c.texts.iter().any(|s| s == "下个版本提供"));
    let r = t.v.timer(5000 + ms);
    assert!(r.repaint && r.timer_ms.is_none());
    assert_eq!(t.v.toast(), None);
}

#[test]
fn voice_page_lists_four_engines_without_fallback() {
    let blocks = pages::build(Page::Voice, &SettingsModel::default());
    let keys: Vec<String> = blocks
        .iter()
        .filter_map(|b| match b {
            Block::Group { rows, .. } => Some(rows.iter().map(|r| r.key.clone())),
            _ => None,
        })
        .flatten()
        .collect();
    for e in VoiceEngineChoice::ALL {
        assert!(keys.contains(&format!("engine_{}", e.key())), "{e:?}");
    }
    assert!(!keys.iter().any(|k| k == "voice_fallback"), "no Win+H fallback (TODO #37)");
}

#[test]
fn key_sound_rows_follow_the_switch() {
    let mut t = T::new(960.0, 680.0);
    t.v.set_page(Page::Keyboard);
    assert!(!t.v.scroll_into_view("key_sound_volume/大"), "volume is disabled while the sound is off");
    t.tap("key_sound");
    assert_eq!(t.take_actions(), vec![SettingsAction::SetKeySound(true)]);
    t.run_timers();
    t.tap("key_sound_volume/大");
    assert_eq!(t.take_actions(), vec![SettingsAction::SetKeySoundVolume(KeySoundVolume::High)]);
    t.tap("key_sound_style/柔和");
    assert_eq!(t.take_actions(), vec![SettingsAction::SetKeySoundStyle(KeySoundStyle::Soft)]);
    let m = t.v.model();
    assert!(m.key_sound && m.key_sound_volume == KeySoundVolume::High && m.key_sound_style == KeySoundStyle::Soft);
}

#[test]
fn input_page_offers_four_shuangpin_schemes_and_no_coming_soon_tags() {
    let mut t = T::new(960.0, 680.0);
    t.v.set_page(Page::Input);
    for s in ShuangpinScheme::ALL {
        t.tap(&format!("shuangpin/{}", s.short()));
        assert_eq!(t.take_actions(), vec![SettingsAction::SetShuangpin(s)], "{s:?}");
    }
    assert_eq!(t.v.model().shuangpin, ShuangpinScheme::Sogou);
    let model = SettingsModel { fuzzy_supported: true, ..SettingsModel::default() };
    let blocks = pages::build(Page::Input, &model);
    let tagged: Vec<String> = blocks
        .iter()
        .filter_map(|b| match b {
            Block::Group { rows, .. } => Some(rows.iter().filter(|r| r.tag.is_some()).map(|r| r.key.clone())),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(tagged.is_empty(), "{tagged:?}");
}

#[test]
fn fuzzy_and_user_dict_show_progress() {
    let mut t = T::new(960.0, 680.0);
    t.v.set_page(Page::Input);
    let busy = SettingsModel {
        fuzzy_supported: true,
        fuzzy_status: Status::new(Level::Unknown, "正在应用模糊音…"),
        user_dict_status: Status::new(Level::Unknown, "正在导入…"),
        ..SettingsModel::default()
    };
    t.v.set_model(busy);
    assert!(!t.v.scroll_into_view("导入") && !t.v.scroll_into_view("导出"), "buttons wait for the running job");
    let blocks = pages::build(Page::Input, t.v.model());
    let status = |key: &str| {
        blocks.iter().find_map(|b| match b {
            Block::Group { rows, .. } => rows.iter().find(|r| r.key == key).and_then(|r| r.status.clone()),
            _ => None,
        })
    };
    assert_eq!(status("fuzzy").map(|s| s.text).as_deref(), Some("正在应用模糊音…"));
    assert_eq!(status("user_dict").map(|s| s.text).as_deref(), Some("正在导入…"));
    // Done: the buttons work again.
    t.v.set_model(SettingsModel { user_dict_status: Status::ok("已导入 12 个词"), ..t.v.model().clone() });
    t.tap("导出");
    assert_eq!(t.take_actions(), vec![SettingsAction::ExportUserDict]);
}
