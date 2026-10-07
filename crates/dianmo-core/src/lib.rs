//! Platform-independent core of 点墨 Dianmo.

pub mod controller;
pub mod engine;
pub mod sink;

pub use controller::{Action, InputController, full_width};
pub use engine::{Candidate, Engine, Schema, Snapshot};
pub use sink::{EditKey, KeyChord, KeyCode, TextSink};
