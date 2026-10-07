//! Dianmo conversion engine: librime 1.17.0 (`rime.dll`, loaded at runtime) + rime-ice schemas.
//!
//! - [`RimeEngine`] implements `dianmo_core::Engine`; [`Options::for_app`] gives the installed layout.
//! - [`deploy`] precompiles schemas/dictionaries at packaging time.
//! - Fuzzy pinyin: [`set_fuzzy`] writes the user customization, [`deploy_user`] (own process:
//!   `dianmo.exe --deploy-user`) builds it, [`RimeEngine::reload`] switches to it.
//! - Double pinyin schemes: [`ShuangpinScheme`], [`Options::shuangpin`], [`RimeEngine::set_shuangpin`].
//! - User dictionary: `RimeEngine::{export,import,clear}_user_dict`, `user_word_count`
//!   (and free functions of the same names for a process without an engine).
//! - `ffi`: `#[repr(C)]` mirror of `rime_api.h` / `rime_levers_api.h`; `t9`, `comment`, `custom`:
//!   pure helpers.

pub mod comment;
pub mod custom;
pub mod ffi;
pub mod t9;

pub use custom::{FUZZY_LABELS, Fuzzy, ShuangpinScheme};

#[cfg(windows)]
mod api;
#[cfg(windows)]
mod engine;
#[cfg(windows)]
mod user;
#[cfg(windows)]
pub use engine::{DeployReport, Error, Options, RimeEngine, deploy, rime_schema, schema_id, shutdown};
#[cfg(windows)]
pub use user::{
    FuzzyChange, clear_user_dict, deploy_user, export_user_dict, fuzzy, import_user_dict, needs_user_deploy, set_fuzzy,
    user_word_count,
};
