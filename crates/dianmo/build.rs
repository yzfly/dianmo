//! Embeds the icon, the `app-icon` PNG (settings / about / onboarding) and version info into
//! dianmo.exe on Windows targets, and sets `DIANMO_BUILD_DATE` (YYYY-MM-DD, UTC; override with the
//! environment variable of the same name) for the about page.
//!
//! Uses `llvm-windres` / `windres` from llvm-mingw (on PATH in `scripts/surface/build.sh`). When no
//! resource compiler is found (e.g. `cargo check` on the Linux server) the exe simply has no icon.

use std::env;
use std::path::PathBuf;
use std::process::Command;

/// Civil date (UTC) from unix seconds (Howard Hinnant's `civil_from_days`).
fn date_from_unix(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn main() {
    println!("cargo:rerun-if-changed=res/dianmo.rc");
    println!("cargo:rerun-if-changed=res/dianmo.ico");
    println!("cargo:rerun-if-changed=res/app-icon.png");
    // Any source change re-runs this script, so the build date stays current.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-env-changed=DIANMO_BUILD_DATE");
    let date = env::var("DIANMO_BUILD_DATE").unwrap_or_else(|_| {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        date_from_unix(now)
    });
    println!("cargo:rustc-env=DIANMO_BUILD_DATE={date}");
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
