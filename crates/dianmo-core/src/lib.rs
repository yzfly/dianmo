//! Platform-independent core of 点墨 Dianmo.

pub mod controller;
pub mod engine;
pub mod sink;

pub use controller::{Action, InputController};
pub use engine::{Candidate, Engine, Schema, Snapshot};
pub use sink::{EditKey, TextSink};
