//! Keyboard UI of 点墨 Dianmo. Platform-independent: draws through [`canvas::Canvas`] and is
//! driven through [`view::View`].

pub mod canvas;
pub mod view;

pub use canvas::{Align, Canvas, Color, Font, Rect, TextStyle};
pub use view::{InputState, PointerEvent, PointerPhase, Response, UiAction, View};
