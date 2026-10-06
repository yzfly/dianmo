//! 关于 page (PRODUCT.md P5): brand hero, update status, developer, links, credits.

use crate::canvas::{Canvas, Rect};
use crate::theme::SettingsTheme;

use super::model::{SettingsAction as A, SettingsModel, UpdateState, HOMEPAGE, LICENSE_URL, RELEASES_URL};
use super::widgets::{bold, centered, Block, Button, ButtonKind, Control, Hero, Row, TagKind};

pub const NAME: &str = "点墨 Dianmo";
pub const SLOGAN: &str = "指尖一点，落字成墨";
pub const DEVELOPER: &str = "云中江树";

/// Third-party components credited on the about page: (name, what it does, url).
pub const CREDITS: [(&str, &str, &str); 5] = [
    ("librime", "中州韵输入法引擎，负责拼音到汉字的转换", "https://github.com/rime/librime"),
    ("雾凇拼音 rime-ice", "精心维护的简体中文词库和输入方案", "https://github.com/iDvel/rime-ice"),
    ("OpenCC", "开放中文转换，简繁和异体字处理", "https://github.com/BYVoid/OpenCC"),
    ("windows-rs", "微软官方的 Rust Windows API 绑定", "https://github.com/microsoft/windows-rs"),
    ("Rust", "让点墨小巧、快速、省电的编程语言", "https://www.rust-lang.org"),
];

pub(crate) fn build(m: &SettingsModel) -> Vec<Block> {
    let version = if m.version.is_empty() { "开发版".to_string() } else { format!("版本 {}", m.version) };
    let subtitle = if m.build_date.is_empty() { version } else { format!("{version} · {} 构建", m.build_date) };
    let hero = Block::Hero(Hero { title: NAME.into(), subtitle, slogan: SLOGAN.into() });

    let update = update_row(&m.update);
    let auto = Row::new("auto_update", "自动检查更新")
        .desc("启动后检查一次，之后每天最多一次；有新版时托盘图标会显示小红点")
        .switch(m.auto_update, A::SetAutoUpdate);

    let info = vec![
        Row::new("developer", "开发者").icon("\u{E77B}").control(Control::Value(DEVELOPER.into())),
        Row::new("homepage", "项目主页与源码").icon("\u{E774}").link("github.com/yzfly/dianmo", A::OpenUrl(HOMEPAGE.into()), true),
        Row::new("license", "开源许可证").icon("\u{E8A5}").link("GPL-3.0", A::OpenUrl(LICENSE_URL.into()), true),
        Row::new("releases", "更新日志").icon("\u{E81C}").link("GitHub Releases", A::OpenUrl(RELEASES_URL.into()), true),
    ];
    let help = vec![
        Row::new("feedback", "反馈问题")
            .icon("\u{ED15}")
            .desc("打开 GitHub 新建问题页，版本、系统和屏幕信息已自动填好")
            .link("", A::ReportIssue, true),
        Row::new("diagnostics", "导出诊断包")
            .icon("\u{E9D9}")
            .desc("把日志和设置打包成 zip，不含剪贴板和词库内容")
            .link("", A::ExportDiagnostics, false),
        Row::new("logs", "打开日志目录").icon("\u{E838}").link("", A::OpenLogDir, false),
        Row::new("onboarding", "重看新手引导").icon("\u{E7BE}").link("", A::ShowOnboarding, false),
    ];
    let credits = CREDITS
        .iter()
        .map(|(name, what, url)| Row::new(format!("credit_{name}"), *name).desc(*what).link("", A::OpenUrl(url.to_string()), true))
        .collect();

    vec![
        hero,
        Block::Group { title: Some("更新".into()), rows: vec![update, auto], note: None },
        Block::Group { title: Some("信息".into()), rows: info, note: None },
        Block::Group { title: Some("帮助与反馈".into()), rows: help, note: None },
        Block::Group { title: Some("致谢".into()), rows: credits, note: Some("点墨站在这些开源项目的肩膀上，感谢它们的作者和社区。".into()) },
        Block::Footer(vec![format!("开发者 {DEVELOPER} · 以 GPL-3.0 许可证开源"), "Copyright © 2026 云中江树".into()]),
    ]
}

fn update_row(u: &UpdateState) -> Row {
    use super::model::Status;
    let check = || Button::new("检查更新", ButtonKind::Secondary, A::CheckUpdate);
    let r = Row::new("update", "检查更新");
    match u {
        UpdateState::Unknown => r.desc("从 GitHub 获取最新版本").control(Control::Buttons(vec![check()])),
        UpdateState::Checking => r
            .desc("正在连接 GitHub…")
            .control(Control::Buttons(vec![Button::new("正在检查…", ButtonKind::Secondary, A::CheckUpdate).disabled()])),
        UpdateState::UpToDate => r.status(Status::ok("已是最新版本")).control(Control::Buttons(vec![check()])),
        UpdateState::Available { version, notes } => {
            let mut r = Row::new("update", format!("发现新版本 {version}"))
                .tag("NEW", TagKind::Accent)
                .control(Control::Buttons(vec![Button::new("立即更新", ButtonKind::Primary, A::InstallUpdate)]));
            if !notes.trim().is_empty() {
                r = r.desc(notes.trim().to_string());
            }
            r
        }
        UpdateState::Downloading(f) => Row::new("update", "正在下载更新")
            .desc(format!("已完成 {}%，下载完成后自动重启点墨", (f.clamp(0.0, 1.0) * 100.0).round() as i32))
            .below(Control::Progress(*f)),
        UpdateState::Failed(e) => r
            .status(Status::error(if e.is_empty() { "检查失败".to_string() } else { format!("检查失败：{e}") }))
            .control(Control::Buttons(vec![Button::new("重试", ButtonKind::Secondary, A::CheckUpdate)])),
    }
}

/// The app icon (a built-in PNG drawn by the host canvas).
pub(crate) fn app_icon(c: &mut dyn Canvas, r: Rect) {
    c.image("app-icon", r);
}

pub(crate) fn paint_hero(c: &mut dyn Canvas, t: &SettingsTheme, h: &Hero, r: Rect) {
    let s = 96.0;
    let icon = Rect::new(r.x + (r.w - s) / 2.0, r.y + 20.0, s, s);
    app_icon(c, icon);
    let y = icon.y + s + 14.0;
    c.text(&h.title, Rect::new(r.x, y, r.w, 32.0), bold(22.0, t.text).with_center());
    c.text(&h.subtitle, Rect::new(r.x, y + 34.0, r.w, 20.0), centered(13.0, t.text_faint));
    c.text(&h.slogan, Rect::new(r.x, y + 62.0, r.w, 24.0), centered(15.0, t.text_secondary));
}

trait CenterExt {
    fn with_center(self) -> Self;
}

impl CenterExt for crate::canvas::TextStyle {
    fn with_center(self) -> Self {
        Self { align: crate::canvas::Align::Center, ..self }
    }
}
