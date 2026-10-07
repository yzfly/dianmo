//! Recognize WAV files with a small streaming model and print accuracy / speed / CPU / memory.
//! No window, no audio device: it only reads files.
//!
//!   asr_wav <model_dir> [wav ...] [--list list.tsv] [--threads N] [--realtime] [--chunk-ms 100]
//!           [--tail-ms 0] [--dll-dir DIR]
//!
//! list.tsv: `path<TAB>reference` per line (reference optional) → CER per file and in total.
//! --realtime feeds the audio at real-time pace in `chunk-ms` chunks (like a microphone) and
//! measures first-char latency, how long after the end of speech the text is complete, the cost of
//! `finish()` after "stop", and CPU per utterance. Without it, files are decoded as fast as possible
//! (RTF). `tail-ms` appends silence after each file (user pauses before tapping stop).

use dianmo_asr::{Options, Recognizer, eval, wav};
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct Item {
    path: PathBuf,
    reference: Option<String>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("错误：{e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut model_dir = None;
    let mut items = Vec::new();
    let mut opts = Options {
        endpoint: false,
        ..Options::default()
    };
    let (mut realtime, mut chunk_ms, mut tail_ms, mut lead_ms) = (false, 100u64, 0u64, 0u64);
    while let Some(a) = args.next() {
        let mut val = |n: &str| args.next().ok_or(format!("{n} 需要参数"));
        match a.as_str() {
            "--threads" => opts.threads = val("--threads")?.parse().map_err(|_| "--threads N")?,
            "--realtime" => realtime = true,
            "--chunk-ms" => chunk_ms = val("--chunk-ms")?.parse().map_err(|_| "--chunk-ms N")?,
            "--lead-ms" => lead_ms = val("--lead-ms")?.parse().map_err(|_| "--lead-ms N")?,
            "--tail-ms" => tail_ms = val("--tail-ms")?.parse().map_err(|_| "--tail-ms N")?,
            "--pad-ms" => {
                opts.tail_padding_ms = val("--pad-ms")?.parse().map_err(|_| "--pad-ms N")?
            }
            "--dll-dir" => opts.dll_dir = Some(val("--dll-dir")?.into()),
            "--list" => {
                let p = val("--list")?;
                let s = std::fs::read_to_string(&p).map_err(|e| format!("{p}: {e}"))?;
                for line in s.lines().filter(|l| !l.trim().is_empty()) {
                    let mut it = line.splitn(2, '\t');
                    let path = it.next().unwrap_or_default().trim().into();
                    items.push(Item {
                        path,
                        reference: it.next().map(|r| r.trim().to_string()),
                    });
                }
            }
            _ if model_dir.is_none() => model_dir = Some(PathBuf::from(a)),
            _ => items.push(Item {
                path: a.into(),
                reference: None,
            }),
        }
    }
    let model_dir = model_dir.ok_or(
        "用法：asr_wav <model_dir> [wav ...] [--list list.tsv] [--threads N] [--realtime]",
    )?;

    let mem0 = usage::memory();
    let t = Instant::now();
    let mut rec = Recognizer::with_options(&model_dir, &opts)?;
    let load = t.elapsed();
    let mem1 = usage::memory();
    let model_mb: f64 = rec
        .files()
        .paths()
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len() as f64)
        .sum::<f64>()
        / 1048576.0;
    println!(
        "# sherpa-onnx {}  model {}  ({model_mb:.1} MB)  threads {}  finish padding {} ms",
        rec.version(),
        model_dir.display(),
        opts.threads,
        opts.tail_padding_ms
    );
    println!(
        "# load {:.0} ms   private {:.1} → {:.1} MB   working set {:.1} MB",
        load.as_secs_f64() * 1e3,
        mem0.private_mb,
        mem1.private_mb,
        mem1.working_set_mb
    );
    println!("# lead {lead_ms} ms, tail {tail_ms} ms of silence added to each file");
    if realtime {
        println!("# realtime: chunk {chunk_ms} ms");
        println!("id\tdur_s\tfirst_ms\tcomplete_ms\tfinish_ms\tcpu_avg%\tcpu_peak%\tcer\ttext");
    } else {
        println!("id\tdur_s\tdecode_ms\trtf\tcpu_ms\tcer\ttext");
    }

    let (mut edits, mut ref_len, mut audio_s, mut cpu_s, mut busy_s) =
        (0usize, 0usize, 0f64, 0f64, 0f64);
    let mut peak_win = 0f64;
    for it in &items {
        let w = wav::read(&it.path)?;
        let id = it
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let lead = (w.sample_rate as u64 * lead_ms / 1000) as usize;
        let (onset, offset) = speech_span(&w);
        let (onset, offset) = (onset + lead_ms as f64 / 1e3, offset + lead_ms as f64 / 1e3);
        let mut samples = vec![0.0; lead];
        samples.extend_from_slice(&w.samples);
        samples.extend(std::iter::repeat_n(
            0.0,
            (w.sample_rate as u64 * tail_ms / 1000) as usize,
        ));
        let dur = samples.len() as f64 / w.sample_rate as f64;
        let cpu0 = usage::cpu_seconds();
        let text;
        let row;
        if realtime {
            let n = (w.sample_rate as u64 * chunk_ms / 1000) as usize;
            let start = Instant::now();
            let mut first: Option<f64> = None;
            let mut changes: Vec<(f64, String)> = Vec::new();
            let (mut win_t, mut win_cpu, mut upeak) = (0f64, cpu0, 0f64);
            for (i, c) in samples.chunks(n).enumerate() {
                let due =
                    Duration::from_secs_f64(((i * n + c.len()) as f64) / w.sample_rate as f64);
                if let Some(d) = due.checked_sub(start.elapsed()) {
                    std::thread::sleep(d);
                }
                rec.accept_waveform_at(w.sample_rate, c);
                let p = rec.partial();
                let now = start.elapsed().as_secs_f64();
                if first.is_none() && !p.is_empty() {
                    first = Some(now);
                }
                if changes.last().map(|(_, s)| s != &p).unwrap_or(true) {
                    changes.push((now, p));
                }
                if now - win_t >= 0.5 {
                    let cpu = usage::cpu_seconds();
                    upeak = upeak.max((cpu - win_cpu) / (now - win_t));
                    (win_t, win_cpu) = (now, cpu);
                }
            }
            let stop = start.elapsed().as_secs_f64();
            text = rec.finish();
            let fin = start.elapsed().as_secs_f64();
            busy_s += fin - stop;
            let target = eval::normalize(&text);
            let complete = changes
                .iter()
                .find(|(_, s)| eval::normalize(s) == target)
                .map(|(t, _)| *t)
                .unwrap_or(fin);
            let cpu = usage::cpu_seconds() - cpu0;
            cpu_s += cpu;
            peak_win = peak_win.max(upeak);
            row = format!(
                "{id}\t{dur:.2}\t{}\t{:.0}\t{:.0}\t{:.1}\t{:.1}",
                first
                    .map(|f| format!("{:.0}", (f - onset) * 1e3))
                    .unwrap_or("-".into()),
                (complete - offset) * 1e3,
                (fin - stop) * 1e3,
                cpu / fin * 100.0,
                upeak * 100.0
            );
        } else {
            let t = Instant::now();
            rec.accept_waveform_at(w.sample_rate, &samples);
            text = rec.finish();
            let d = t.elapsed().as_secs_f64();
            busy_s += d;
            let cpu = usage::cpu_seconds() - cpu0;
            cpu_s += cpu;
            row = format!(
                "{id}\t{dur:.2}\t{:.0}\t{:.3}\t{:.0}",
                d * 1e3,
                d / dur,
                cpu * 1e3
            );
        }
        audio_s += dur;
        let cer = match &it.reference {
            Some(r) => {
                let (e, l) = eval::cer_counts(r, &text);
                edits += e;
                ref_len += l;
                format!("{:.1}%", e as f64 / l.max(1) as f64 * 100.0)
            }
            None => "-".into(),
        };
        println!("{row}\t{cer}\t{text}");
    }
    let mem2 = usage::memory();
    println!(
        "# total: audio {audio_s:.1} s, CER {}, cpu {cpu_s:.2} s ({:.1}% of one core over the audio), {}",
        if ref_len > 0 {
            format!(
                "{:.2}% ({edits}/{ref_len})",
                edits as f64 / ref_len as f64 * 100.0
            )
        } else {
            "-".into()
        },
        cpu_s / audio_s * 100.0,
        if realtime {
            format!(
                "peak 0.5 s window {:.0}% of one core, finish() total {:.0} ms",
                peak_win * 100.0,
                busy_s * 1e3
            )
        } else {
            format!("RTF {:.3}", busy_s / audio_s)
        }
    );
    println!(
        "# memory: private now {:.1} MB, peak private {:.1} MB, peak working set {:.1} MB",
        mem2.private_mb, mem2.peak_private_mb, mem2.peak_working_set_mb
    );
    Ok(())
}

