//! `#[repr(C)]` mirrors of librime 1.17.0 `rime_api.h` (only what Dianmo needs is typed;
//! every `RimeApi` slot is present so the layout matches).
//!
//! Conventions from the header:
//! - `Bool` is `int`.
//! - Structs with a leading `data_size` must be initialised with
//!   `data_size = sizeof(T) - sizeof(data_size)` (`RIME_STRUCT_INIT`); [`rime_struct`] does that.
//! - Strings are UTF-8; anything librime returns in a struct is freed with its own `free_*`.

#![allow(dead_code)]

use std::ffi::{c_char, c_int, c_void};

pub type Bool = c_int;
pub type RimeSessionId = usize;

/// An API slot Dianmo doesn't call. All function pointers have the same size, so the
/// table layout is unaffected.
pub type Unused = Option<unsafe extern "C" fn()>;

#[repr(C)]
pub struct RimeTraits {
    pub data_size: c_int,
    pub shared_data_dir: *const c_char,
    pub user_data_dir: *const c_char,
    pub distribution_name: *const c_char,
    pub distribution_code_name: *const c_char,
    pub distribution_version: *const c_char,
    /// `"rime.<app>"`; the prefix lets librime clean up old log files.
    pub app_name: *const c_char,
    pub modules: *const *const c_char,
    /// 0 INFO, 1 WARNING, 2 ERROR, 3 FATAL.
    pub min_log_level: c_int,
    /// NULL: temp dir; "": stderr only.
    pub log_dir: *const c_char,
    pub prebuilt_data_dir: *const c_char,
    pub staging_dir: *const c_char,
}

#[repr(C)]
pub struct RimeComposition {
    pub length: c_int,
    pub cursor_pos: c_int,
    pub sel_start: c_int,
    pub sel_end: c_int,
    pub preedit: *mut c_char,
}

#[repr(C)]
pub struct RimeCandidate {
    pub text: *mut c_char,
    pub comment: *mut c_char,
    pub reserved: *mut c_void,
}

#[repr(C)]
pub struct RimeMenu {
    pub page_size: c_int,
    pub page_no: c_int,
    pub is_last_page: Bool,
    pub highlighted_candidate_index: c_int,
    pub num_candidates: c_int,
    pub candidates: *mut RimeCandidate,
    pub select_keys: *mut c_char,
}

#[repr(C)]
pub struct RimeCommit {
    pub data_size: c_int,
    pub text: *mut c_char,
}

#[repr(C)]
pub struct RimeContext {
    pub data_size: c_int,
    pub composition: RimeComposition,
    pub menu: RimeMenu,
    pub commit_text_preview: *mut c_char,
    pub select_labels: *mut *mut c_char,
}

#[repr(C)]
pub struct RimeStatus {
    pub data_size: c_int,
    pub schema_id: *mut c_char,
    pub schema_name: *mut c_char,
    pub is_disabled: Bool,
    pub is_composing: Bool,
    pub is_ascii_mode: Bool,
    pub is_full_shape: Bool,
    pub is_simplified: Bool,
    pub is_traditional: Bool,
    pub is_ascii_punct: Bool,
}

#[repr(C)]
pub struct RimeCandidateListIterator {
    pub ptr: *mut c_void,
    pub index: c_int,
    pub candidate: RimeCandidate,
}

pub type RimeNotificationHandler = Option<
    unsafe extern "C" fn(
        context_object: *mut c_void,
        session_id: RimeSessionId,
        message_type: *const c_char,
        message_value: *const c_char,
    ),
>;

type SessionFn<R> = Option<unsafe extern "C" fn(RimeSessionId) -> R>;

