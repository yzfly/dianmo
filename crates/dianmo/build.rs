//! Embeds the 「墨」 icon and version info into dianmo.exe on Windows targets.
//!
//! Uses `llvm-windres` / `windres` from llvm-mingw (on PATH in `scripts/surface/build.sh`). When no
//! resource compiler is found (e.g. `cargo check` on the Linux server) the exe simply has no icon.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=res/dianmo.rc");
    println!("cargo:rerun-if-changed=res/dianmo.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("res");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("dianmo-res.o");
    let ver = env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts = ver.split(['.', '-']).map(|p| p.parse::<u16>().unwrap_or(0));
    let (major, minor, patch) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    let defines = [
        format!("-DDIANMO_VER_MAJOR={major}"),
        format!("-DDIANMO_VER_MINOR={minor}"),
        format!("-DDIANMO_VER_PATCH={patch}"),
        format!("-DDIANMO_VER_STR=\\\"{ver}\\\""),
    ];
    for tool in ["llvm-windres", "x86_64-w64-mingw32-windres", "windres"] {
        let status = Command::new(tool)
            .current_dir(&dir)
            .args(&defines)
            .args(["--target=pe-x86-64", "-O", "coff", "-i", "dianmo.rc", "-o"])
            .arg(&out)
            .status();
        match status {
            Ok(s) if s.success() => {
                println!("cargo:rustc-link-arg-bins={}", out.display());
                return;
            }
            Ok(s) => println!("cargo:warning={tool} failed ({s}); dianmo.exe will have no icon"),
            Err(_) => continue,
        }
    }
    if env::var("HOST").is_ok_and(|h| h.contains("windows")) {
        println!("cargo:warning=no windres found; dianmo.exe will have no icon");
    }
}
