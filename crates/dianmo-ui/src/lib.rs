//! Keyboard UI of 点墨 Dianmo. Platform-independent: draws through [`canvas::Canvas`] and is
//! driven through [`view::View`]. [`KeyboardView`] is the phone-style keyboard.

pub mod canvas;
pub mod view;

mod clip;
mod draw;
mod keyboard;
mod pc;
pub mod layout;
mod scroll;
pub mod settings;
pub mod theme;

pub use canvas::{Align, Canvas, Color, Font, Rect, TextStyle};
pub use clip::clip_preview;
pub use keyboard::{KeyboardConfig, KeyboardView, MORE_BATCH};
pub use layout::{Layout, SymTab};
pub use settings::{OnboardingView, SettingsAction, SettingsModel, SettingsView};
pub use theme::{SettingsTheme, Theme, ThemeKind};
pub use view::{ClipItem, InputState, PointerEvent, PointerPhase, Response, UiAction, View};