/// `RimeApi` of librime 1.17.0: `data_size` + 98 function pointers, in header order.
#[repr(C)]
pub struct RimeApi {
    pub data_size: c_int,
    pub setup: Option<unsafe extern "C" fn(*mut RimeTraits)>,
    pub set_notification_handler: Option<unsafe extern "C" fn(RimeNotificationHandler, *mut c_void)>,
    pub initialize: Option<unsafe extern "C" fn(*mut RimeTraits)>,
    pub finalize: Option<unsafe extern "C" fn()>,
    pub start_maintenance: Option<unsafe extern "C" fn(Bool) -> Bool>,
    pub is_maintenance_mode: Option<unsafe extern "C" fn() -> Bool>,
    pub join_maintenance_thread: Option<unsafe extern "C" fn()>,
    pub deployer_initialize: Option<unsafe extern "C" fn(*mut RimeTraits)>,
    pub prebuild: Option<unsafe extern "C" fn() -> Bool>,
    pub deploy: Option<unsafe extern "C" fn() -> Bool>,
    pub deploy_schema: Option<unsafe extern "C" fn(*const c_char) -> Bool>,
    pub deploy_config_file: Unused,
    pub sync_user_data: Option<unsafe extern "C" fn() -> Bool>,
    pub create_session: Option<unsafe extern "C" fn() -> RimeSessionId>,
    pub find_session: SessionFn<Bool>,
    pub destroy_session: SessionFn<Bool>,
    pub cleanup_stale_sessions: Unused,
    pub cleanup_all_sessions: Option<unsafe extern "C" fn()>,
    pub process_key: Option<unsafe extern "C" fn(RimeSessionId, c_int, c_int) -> Bool>,
    pub commit_composition: SessionFn<Bool>,
    pub clear_composition: SessionFn<()>,
    pub get_commit: Option<unsafe extern "C" fn(RimeSessionId, *mut RimeCommit) -> Bool>,
    pub free_commit: Option<unsafe extern "C" fn(*mut RimeCommit) -> Bool>,
    pub get_context: Option<unsafe extern "C" fn(RimeSessionId, *mut RimeContext) -> Bool>,
    pub free_context: Option<unsafe extern "C" fn(*mut RimeContext) -> Bool>,
    pub get_status: Option<unsafe extern "C" fn(RimeSessionId, *mut RimeStatus) -> Bool>,
    pub free_status: Option<unsafe extern "C" fn(*mut RimeStatus) -> Bool>,
    pub set_option: Option<unsafe extern "C" fn(RimeSessionId, *const c_char, Bool)>,
    pub get_option: Option<unsafe extern "C" fn(RimeSessionId, *const c_char) -> Bool>,
    pub set_property: Unused,
    pub get_property: Unused,
    pub get_schema_list: Unused,
    pub free_schema_list: Unused,
    pub get_current_schema: Option<unsafe extern "C" fn(RimeSessionId, *mut c_char, usize) -> Bool>,
    pub select_schema: Option<unsafe extern "C" fn(RimeSessionId, *const c_char) -> Bool>,
    pub schema_open: Unused,
    pub config_open: Unused,
    pub config_close: Unused,
    pub config_get_bool: Unused,
    pub config_get_int: Unused,
    pub config_get_double: Unused,
    pub config_get_string: Unused,
    pub config_get_cstring: Unused,
    pub config_update_signature: Unused,
    pub config_begin_map: Unused,
    pub config_next: Unused,
    pub config_end: Unused,
    pub simulate_key_sequence: Option<unsafe extern "C" fn(RimeSessionId, *const c_char) -> Bool>,
    pub register_module: Unused,
    pub find_module: Option<unsafe extern "C" fn(*const c_char) -> *mut RimeModule>,
    pub run_task: Option<unsafe extern "C" fn(*const c_char) -> Bool>,
    pub get_shared_data_dir: Unused,
    pub get_user_data_dir: Unused,
    pub get_sync_dir: Unused,
    pub get_user_id: Unused,
    pub get_user_data_sync_dir: Unused,
    pub config_init: Unused,
    pub config_load_string: Unused,
    pub config_set_bool: Unused,
    pub config_set_int: Unused,
    pub config_set_double: Unused,
    pub config_set_string: Unused,
    pub config_get_item: Unused,
    pub config_set_item: Unused,
    pub config_clear: Unused,
    pub config_create_list: Unused,
    pub config_create_map: Unused,
    pub config_list_size: Unused,
    pub config_begin_list: Unused,
    /// Pointer is valid until the next edit.
    pub get_input: SessionFn<*const c_char>,
    pub get_caret_pos: SessionFn<usize>,
    pub select_candidate: Option<unsafe extern "C" fn(RimeSessionId, usize) -> Bool>,
    pub get_version: Option<unsafe extern "C" fn() -> *const c_char>,
    pub set_caret_pos: Option<unsafe extern "C" fn(RimeSessionId, usize)>,
    pub select_candidate_on_current_page: Unused,
    pub candidate_list_begin: Option<unsafe extern "C" fn(RimeSessionId, *mut RimeCandidateListIterator) -> Bool>,
    pub candidate_list_next: Option<unsafe extern "C" fn(*mut RimeCandidateListIterator) -> Bool>,
    pub candidate_list_end: Option<unsafe extern "C" fn(*mut RimeCandidateListIterator)>,
    pub user_config_open: Unused,
    pub candidate_list_from_index:
        Option<unsafe extern "C" fn(RimeSessionId, *mut RimeCandidateListIterator, c_int) -> Bool>,
    pub get_prebuilt_data_dir: Unused,
    pub get_staging_dir: Unused,
    pub commit_proto: Unused,
    pub context_proto: Unused,
    pub status_proto: Unused,
    pub get_state_label: Unused,
    pub delete_candidate: Option<unsafe extern "C" fn(RimeSessionId, usize) -> Bool>,
    pub delete_candidate_on_current_page: Unused,
    pub get_state_label_abbreviated: Unused,
    pub set_input: Option<unsafe extern "C" fn(RimeSessionId, *const c_char) -> Bool>,
    pub get_shared_data_dir_s: Unused,
    pub get_user_data_dir_s: Unused,
    pub get_prebuilt_data_dir_s: Unused,
    pub get_staging_dir_s: Unused,
    pub get_sync_dir_s: Unused,
    pub highlight_candidate: Option<unsafe extern "C" fn(RimeSessionId, usize) -> Bool>,
    pub highlight_candidate_on_current_page: Unused,
    pub change_page: Unused,
}

