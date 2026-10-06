//! Dianmo conversion engine: librime 1.17.0 (`rime.dll`, loaded at runtime) + rime-ice schemas.
//!
//! - [`RimeEngine`] implements `dianmo_core::Engine`; [`Options::for_app`] gives the installed layout.
//! - [`deploy`] precompiles schemas/dictionaries at packaging time.
//! - `ffi`: `#[repr(C)]` mirror of `rime_api.h`; `t9`, `comment`: pure helpers.

pub mod comment;
pub mod ffi;
pub mod t9;

#[cfg(windows)]
mod api;
#[cfg(windows)]
mod engine;
#[cfg(windows)]
pub use engine::{DeployReport, Error, Options, RimeEngine, deploy, schema_id, shutdown};
