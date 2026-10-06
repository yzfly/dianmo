//! Manual test of the voice module (`src/voice.rs`) without the keyboard app.
//!
//!   voice_probe [--engine wetype|doubao|system] [--secs N] [--cancel] [--no-launch] [--no-fallback]
//!   voice_probe --avail            availability of every engine
//!   voice_probe --clip-roundtrip   save clipboard, overwrite it, restore, compare fingerprints
//!
//! Start → wait N seconds (default 4) → stop (or cancel), polling every 300 ms like the host
//! does, and prints state changes, the session report and whether the clipboard came back
//! unchanged. Never prints recognized text or clipboard contents (only counts and hashes): the
//! microphone hears the real room.
//!
//! Run it from a console/hidden PowerShell while the target window (e.g. Notepad) has focus; it
//! has no window of its own and never takes focus.

#[cfg(windows)]
#[path = "../src/voice.rs"]
mod voice;

#[cfg(not(windows))]
fn main() {
    eprintln!("voice_probe runs on Windows only");
}

#[cfg(windows)]
fn main() {
    use std::time::{Duration, Instant};
    use voice::{ClipSnapshot, Voice, VoiceEngine};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let opt = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();

    let fp = || match ClipSnapshot::take() {
        (Some(s), _) => format!("{:016x} ({} formats, {} skipped)", s.fingerprint(), s.format_count(), s.skipped),
        _ => "<clipboard busy>".into(),
    };

    if flag("--avail") {
        for e in VoiceEngine::ALL {
            let v = Voice::new(e);
            println!("{:<7} available={} {}", e.as_setting(), Voice::available(e), v.unavailable_reason().unwrap_or_default());
        }
        return;
    }

    if flag("--clip-roundtrip") {
        let before = fp();
        let (snap, ok) = ClipSnapshot::take();
        let Some(snap) = snap.filter(|_| ok) else {
            println!("clipboard busy");
            return;
        };
        let put = voice::put_clipboard_text("点墨 voice_probe 剪贴板测试");
        let mid = fp();
        let restored = snap.restore();
        let after = fp();
        println!("before:   {before}\noverwrite ok={put}: {mid}\nrestored={restored:?}\nafter:    {after}\nequal={}", before == after);
        return;
    }

    let engine = opt("--engine").and_then(|s| VoiceEngine::from_setting(&s)).unwrap_or(VoiceEngine::WeType);
    let secs: f64 = opt("--secs").and_then(|s| s.parse().ok()).unwrap_or(4.0);
    let mut v = Voice::new(engine);
    v.set_log(|m| println!("    log: {m}"));
    if flag("--no-fallback") {
        v.set_fallback(false);
    }
    if flag("--no-launch") {
        v.set_doubao_launch(false);
    }
    let t0 = Instant::now();
    let ms = move || t0.elapsed().as_millis();
    println!("engine={} available={} {}", engine.as_setting(), Voice::available(engine), v.unavailable_reason().unwrap_or_default());
    let clip_before = fp();
    println!("clipboard before: {clip_before}");

    let mut last = v.start();
    println!("{:>6} ms start -> {last:?}", ms());
    let stop_at = Duration::from_secs_f64(secs);
    let mut stopped = false;
    loop {
        std::thread::sleep(Duration::from_millis(300));
        if !stopped && t0.elapsed() >= stop_at {
            stopped = true;
            if flag("--cancel") {
                v.cancel();
                println!("{:>6} ms cancel -> {:?}", ms(), v.state());
            } else {
                println!("{:>6} ms stop -> {:?}", ms(), v.stop());
            }
            last = v.state();
        } else if v.needs_poll() {
            let s = v.poll();
            if s != last {
                println!("{:>6} ms poll -> {s:?}", ms());
                last = s;
            }
        } else if stopped {
            break;
        }
        if t0.elapsed() > stop_at + Duration::from_secs(15) {
            println!("timeout; cancelling");
            v.cancel();
            break;
        }
    }
    let r = v.last_report();
    println!(
        "report: fell_back={} listen_after_ms={:?} clipboard_owner={:?} typed_chars={} clipboard_restored={:?} restored_formats={} skipped_formats={}",
        r.fell_back, r.listen_after_ms, r.clipboard_owner, r.typed_chars, r.clipboard_restored, r.restored_formats, r.skipped_formats
    );
    let clip_after = fp();
    println!("clipboard after:  {clip_after}\nclipboard unchanged={}", clip_before == clip_after);
}