/// `RimeModule` (`rime_api.h`); `get_api` returns the module's own API table.
#[repr(C)]
pub struct RimeModule {
    pub data_size: c_int,
    pub module_name: *const c_char,
    pub initialize: Option<unsafe extern "C" fn()>,
    pub finalize: Option<unsafe extern "C" fn()>,
    pub get_api: Option<unsafe extern "C" fn() -> *mut c_void>,
}

/// `RimeUserDictIterator` (`rime_levers_api.h`).
#[repr(C)]
pub struct RimeUserDictIterator {
    pub ptr: *mut c_void,
    pub i: usize,
}

/// `RimeLeversApi` of librime 1.17.0 (`rime_levers_api.h`, module "levers"): `data_size` + 32
/// function pointers, in header order. Only the user-dictionary slots are typed.
#[repr(C)]
pub struct RimeLeversApi {
    pub data_size: c_int,
    // custom settings (11)
    pub custom_settings_init: Unused,
    pub custom_settings_destroy: Unused,
    pub load_settings: Unused,
    pub save_settings: Unused,
    pub customize_bool: Unused,
    pub customize_int: Unused,
    pub customize_double: Unused,
    pub customize_string: Unused,
    pub is_first_run: Unused,
    pub settings_is_modified: Unused,
    pub settings_get_config: Unused,
    // switcher settings (13)
    pub switcher_settings_init: Unused,
    pub get_available_schema_list: Unused,
    pub get_selected_schema_list: Unused,
    pub schema_list_destroy: Unused,
    pub get_schema_id: Unused,
    pub get_schema_name: Unused,
    pub get_schema_version: Unused,
    pub get_schema_author: Unused,
    pub get_schema_description: Unused,
    pub get_schema_file_path: Unused,
    pub select_schemas: Unused,
    pub get_hotkeys: Unused,
    pub set_hotkeys: Unused,
    // user dictionaries (7)
    pub user_dict_iterator_init: Option<unsafe extern "C" fn(*mut RimeUserDictIterator) -> Bool>,
    pub user_dict_iterator_destroy: Option<unsafe extern "C" fn(*mut RimeUserDictIterator)>,
    pub next_user_dict: Option<unsafe extern "C" fn(*mut RimeUserDictIterator) -> *const c_char>,
    pub backup_user_dict: Option<unsafe extern "C" fn(*const c_char) -> Bool>,
    pub restore_user_dict: Option<unsafe extern "C" fn(*const c_char) -> Bool>,
    /// Number of entries written, or -1.
    pub export_user_dict: Option<unsafe extern "C" fn(*const c_char, *const c_char) -> c_int>,
    /// Number of entries read, or -1.
    pub import_user_dict: Option<unsafe extern "C" fn(*const c_char, *const c_char) -> c_int>,
    pub customize_item: Unused,
}

