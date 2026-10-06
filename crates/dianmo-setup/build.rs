//! Embeds the icon, the manifest (asInvoker, per-monitor DPI aware, common controls v6) and version
//! info into dianmo-setup.exe on Windows targets (llvm-windres from llvm-mingw, on PATH in
//! `scripts/surface/build.sh`). Without a resource compiler (e.g. `cargo check` on Linux) the exe
//! simply has none.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=res/setup.rc");
    println!("cargo:rerun-if-changed=res/setup.manifest");
    println!("cargo:rerun-if-changed=../dianmo/res/dianmo.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("res");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("setup-res.o");
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
            .args(["--target=pe-x86-64", "-O", "coff", "-i", "setup.rc", "-o"])
            .arg(&out)
            .status();
        match status {
            Ok(s) if s.success() => {
                // Only the installer; the packer is a plain console tool.
                println!("cargo:rustc-link-arg-bin=dianmo-setup={}", out.display());
                return;
            }
            Ok(s) => println!("cargo:warning={tool} failed ({s}); dianmo-setup.exe will have no icon/manifest"),
            Err(_) => continue,
        }
    }
    if env::var("HOST").is_ok_and(|h| h.contains("windows")) {
        println!("cargo:warning=no windres found; dianmo-setup.exe will have no icon/manifest");
    }
}
