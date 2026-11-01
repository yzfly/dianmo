//! When a tap on a text field should pop up the keyboard (设置 › 常规 ›「点几下弹出」).
//!
//! Platform-independent so it is unit-tested on Linux: the Windows focus watcher
//! (`dianmo-win/src/focus.rs`) identifies each field (`FocusEvent::Editable::field`) and each
//! physical touch contact ([`Tap`]); this module only counts.
//!
//! 单击 opens on the first tap. 双击 opens on the second tap on the *same* field, the two finger
//! downs no further apart than the double-click time (Windows' setting, default 500 ms). One
//! contact can reach the app as several events — the focus change it caused, a re-tap of the
//! already focused field seen by the mouse hook, the same seen by raw input — and they all carry
//! the same [`Tap::seq`], so they count once. A tap on another field, outside any field, a slow
//! second tap, hiding / showing the keyboard and changing the mode all start over.
//!
//! Based on the community version by Enderlessss (issue #1,
//! <https://github.com/Enderlessss/dianmo-branch-doubleclickfeaturetest>): tap sequence and field
//! id, reset points. Here the gap is measured between finger downs at the source (not when the
//! events reach the UI thread, which differs between a UIA focus event and a re-tap) and follows
//! the system double-click time.

use crate::settings::AutoShowClick;

/// One physical touch contact, as seen by the focus watcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tap {
    /// Incremented once per contact (never 0 for a real one).
    pub seq: u32,
    /// When the finger went down, in the watcher's millisecond clock (wraps, compare with
    /// `wrapping_sub`).
    pub down_ms: u32,
}

/// Two downs closer than this are one contact counted twice (e.g. raw input and the mouse press
/// Windows synthesizes from it); nobody double-taps that fast.
pub const MIN_GAP_MS: u32 = 60;

/// The double-tap window from the system double-click time (`GetDoubleClickTime`), kept within a
/// range that still makes sense on a touch screen.
pub fn double_tap_window(system_ms: u32) -> u32 {
    system_ms.clamp(200, 1000)
}

/// Counts taps for 双击. Feed every touch tap on an editable field ([`AutoShowGate::tap`]) and
/// call [`AutoShowGate::reset`] whenever the count must start over.
#[derive(Clone, Debug, Default)]
pub struct AutoShowGate {
    /// Field and contact of the pending first tap.
    first: Option<(u64, Tap)>,
}

impl AutoShowGate {
    /// A touch tap on editable field `field` (`tap`: its contact, `None` when the watcher has no
    /// contact for it). True when this tap should open the keyboard. `window_ms`: see
    /// [`double_tap_window`].
    pub fn tap(&mut self, mode: AutoShowClick, field: u64, tap: Option<Tap>, window_ms: u32) -> bool {
        if mode == AutoShowClick::Single {
            self.first = None;
            return true;
        }
        let Some(tap) = tap else {
            // Can't tell taps apart: never guess a double tap.
            self.first = None;
            return false;
        };
        if let Some((f, first)) = self.first {
            if tap.seq == first.seq {
                // The same contact again. Its focus event names the field it moved to, which may
                // come after a re-tap of the previously focused one: keep the latest field.
                self.first = Some((field, first));
                return false;
            }
            let gap = tap.down_ms.wrapping_sub(first.down_ms);
            if f == field {
                if gap < MIN_GAP_MS {
                    // One contact seen twice with different sequence numbers: still the first tap.
                    return false;
                }
                if gap <= window_ms {
                    self.first = None;
                    return true;
                }
            }
        }
        self.first = Some((field, tap));
        false
    }

    /// Start over (a tap outside any field, keyboard shown or hidden, mode changed, …).
    pub fn reset(&mut self) {
        self.first = None;
    }

