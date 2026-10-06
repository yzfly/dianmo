//! 点墨 Dianmo installer support: the payload format shared by the packer (`dianmo-pack`) and the
//! installer stub (`dianmo-setup`).
//!
//! `DianmoSetup-<version>.exe` = the stub exe, then the payload, then a 32-byte trailer:
//!
//! ```text
//! payload  := header entry*
//! header   := "DMPL" u32:entry_count u16:len version(utf8)
//! entry    := u16:len path(utf8, '/'-separated, relative) u64:size u64:packed_len zlib-data
//! trailer  := "DMSETUP1" u64:payload_offset u64:payload_len u64:total_size
//! ```
//!
//! All integers little-endian. Each file is one zlib stream (deflate + Adler-32, checked when
//! extracting). Appending keeps the stub generic (built once, no `include_bytes!` of 20+ MB) and lets
//! the stub find its payload by reading its own last 32 bytes.

pub mod payload;
