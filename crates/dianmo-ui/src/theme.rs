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
