//! Colour themes. Every colour the keyboard paints comes from a [`Theme`].

use crate::canvas::Color;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeKind {
    #[default]
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    /// Keyboard and candidate-bar background.
    pub background: Color,
    /// Character keys.
    pub key: Color,
    pub key_pressed: Color,
    /// Function keys (shift, backspace, 123, ...).
    pub func: Color,
    pub func_pressed: Color,
    /// Thin "bottom edge" drawn under every key.
    pub shadow: Color,
    pub accent: Color,
    pub accent_pressed: Color,
    /// Text on accent surfaces.
    pub on_accent: Color,
    /// Tinted surface for latched states (caps lock, active tab).
    pub accent_soft: Color,
    pub text: Color,
    /// Secondary text: hints, preedit, toolbar icons.
    pub text_secondary: Color,
    /// Faint text: corner symbols, the schema name on the space bar.
    pub text_faint: Color,
    /// Pop-up bubble and alternates popup.
    pub bubble: Color,
    pub bubble_shadow: Color,
    /// Pressed state of flat items (toolbar icons, candidates).
    pub flat_pressed: Color,
    pub divider: Color,
}

impl Theme {
    pub fn of(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Light => Self::light(),
            ThemeKind::Dark => Self::dark(),
        }
    }

    pub fn light() -> Self {
        let accent = Color::rgb(46, 104, 236);
        Self {
            background: Color::rgb(231, 233, 238),
            key: Color::rgb(255, 255, 255),
            key_pressed: Color::rgb(214, 218, 226),
            func: Color::rgb(205, 210, 219),
            func_pressed: Color::rgb(181, 187, 198),
            shadow: Color::rgb(140, 147, 160).with_alpha(0.55),
            accent,
            accent_pressed: Color::rgb(33, 84, 200),
            on_accent: Color::rgb(255, 255, 255),
            accent_soft: Color::rgb(214, 226, 252),
            text: Color::rgb(28, 30, 35),
            text_secondary: Color::rgb(96, 103, 115),
            text_faint: Color::rgb(140, 146, 158),
            bubble: Color::rgb(255, 255, 255),
            bubble_shadow: Color::rgb(60, 66, 80).with_alpha(0.22),
            flat_pressed: Color::rgb(28, 30, 35).with_alpha(0.08),
            divider: Color::rgb(28, 30, 35).with_alpha(0.10),
        }
    }

    pub fn dark() -> Self {
        let accent = Color::rgb(82, 139, 255);
        Self {
            background: Color::rgb(21, 22, 25),
            key: Color::rgb(66, 68, 75),
            key_pressed: Color::rgb(92, 95, 104),
            func: Color::rgb(44, 46, 52),
            func_pressed: Color::rgb(68, 71, 79),
            shadow: Color::rgb(0, 0, 0).with_alpha(0.55),
            accent,
            accent_pressed: Color::rgb(62, 114, 224),
            on_accent: Color::rgb(255, 255, 255),
            accent_soft: Color::rgb(44, 62, 104),
            text: Color::rgb(236, 237, 241),
            text_secondary: Color::rgb(160, 165, 176),
            text_faint: Color::rgb(118, 123, 134),
            bubble: Color::rgb(88, 91, 99),
            bubble_shadow: Color::rgb(0, 0, 0).with_alpha(0.45),
            flat_pressed: Color::rgb(255, 255, 255).with_alpha(0.10),
            divider: Color::rgb(255, 255, 255).with_alpha(0.10),
        }
    }
}