/// A zeroed struct with `data_size` set per `RIME_STRUCT_INIT`.
///
/// # Safety
/// `T` must be one of the `data_size`-headed structs above (all-zero is a valid value and
/// the first field is a `c_int`).
pub unsafe fn rime_struct<T>() -> T {
    let mut v: T = unsafe { std::mem::zeroed() };
    let size = (std::mem::size_of::<T>() - std::mem::size_of::<c_int>()) as c_int;
    unsafe { *(&mut v as *mut T as *mut c_int) = size };
    v
}

/// X11 keysyms librime understands (`rime/key_table.h`).
pub mod keysym {
    pub const BACKSPACE: i32 = 0xff08;
    pub const RETURN: i32 = 0xff0d;
    pub const ESCAPE: i32 = 0xff1b;
    pub const DELETE: i32 = 0xffff;
    pub const APOSTROPHE: i32 = 0x27;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    // Layouts for 64-bit targets (Dianmo only ships x64).
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn layouts_match_rime_api_h() {
        assert_eq!(size_of::<RimeTraits>(), 96);
        assert_eq!(offset_of!(RimeTraits, min_log_level), 64);
        assert_eq!(offset_of!(RimeTraits, staging_dir), 88);
        assert_eq!(size_of::<RimeComposition>(), 24);
        assert_eq!(size_of::<RimeCandidate>(), 24);
        assert_eq!(size_of::<RimeMenu>(), 40);
        assert_eq!(offset_of!(RimeMenu, candidates), 24);
        assert_eq!(size_of::<RimeCommit>(), 16);
        assert_eq!(size_of::<RimeContext>(), 8 + 24 + 40 + 16);
        assert_eq!(offset_of!(RimeContext, composition), 8);
        assert_eq!(offset_of!(RimeContext, menu), 32);
        assert_eq!(size_of::<RimeStatus>(), 8 + 16 + 28 + 4);
        assert_eq!(size_of::<RimeCandidateListIterator>(), 8 + 8 + 24);
        // data_size + 98 function pointers
        assert_eq!(size_of::<RimeApi>(), 8 + 98 * 8);
        assert_eq!(offset_of!(RimeApi, get_input), 8 + 69 * 8);
        assert_eq!(offset_of!(RimeApi, set_input), 8 + 89 * 8);
        assert_eq!(offset_of!(RimeApi, change_page), 8 + 97 * 8);
        assert_eq!(offset_of!(RimeApi, find_module), 8 + 49 * 8);
        assert_eq!(size_of::<RimeModule>(), 8 + 4 * 8);
        // data_size + 32 function pointers
        assert_eq!(size_of::<RimeLeversApi>(), 8 + 32 * 8);
        assert_eq!(offset_of!(RimeLeversApi, user_dict_iterator_init), 8 + 24 * 8);
        assert_eq!(offset_of!(RimeLeversApi, export_user_dict), 8 + 29 * 8);
        assert_eq!(offset_of!(RimeLeversApi, customize_item), 8 + 31 * 8);
    }

    #[test]
    fn rime_struct_sets_data_size() {
        let t: RimeTraits = unsafe { rime_struct() };
        assert_eq!(t.data_size as usize, size_of::<RimeTraits>() - 4);
        let c: RimeContext = unsafe { rime_struct() };
        assert_eq!(c.data_size as usize, size_of::<RimeContext>() - 4);
        assert!(c.composition.preedit.is_null());
    }
}
