//! Windows platform layer of 点墨 Dianmo (see `docs/DESIGN.md` §3).
//!
//! - [`run`]: the docked, never-activating keyboard window. It owns a [`dianmo_ui::View`], feeds it
//!   `WM_POINTER` multi-touch and timer ticks, paints it with Direct2D/DirectWrite through
//!   DirectComposition, and hands the view's [`dianmo_ui::UiAction`]s to an [`App`].
//! - [`SendInputSink`]: a [`dianmo_core::TextSink`] that types into the focused app with `SendInput`.
//! - [`start_voice_typing`]: Windows voice typing (Win+H).
//! - AppBar (screen space reservation), tray icon and edge handle are managed by the host.
//! - [`focus`]: UI Automation focus watcher for auto show/hide ([`start_focus_watcher`]).
//! - [`tabtip`]: read/set the system touch keyboard's auto-invoke settings.
//! - App windows (settings, about, onboarding): [`HostControl::open_window`] shows a
//!   [`dianmo_ui::View`] in an ordinary activatable window on the same thread and devices.
//!
//! On non-Windows targets the crate only has its platform-independent helpers (e.g. the floating
//! ball's geometry, unit-tested there) so the workspace still builds and tests on Linux.

mod ball_geom;
#[cfg(windows)]
mod appbar;
#[cfg(windows)]
mod canvas;
#[cfg(windows)]
mod clock;
#[cfg(windows)]
pub mod focus;
#[cfg(windows)]
mod handle;
#[cfg(windows)]
mod host;
#[cfg(windows)]
mod sink;
#[cfg(windows)]
pub mod tabtip;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod window;

#[cfg(windows)]
pub use clock::now_ms;
#[cfg(windows)]
pub use handle::{BallEdge, BallEvent, BallPos, BallState};
#[cfg(windows)]
pub use focus::{FieldKind, FocusEvent, FocusWatcher, start_focus_watcher};
#[cfg(windows)]
pub use host::{App, HostControl, HostOptions, HostProxy, enable_per_monitor_dpi, run, run_with};
#[cfg(windows)]
pub use tray::TrayItem;
#[cfg(windows)]
pub use window::{WindowId, WindowOptions, system_dark_mode};
#[cfg(windows)]
pub use sink::{SendInputSink, send_chord, send_edit_key, send_key_event, send_text, start_voice_typing};
