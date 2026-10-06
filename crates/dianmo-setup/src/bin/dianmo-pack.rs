//! Builds `DianmoSetup-<version>.exe`: a copy of the installer stub with the dist directory appended
//! as the payload (see `dianmo_setup::payload`).
//!
//!   dianmo-pack <dianmo-setup.exe> <dist dir> <out.exe> [--version <v>]
//!
//! The dist dir must contain `dianmo.exe` at its top (what `scripts/surface/package.sh` assembles).
//! Compresses on two threads (the Surface is the user's machine; packaging runs at low priority).

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use dianmo_setup::payload::{self, Packed};

fn main() {
    if let Err(e) = run() {
        eprintln!("dianmo-pack: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut version = env!("CARGO_PKG_VERSION").to_owned();
    if let Some(i) = args.iter().position(|a| a == "--version") {
        version = args.get(i + 1).cloned().ok_or("--version needs a value")?;
        args.drain(i..i + 2);
    }
    let [stub, dist, out] = <[String; 3]>::try_from(args)
        .map_err(|_| "usage: dianmo-pack <dianmo-setup.exe> <dist dir> <out.exe> [--version <v>]".to_owned())?;
    let (stub, dist, out) = (PathBuf::from(stub), PathBuf::from(dist), PathBuf::from(out));
    if !dist.join("dianmo.exe").is_file() {
        return Err(format!("{} has no dianmo.exe", dist.display()));
    }
    let t0 = Instant::now();
    let files = payload::list_dir(&dist).map_err(|e| format!("reading {}: {e}", dist.display()))?;

    // Biggest first so the two workers finish at about the same time.
    let mut jobs: Vec<(usize, u64)> =
        files.iter().enumerate().map(|(i, (_, p))| (i, std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))).collect();
    jobs.sort_by_key(|&(_, len)| std::cmp::Reverse(len));
    let queue = Mutex::new(jobs.into_iter());
    let results: Mutex<Vec<Option<Packed>>> = Mutex::new((0..files.len()).map(|_| None).collect());
    let error: Mutex<Option<String>> = Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..2 {
            s.spawn(|| {
                loop {
                    let Some((i, _)) = queue.lock().unwrap().next() else { break };
                    let (name, path) = &files[i];
                    match std::fs::read(path) {
                        Ok(data) => results.lock().unwrap()[i] = Some(payload::pack(name.clone(), &data)),
                        Err(e) => {
                            *error.lock().unwrap() = Some(format!("reading {}: {e}", path.display()));
                            break;
                        }
                    }
                }
            });
        }
    });
    if let Some(e) = error.into_inner().unwrap() {
        return Err(e);
    }
    let packed: Vec<Packed> = results.into_inner().unwrap().into_iter().map(Option::unwrap).collect();

    let stub_bytes = std::fs::read(&stub).map_err(|e| format!("reading {}: {e}", stub.display()))?;
    if stub_bytes.get(..2) != Some(b"MZ") {
        return Err(format!("{} is not an exe", stub.display()));
    }
    let tmp = out.with_extension("exe.tmp");
    let write = || -> std::io::Result<u64> {
        let mut f = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        f.write_all(&stub_bytes)?;
        let len = payload::write_payload(&mut f, stub_bytes.len() as u64, &version, &packed)?;
        f.flush()?;
        Ok(len)
    };
    let len = write().map_err(|e| format!("writing {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &out).map_err(|e| format!("renaming to {}: {e}", out.display()))?;

    // Self-check: the result must read back.
    let mut f = std::fs::File::open(&out).map_err(|e| e.to_string())?;
    let info = payload::info(&mut f).map_err(|e| format!("self-check failed: {e}"))?;
    let total: u64 = packed.iter().map(|p| p.size).sum();
    println!(
        "{}: {} files, {:.1} MB -> payload {:.1} MB + stub {} KB = {:.1} MB, version {}, {:.1}s",
        out.display(),
        info.count,
        total as f64 / 1048576.0,
        len as f64 / 1048576.0,
        stub_bytes.len() / 1024,
        (stub_bytes.len() as u64 + len + payload::TRAILER_LEN) as f64 / 1048576.0,
        info.version,
        t0.elapsed().as_secs_f32()
    );
    Ok(())
}
