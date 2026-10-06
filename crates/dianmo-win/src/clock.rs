use std::sync::OnceLock;
use std::time::Instant;

/// Monotonic milliseconds since the first call (the clock used for `PointerEvent::time_ms` and
/// `View::timer`).
pub fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}
