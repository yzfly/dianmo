//! Windows platform layer of 点墨 Dianmo (see `docs/DESIGN.md` §3).
//!
//! - [`run`]: the docked, never-activating keyboard window. It owns a [`dianmo_ui::View`], feeds it
//!   `WM_POINTER` multi-touch and timer ticks, paints it with Direct2D/DirectWrite through
//!   DirectComposition, and hands the view's [`dianmo_ui::UiAction`]s to an [`App`].
//! - [`SendInputSink`]: a [`dianmo_core::TextSink`] that types into the focused app with `SendInput`.
//! - [`start_voice_typing`]: Windows voice typing (Win+H).
//! - AppBar (screen space reservation), tray icon and edge handle are managed by the host.
//! - [`tabtip`]: read/set the system touch keyboard's auto-invoke settings.
//!
//! The crate is empty on non-Windows targets so the workspace still builds and tests on Linux.
#![cfg(windows)]

mod appbar;
mod canvas;
mod clock;
mod handle;
mod host;
mod sink;
pub mod tabtip;
mod tray;

pub use clock::now_ms;
pub use host::{App, HostControl, HostOptions, HostProxy, enable_per_monitor_dpi, run, run_with};
pub use sink::{SendInputSink, send_edit_key, send_text, start_voice_typing};