/// Start/end of speech in seconds: 20 ms frames whose RMS exceeds 10% of the loudest frame.
fn speech_span(w: &wav::Wav) -> (f64, f64) {
    let n = (w.sample_rate / 50) as usize;
    let rms: Vec<f32> = w
        .samples
        .chunks(n)
        .map(|c| (c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let max = rms.iter().cloned().fold(0.0, f32::max);
    let th = max * 0.1;
    let first = rms.iter().position(|&r| r > th).unwrap_or(0);
    let last = rms
        .iter()
        .rposition(|&r| r > th)
        .unwrap_or(rms.len().saturating_sub(1));
    (first as f64 * 0.02, (last + 1) as f64 * 0.02)
}

#[cfg(windows)]
mod usage {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    pub struct Mem {
        pub private_mb: f64,
        pub peak_private_mb: f64,
        pub working_set_mb: f64,
        pub peak_working_set_mb: f64,
    }

    pub fn memory() -> Mem {
        let mut c = PROCESS_MEMORY_COUNTERS_EX {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        unsafe {
            let _ = K32GetProcessMemoryInfo(
                GetCurrentProcess(),
                &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
                c.cb,
            );
        }
        let mb = |x: usize| x as f64 / 1048576.0;
        Mem {
            private_mb: mb(c.PrivateUsage),
            peak_private_mb: mb(c.PeakPagefileUsage),
            working_set_mb: mb(c.WorkingSetSize),
            peak_working_set_mb: mb(c.PeakWorkingSetSize),
        }
    }

    /// User + kernel CPU time of this process, all threads.
    pub fn cpu_seconds() -> f64 {
        let (mut a, mut b, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        unsafe {
            let _ = GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut k, &mut u);
        }
        let f =
            |t: FILETIME| ((t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64) as f64 * 1e-7;
        f(k) + f(u)
    }
}

#[cfg(not(windows))]
mod usage {
    pub struct Mem {
        pub private_mb: f64,
        pub peak_private_mb: f64,
        pub working_set_mb: f64,
        pub peak_working_set_mb: f64,
    }
    pub fn memory() -> Mem {
        Mem {
            private_mb: 0.0,
            peak_private_mb: 0.0,
            working_set_mb: 0.0,
            peak_working_set_mb: 0.0,
        }
    }
    pub fn cpu_seconds() -> f64 {
        0.0
    }
}
