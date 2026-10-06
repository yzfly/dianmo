//! Links `examples/res/examples.rc` (icon + RCDATA PNG) into the examples on Windows targets, so
//! `window_demo` can show the exe icon and an embedded image like the real app does.
//!
//! Uses `llvm-windres` / `windres` from llvm-mingw (on PATH in `scripts/surface/build.sh`). Without
//! a resource compiler (e.g. `cargo check` on the Linux server) the examples have no resources.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=examples/res");
    println!("cargo:rerun-if-changed=../dianmo/res/dianmo.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("examples").join("res");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("examples-res.o");
    for tool in ["llvm-windres", "x86_64-w64-mingw32-windres", "windres"] {
        let status = Command::new(tool)
            .current_dir(&dir)
            .args(["--target=pe-x86-64", "-O", "coff", "-i", "examples.rc", "-o"])
            .arg(&out)
            .status();
        match status {
            Ok(s) if s.success() => {
                println!("cargo:rustc-link-arg-examples={}", out.display());
                return;
            }
            Ok(s) => println!("cargo:warning={tool} failed ({s}); examples will have no resources"),
            Err(_) => continue,
        }
    }
}