/// Colours of the settings window, the about page and onboarding (`crate::settings`).
///
/// Neutral greys and semantic colours follow ByteDance's Arco Design tokens (the design language
/// of 飞书 / 豆包 desktop); the brand blue is the one used by the keyboard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SettingsTheme {
    pub kind: ThemeKind,
    /// Content area background (behind the cards).
    pub background: Color,
    /// Left navigation / top tabs background.
    pub sidebar: Color,
    /// Group cards.
    pub card: Color,
    pub card_border: Color,
    pub divider: Color,
    /// Titles and body text.
    pub text: Color,
    /// Descriptions, group titles.
    pub text_secondary: Color,
    /// Captions, disabled text, chevrons.
    pub text_faint: Color,
    pub accent: Color,
    pub accent_pressed: Color,
    /// Selected nav item, info badges.
    pub accent_soft: Color,
    pub on_accent: Color,
    /// Pressed / hovered overlay on flat items.
    pub pressed: Color,
    pub hover: Color,
    /// Segmented control track, secondary buttons, switch track when off.
    pub fill: Color,
    pub fill_strong: Color,
    pub switch_off: Color,
    /// Thumb of switches and sliders.
    pub knob: Color,
    pub shadow: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub danger_soft: Color,
    pub warning_soft: Color,
}

impl SettingsTheme {
    pub fn of(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Light => Self::light(),
            ThemeKind::Dark => Self::dark(),
        }
    }

    pub fn light() -> Self {
        Self {
            kind: ThemeKind::Light,
            background: Color::rgb(245, 246, 248),
            sidebar: Color::rgb(238, 240, 243),
            card: Color::rgb(255, 255, 255),
            card_border: Color::rgb(229, 230, 235).with_alpha(0.7),
            divider: Color::rgb(229, 230, 235),
            text: Color::rgb(29, 33, 41),
            text_secondary: Color::rgb(78, 89, 105),
            text_faint: Color::rgb(134, 144, 156),
            accent: Color::rgb(59, 108, 246),
            accent_pressed: Color::rgb(40, 86, 214),
            accent_soft: Color::rgb(232, 239, 254),
            on_accent: Color::rgb(255, 255, 255),
            pressed: Color::rgb(29, 33, 41).with_alpha(0.07),
            hover: Color::rgb(29, 33, 41).with_alpha(0.04),
            fill: Color::rgb(242, 243, 245),
            fill_strong: Color::rgb(229, 230, 235),
            switch_off: Color::rgb(201, 205, 212),
            knob: Color::rgb(255, 255, 255),
            shadow: Color::rgb(29, 33, 41).with_alpha(0.10),
            success: Color::rgb(0, 180, 42),
            warning: Color::rgb(255, 125, 0),
            danger: Color::rgb(245, 63, 63),
            danger_soft: Color::rgb(255, 236, 232),
            warning_soft: Color::rgb(255, 247, 232),
        }
    }

    pub fn dark() -> Self {
        Self {
            kind: ThemeKind::Dark,
            background: Color::rgb(23, 23, 26),
            sidebar: Color::rgb(30, 30, 33),
            card: Color::rgb(36, 36, 40),
            card_border: Color::rgb(255, 255, 255).with_alpha(0.06),
            divider: Color::rgb(255, 255, 255).with_alpha(0.08),
            text: Color::rgb(255, 255, 255).with_alpha(0.90),
            text_secondary: Color::rgb(255, 255, 255).with_alpha(0.65),
            text_faint: Color::rgb(255, 255, 255).with_alpha(0.42),
            accent: Color::rgb(82, 132, 255),
            accent_pressed: Color::rgb(64, 112, 230),
            accent_soft: Color::rgb(82, 132, 255).with_alpha(0.18),
            on_accent: Color::rgb(255, 255, 255),
            pressed: Color::rgb(255, 255, 255).with_alpha(0.10),
            hover: Color::rgb(255, 255, 255).with_alpha(0.05),
            fill: Color::rgb(255, 255, 255).with_alpha(0.08),
            fill_strong: Color::rgb(255, 255, 255).with_alpha(0.14),
            switch_off: Color::rgb(255, 255, 255).with_alpha(0.22),
            knob: Color::rgb(255, 255, 255),
            shadow: Color::rgb(0, 0, 0).with_alpha(0.35),
            success: Color::rgb(39, 195, 70),
            warning: Color::rgb(255, 150, 38),
            danger: Color::rgb(247, 105, 101),
            danger_soft: Color::rgb(247, 105, 101).with_alpha(0.16),
            warning_soft: Color::rgb(255, 150, 38).with_alpha(0.14),
        }
    }
}