    /// Whether a first tap is waiting for its second.
    pub fn pending(&self) -> bool {
        self.first.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AutoShowClick::{Double, Single};

    const W: u32 = 500;

    fn t(seq: u32, down_ms: u32) -> Option<Tap> {
        Some(Tap { seq, down_ms })
    }

    #[test]
    fn single_opens_on_every_tap() {
        let mut g = AutoShowGate::default();
        assert!(g.tap(Single, 1, t(1, 0), W));
        assert!(g.tap(Single, 2, t(2, 100), W));
        assert!(g.tap(Single, 2, None, W), "single tap needs no contact id");
        assert!(!g.pending());
    }

    #[test]
    fn double_needs_two_taps_on_the_same_field() {
        let mut g = AutoShowGate::default();
        assert!(!g.tap(Double, 7, t(1, 1000), W));
        assert!(g.pending());
        assert!(g.tap(Double, 7, t(2, 1200), W));
        assert!(!g.pending(), "a double tap is consumed");
        // A third quick tap is a new first tap, not a second double tap.
        assert!(!g.tap(Double, 7, t(3, 1350), W));
    }

    #[test]
    fn several_events_of_one_contact_count_once() {
        let mut g = AutoShowGate::default();
        // Focus event + hook re-tap + raw re-tap of the same contact.
        assert!(!g.tap(Double, 7, t(1, 1000), W));
        assert!(!g.tap(Double, 7, t(1, 1000), W));
        assert!(!g.tap(Double, 7, t(1, 1000), W));
        // Second contact, also reported twice: opens once.
        assert!(g.tap(Double, 7, t(2, 1180), W));
        assert!(!g.tap(Double, 7, t(2, 1180), W));
    }

    #[test]
    fn one_contact_with_two_sequence_numbers_is_not_a_double_tap() {
        let mut g = AutoShowGate::default();
        assert!(!g.tap(Double, 7, t(1, 1000), W));
        assert!(!g.tap(Double, 7, t(2, 1000 + MIN_GAP_MS - 1), W));
        // The real second tap is measured from the first contact.
        assert!(g.tap(Double, 7, t(3, 1200), W));
    }

    #[test]
    fn focus_event_after_retap_of_old_field_names_the_new_field() {
        let mut g = AutoShowGate::default();
        // Contact 1 lands on field 8, but the re-tap of the old field 7 arrives first.
        assert!(!g.tap(Double, 7, t(1, 1000), W));
        assert!(!g.tap(Double, 8, t(1, 1000), W));
        assert!(g.tap(Double, 8, t(2, 1200), W));
    }

    #[test]
    fn another_field_starts_over() {
        let mut g = AutoShowGate::default();
        assert!(!g.tap(Double, 7, t(1, 1000), W));
        assert!(!g.tap(Double, 8, t(2, 1150), W), "first tap on field 8");
        assert!(g.tap(Double, 8, t(3, 1300), W));
        assert!(!g.tap(Double, 7, t(4, 1400), W), "back on 7 is a first tap");
    }

    #[test]
    fn slow_second_tap_is_a_new_first_tap() {
        let mut g = AutoShowGate::default();
        assert!(!g.tap(Double, 7, t(1, 1000), W));
        assert!(!g.tap(Double, 7, t(2, 1000 + W + 1), W), "too slow");
        assert!(g.tap(Double, 7, t(3, 1000 + W + 1 + W), W), "exactly the window still counts");
    }

    #[test]
    fn reset_and_missing_contact_start_over() {
        let mut g = AutoShowGate::default();
        assert!(!g.tap(Double, 7, t(1, 1000), W));
        g.reset();
        assert!(!g.tap(Double, 7, t(2, 1200), W));
        assert!(!g.tap(Double, 7, None, W));
        assert!(!g.pending());
        assert!(!g.tap(Double, 7, t(3, 1300), W));
        // Switching to 单击 clears a pending first tap too.
        assert!(g.tap(Single, 7, t(4, 1350), W));
        assert!(!g.tap(Double, 7, t(5, 1450), W));
    }

    #[test]
    fn millisecond_clock_wraps() {
        let mut g = AutoShowGate::default();
        assert!(!g.tap(Double, 7, t(1, u32::MAX - 100), W));
        assert!(g.tap(Double, 7, t(2, 100), W));
    }

    #[test]
    fn window_follows_system_double_click_time() {
        assert_eq!(double_tap_window(500), 500);
        assert_eq!(double_tap_window(900), 900);
        assert_eq!(double_tap_window(0), 200);
        assert_eq!(double_tap_window(5000), 1000);
    }
}
