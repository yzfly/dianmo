//! Dianmo conversion engine: librime 1.17.0 (`rime.dll`, loaded at runtime) + rime-ice schemas.
//!
//! Status: FFI layout (`ffi`) and the pure T9 helpers (`t9`) are done; the safe wrapper and
//! `RimeEngine` (implementing `dianmo_core::Engine`) are next — see `docs/status/dianmo-rime.md`.

pub mod ffi;
pub mod t9;
