//! Measures and smoke-tests the Rime engine on Windows.
//!
//!   probe deploy <shared> [--dll <rime.dll>] [--log <dir>]
//!   probe run [--shared <dir>] [--user <dir>] [--dll <rime.dll>] [--log <dir>] [--bench <n>]
//!   probe deploy-user --shared <dir> --user <dir> [--log <dir>]     (fuzzy pinyin user build)
//!   probe v2 --shared <dir> --user <dir> [--log <dir>]              (v0.2 features end to end)
//!   probe dict --shared <dir> --user <dir> --file <txt>             (user dict, no engine)
//!   probe fuzzy <bits> --shared <dir> --user <dir>                  (set_fuzzy, e.g. 1001000)
//!
//! `--dll` defaults to `rime.dll` next to the exe. `run` defaults: shared `<exe>\data\rime`,
//! user = a fresh temp dir (so "empty user dir, no maintenance" is what gets tested).
//! Prints at most 3 candidates per input.

#[cfg(not(windows))]
fn main() {
    eprintln!("probe only runs on Windows");
}

#[cfg(windows)]
fn main() {
    if let Err(e) = win::main() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
mod win {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use dianmo_core::{Candidate, Engine, Schema, Snapshot};
    use dianmo_rime::{FuzzyChange, Options, RimeEngine, ShuangpinScheme, deploy};

    type Res<T> = Result<T, Box<dyn std::error::Error>>;

    struct Args {
        cmd: String,
        pos: Vec<String>,
        dll: Option<PathBuf>,
        shared: Option<PathBuf>,
        user: Option<PathBuf>,
        log: Option<PathBuf>,
        bench: usize,
        file: Option<PathBuf>,
    }

    fn parse() -> Res<Args> {
        let mut it = std::env::args().skip(1);
        let cmd = it.next().ok_or("usage: probe deploy <shared> | probe run [--shared D] [--user D] | probe deploy-user|v2|dict ...")?;
        let mut a = Args { cmd, pos: vec![], dll: None, shared: None, user: None, log: None, bench: 20, file: None };
        while let Some(x) = it.next() {
            let mut val = || it.next().ok_or_else(|| format!("{x} needs a value"));
            match x.as_str() {
                "--dll" => a.dll = Some(val()?.into()),
                "--shared" => a.shared = Some(val()?.into()),
                "--user" => a.user = Some(val()?.into()),
                "--log" => a.log = Some(val()?.into()),
                "--bench" => a.bench = val()?.parse()?,
                "--file" => a.file = Some(val()?.into()),
                _ => a.pos.push(x),
            }
        }
        Ok(a)
    }

    fn exe_dir() -> PathBuf {
        std::env::current_exe().unwrap().parent().unwrap().to_path_buf()
    }

    fn mem() -> (f64, f64) {
        use windows::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
        };
        use windows::Win32::System::Threading::GetCurrentProcess;
        let mut c = PROCESS_MEMORY_COUNTERS_EX { cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
        unsafe {
            let _ = GetProcessMemoryInfo(GetCurrentProcess(), &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS, c.cb);
        }
        (c.PrivateUsage as f64 / 1048576.0, c.WorkingSetSize as f64 / 1048576.0)
    }

    fn ms(d: Duration) -> f64 {
        d.as_secs_f64() * 1000.0
    }

    pub fn main() -> Res<()> {
        let a = parse()?;
        let dll = a.dll.clone().unwrap_or_else(|| exe_dir().join("rime.dll"));
        match a.cmd.as_str() {
            "deploy" => {
                let shared = PathBuf::from(a.pos.first().ok_or("probe deploy <shared>")?);
                let opts = Options {
                    dll,
                    shared_data_dir: shared.clone(),
                    user_data_dir: std::env::temp_dir(),
                    log_dir: a.log.clone(),
                    min_log_level: if a.log.is_some() { 0 } else { 2 },
                    shuangpin: ShuangpinScheme::Flypy,
                };
                let r = deploy(&opts)?;
                println!("deploy: {} ms, build {:.1} MB", r.millis, r.build_bytes as f64 / 1048576.0);
                let mut files: Vec<_> = std::fs::read_dir(shared.join("build"))?.flatten().collect();
                files.sort_by_key(|e| e.file_name());
                for e in files {
                    println!("  {:>10}  {}", e.metadata()?.len(), e.file_name().to_string_lossy());
                }
                Ok(())
            }
            "run" => run(&a, dll),
            "deploy-user" => deploy_user(&a, dll),
            "v2" => v2(&a, dll),
            "dict" => dict(&a, dll),
            "fuzzy" => {
                let opts = opts_for(&a, dll)?;
                let bits = a.pos.first().ok_or("probe fuzzy <7 bits, e.g. 1001000>")?;
                let mut f = [false; 7];
                for (i, c) in bits.chars().take(7).enumerate() {
                    f[i] = c == '1';
                }
                let c = dianmo_rime::set_fuzzy(&opts, f)?;
                println!("set_fuzzy -> {c:?}; now {:?}; needs_user_deploy={}", dianmo_rime::fuzzy(&opts), dianmo_rime::needs_user_deploy(&opts));
                Ok(())
            }
            _ => Err("unknown command".into()),
        }
    }

    fn show(label: &str, s: &Snapshot) {
        let top: Vec<String> = s.candidates.iter().take(3).map(fmt_cand).collect();
        println!(
            "  {label}: preedit={:?} commit={:?} n={} top3=[{}]",
            s.preedit,
            s.commit,
            s.candidates.len(),
            top.join(", ")
        );
    }

    fn fmt_cand(c: &Candidate) -> String {
        match &c.comment {
            Some(m) => format!("{}({m})", c.text),
            None => c.text.clone(),
        }
    }

    /// Per-key latency collector.
    #[derive(Default)]
    struct Keys {
        all: Vec<Duration>,
    }

    impl Keys {
        fn add(&mut self, d: Duration) {
            self.all.push(d);
        }
        fn report(&self, label: &str) {
            let n = self.all.len();
            if n == 0 {
                return;
            }
            let mut v = self.all.clone();
            v.sort();
            let total: Duration = v.iter().sum();
            let pct = |p: f64| ms(v[((n as f64 * p) as usize).min(n - 1)]);
            println!(
                "  {label}: {n} keys, avg {:.2} ms, p50 {:.2}, p95 {:.2}, max {:.2} ms",
                ms(total) / n as f64,
                pct(0.5),
                pct(0.95),
                ms(v[n - 1])
            );
        }
    }

    fn is_hanzi(s: &str) -> bool {
        s.chars().all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    }

    fn type_str(e: &mut RimeEngine, s: &str, keys: &mut Keys) -> Snapshot {
        let mut last = Snapshot::default();
        for c in s.chars() {
            let t = Instant::now();
            last = e.input(c);
            keys.add(t.elapsed());
        }
        last
    }

    fn run(a: &Args, dll: PathBuf) -> Res<()> {
        let shared = a.shared.clone().unwrap_or_else(|| exe_dir().join("data").join("rime"));
        let user = a.user.clone().unwrap_or_else(|| {
            let d = std::env::temp_dir().join(format!("dianmo-probe-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            d
        });
        let opts = Options {
            dll,
            shared_data_dir: shared,
            user_data_dir: user.clone(),
            log_dir: a.log.clone(),
            min_log_level: if a.log.is_some() { 0 } else { 1 },
            shuangpin: ShuangpinScheme::Flypy,
        };
        let (p0, _) = mem();
        println!("before load: private {p0:.1} MB");

        let t = Instant::now();
        let mut e = RimeEngine::start(&opts, Schema::Pinyin)?;
        println!(
            "start (load+initialize+session+rime_ice): {:.1} ms  librime {}  user build: {}",
            ms(t.elapsed()),
            e.rime_version(),
            e.uses_user_build()
        );
        let (p, w) = mem();
        println!("after start: private {p:.1} MB, working set {w:.1} MB");

        let mut keys = Keys::default();
        let t = Instant::now();
        let s = e.input('n');
        println!("first key: {:.1} ms", ms(t.elapsed()));
        show("n", &s);
        e.clear();

        println!("[全拼]");
        let s = type_str(&mut e, "nihao", &mut keys);
        show("nihao", &s);
        let s = e.backspace();
        show("nihao<BS>", &s);
        let s = e.commit_raw();
        show("nihao<BS> commit_raw", &s);
        type_str(&mut e, "nihao", &mut keys);
        let s = e.select(0);
        show("nihao select(0)", &s);
        let s = type_str(&mut e, "nh", &mut keys);
        show("nh", &s);
        e.clear();
        let s = type_str(&mut e, "jintiantianqihenhao", &mut keys);
        show("jintiantianqihenhao", &s);
        // partial selection: first two-character candidate
        if let Some(i) = s.candidates.iter().position(|c| c.text.chars().count() == 2 && is_hanzi(&c.text)) {
            let s = e.select(i);
            show(&format!("select({i}) [2 chars]"), &s);
            let s = e.backspace();
            show("  then <BS>", &s);
            let s = e.select(i);
            show(&format!("  select({i}) again"), &s);
            let s = e.commit_raw();
            show("  then commit_raw", &s);
        }
        // candidate paging
        type_str(&mut e, "nihao", &mut keys);
        let t = Instant::now();
        let p1 = e.candidates(0, 30);
        let p2 = e.candidates(30, 30);
        let p3 = e.candidates(60, 200);
        println!(
            "  paging nihao: [0,30)={} [30,60)={} [60,260)={} in {:.1} ms; p2 starts with {:?}",
            p1.len(),
            p2.len(),
            p3.len(),
            ms(t.elapsed()),
            p2.first().map(|c| &c.text)
        );
        e.clear();
        for text in ["n", "ni", "nihao", "jintiantianqihenhao"] {
            type_str(&mut e, text, &mut Keys::default());
            let t = Instant::now();
            let n = e.candidates(0, 5000).len();
            println!("  total candidates for {text:?}: {n} ({:.1} ms)", ms(t.elapsed()));
            e.clear();
        }
        type_str(&mut e, "nihao", &mut Keys::default());
        let continuous = p1.len() == 30 && e.candidates(29, 2).first().map(|c| &c.text) == p1.last().map(|c| &c.text);
        println!("  paging consistent: {continuous}");
        e.clear();
        keys.report("全拼");

        println!("[小鹤双拼]");
        let t = Instant::now();
        e.set_schema(Schema::Shuangpin);
        println!("  set_schema: {:.1} ms -> {:?}", ms(t.elapsed()), e.schema());
        let mut k2 = Keys::default();
        let s = type_str(&mut e, "nihc", &mut k2);
        show("nihc", &s);
        let s = e.commit_raw();
        show("nihc commit_raw", &s);
        k2.report("小鹤");

        println!("[九宫格]");
        let t = Instant::now();
        e.set_schema(Schema::T9);
        println!("  set_schema: {:.1} ms -> {:?}", ms(t.elapsed()), e.schema());
        let mut k3 = Keys::default();
        let s = type_str(&mut e, "64426", &mut k3);
        show("64426", &s);
        println!("  input={:?}", e.raw_input());
        let t = Instant::now();
        let sp = e.t9_spellings();
        println!("  t9_spellings ({:.1} ms): {:?}", ms(t.elapsed()), &sp[..sp.len().min(10)]);
        let s = e.pick_t9_spelling("ni");
        show("pick ni", &s);
        println!("  input={:?} spellings={:?}", e.raw_input(), first(&e.t9_spellings(), 8));
        let s = e.pick_t9_spelling("hao");
        show("pick hao", &s);
        println!("  input={:?}", e.raw_input());
        let s = e.commit_raw();
        show("commit_raw", &s);
        // partial selection, then pick for the rest
        let s = type_str(&mut e, "64426", &mut k3);
        if let Some(i) = s.candidates.iter().position(|c| c.text.chars().count() == 1 && is_hanzi(&c.text)) {
            let s = e.select(i);
            show(&format!("select({i}) [1 char]"), &s);
            let sp = e.t9_spellings();
            println!("  input={:?} spellings={:?}", e.raw_input(), first(&sp, 8));
            if let Some(x) = sp.first().cloned() {
                let s = e.pick_t9_spelling(&x);
                show(&format!("pick {x}"), &s);
                println!("  input={:?}", e.raw_input());
            }
            let s = e.backspace();
            show("<BS>", &s);
            let s = e.commit_raw();
            show("commit_raw", &s);
        }
        e.clear();
        let s = type_str(&mut e, "64426", &mut k3);
        let s2 = e.select(0);
        show("64426 select(0)", &s2);
        let _ = s;
        k3.report("九宫格");

        // Benchmark: retype a long input in each schema.
        println!("[bench x{}]", a.bench);
        for (schema, text) in [(Schema::Pinyin, "jintiantianqihenhao"), (Schema::Shuangpin, "nihc"), (Schema::T9, "64426")] {
            e.set_schema(schema);
            let mut k = Keys::default();
            let mut spikes = Vec::new();
            for it in 0..a.bench {
                for (i, c) in text.chars().enumerate() {
                    let t = Instant::now();
                    e.input(c);
                    let d = t.elapsed();
                    k.add(d);
                    if ms(d) > 10.0 {
                        spikes.push(format!("#{it}:{i}={:.0}", ms(d)));
                    }
                }
                e.clear();
            }
            k.report(&format!("{schema:?} {text}"));
            println!("    spikes >10ms (iteration:key): {}", spikes.join(" "));
        }
        let t = Instant::now();
        e.set_schema(Schema::Pinyin);
        println!("  set_schema back to Pinyin: {:.1} ms", ms(t.elapsed()));

        let (p, w) = mem();
        println!("end: private {p:.1} MB, working set {w:.1} MB");
        drop(e);
        let mut files: Vec<_> = std::fs::read_dir(&user).map(|r| r.flatten().collect()).unwrap_or_default();
        files.sort_by_key(|e| e.file_name());
        println!("user dir {}:", user.display());
        for f in files {
            println!("  {}", f.file_name().to_string_lossy());
        }
        if a.user.is_none() {
            let _ = std::fs::remove_dir_all(&user);
        }
        Ok(())
    }

    fn opts_for(a: &Args, dll: PathBuf) -> Res<Options> {
        Ok(Options {
            dll,
            shared_data_dir: a.shared.clone().ok_or("--shared needed")?,
            user_data_dir: a.user.clone().ok_or("--user needed")?,
            log_dir: a.log.clone(),
            min_log_level: if a.log.is_some() { 0 } else { 1 },
            shuangpin: ShuangpinScheme::Flypy,
        })
    }

    fn list_dir(dir: &std::path::Path) {
        let mut files: Vec<_> = std::fs::read_dir(dir).map(|r| r.flatten().collect()).unwrap_or_default();
        files.sort_by_key(|e| e.file_name());
        for e in files {
            let len = e.metadata().map(|m| if m.is_dir() { 0 } else { m.len() }).unwrap_or(0);
            println!("    {:>10}  {}", len, e.file_name().to_string_lossy());
        }
    }

    fn deploy_user(a: &Args, dll: PathBuf) -> Res<()> {
        let opts = opts_for(a, dll)?;
        let r = dianmo_rime::deploy_user(&opts)?;
        println!("deploy-user: {} ms, {:.1} KB", r.millis, r.build_bytes as f64 / 1024.0);
        list_dir(&opts.user_data_dir.join("build.new"));
        Ok(())
    }

    /// Top 3 + where `want` is among the first 100 candidates (position only).
    fn probe_input(e: &mut RimeEngine, label: &str, keys: &str, want: &str) {
        e.clear();
        let t = Instant::now();
        let s = type_str(e, keys, &mut Keys::default());
        let d = ms(t.elapsed());
        let all = e.candidates(0, 100);
        let pos = all.iter().position(|c| c.text == want).map(|i| i.to_string()).unwrap_or_else(|| "-".into());
        let top: Vec<String> = s.candidates.iter().take(3).map(fmt_cand).collect();
        println!("  {label} {keys:?}: preedit={:?} top3=[{}] pos({want})={pos} ({d:.1} ms)", s.preedit, top.join(", "));
        e.clear();
    }

    fn v2(a: &Args, dll: PathBuf) -> Res<()> {
        let mut opts = opts_for(a, dll)?;
        let t = Instant::now();
        let mut e = RimeEngine::start(&opts, Schema::Pinyin)?;
        println!("start: {:.1} ms, user build: {}", ms(t.elapsed()), e.uses_user_build());

        println!("[模糊音 off]");
        probe_input(&mut e, "全拼", "zi", "知");
        probe_input(&mut e, "全拼", "lan", "南");
        e.set_schema(Schema::T9);
        probe_input(&mut e, "九宫格", "526", "南");
        e.set_schema(Schema::Shuangpin);
        probe_input(&mut e, "小鹤", "lj", "南");
        e.set_schema(Schema::Pinyin);

        println!("[模糊音 z/zh + n/l on]");
        let fuzzy = [true, false, false, true, false, false, false];
        let c = dianmo_rime::set_fuzzy(&opts, fuzzy)?;
        println!("  set_fuzzy -> {c:?}, needs_user_deploy={}", dianmo_rime::needs_user_deploy(&opts));
        if c == FuzzyChange::Deploy {
            let exe = std::env::current_exe()?;
            let mut cmd = std::process::Command::new(exe);
            cmd.arg("deploy-user").arg("--shared").arg(&opts.shared_data_dir).arg("--user").arg(&opts.user_data_dir);
            cmd.arg("--dll").arg(&opts.dll);
            if let Some(l) = &opts.log_dir {
                cmd.arg("--log").arg(l);
            }
            let t = Instant::now();
            let out = cmd.output()?;
            println!("  deploy-user process: {:.0} ms, exit {:?}", ms(t.elapsed()), out.status.code());
            for line in String::from_utf8_lossy(&out.stdout).lines().chain(String::from_utf8_lossy(&out.stderr).lines()) {
                println!("  | {line}");
            }
        }
        println!("  needs_user_deploy={}", dianmo_rime::needs_user_deploy(&opts));
        let t = Instant::now();
        e.reload(&opts)?;
        println!("  reload: {:.1} ms, user build: {}", ms(t.elapsed()), e.uses_user_build());
        probe_input(&mut e, "全拼", "zi", "知");
        probe_input(&mut e, "全拼", "lan", "南");
        e.set_schema(Schema::T9);
        probe_input(&mut e, "九宫格", "526", "南");
        e.set_schema(Schema::Shuangpin);
        probe_input(&mut e, "小鹤", "zi", "知");
        probe_input(&mut e, "小鹤", "lj", "南");
        e.set_schema(Schema::Pinyin);
        let s = type_str(&mut e, "nihao", &mut Keys::default());
        show("全拼 nihao (fuzzy on)", &s);
        e.clear();

        println!("[双拼方案]");
        e.set_schema(Schema::Shuangpin);
        for (sp, codes) in [
            (ShuangpinScheme::Flypy, ["nihc", "ulpb", "mktm"]),
            (ShuangpinScheme::Ziranma, ["nihk", "udpn", "mytm"]),
            (ShuangpinScheme::Mspy, ["nihk", "udpn", "m;tm"]),
            (ShuangpinScheme::Sogou, ["nihk", "udpn", "m;tm"]),
        ] {
            let t = Instant::now();
            e.set_shuangpin(sp);
            println!(" {} ({}): set_shuangpin {:.1} ms", sp.name(), sp.schema_id(), ms(t.elapsed()));
            for (code, want) in codes.iter().zip(["你好", "双拼", "明天"]) {
                probe_input(&mut e, "  ", code, want);
            }
        }
        e.set_shuangpin(ShuangpinScheme::Flypy);
        e.set_schema(Schema::Pinyin);

        println!("[用户词库]");
        let t = Instant::now();
        println!("  count: {:?} ({:.1} ms)", e.user_word_count(), ms(t.elapsed()));
        for w in ["nihao", "jintian", "shurufa", "dianmo"] {
            type_str(&mut e, w, &mut Keys::default());
            let s = e.select(0);
            println!("  learn {w}: commit={:?}", s.commit.is_some());
            e.clear();
        }
        let t = Instant::now();
        println!("  count after typing: {:?} ({:.1} ms)", e.user_word_count(), ms(t.elapsed()));
        let file = a.file.clone().unwrap_or_else(|| opts.user_data_dir.join("..").join("dianmo-export.txt"));
        let t = Instant::now();
        let n = e.export_user_dict(&file)?;
        let lines = std::fs::read_to_string(&file).map(|t| t.lines().count()).unwrap_or(0);
        println!("  export: {n} entries, file {} lines, {} bytes ({:.1} ms)", lines, std::fs::metadata(&file)?.len(), ms(t.elapsed()));
        let t = Instant::now();
        e.clear_user_dict()?;
        println!("  clear: {:.1} ms; count {:?}; userdb exists {}", ms(t.elapsed()), e.user_word_count(), opts.user_data_dir.join("rime_ice.userdb").exists());
        let t = Instant::now();
        let n = e.import_user_dict(&file)?;
        println!("  import: {n} entries ({:.1} ms); count {:?}", ms(t.elapsed()), e.user_word_count());
        let s = type_str(&mut e, "nihao", &mut Keys::default());
        show("nihao after import", &s);
        e.clear();

        println!("[模糊音 all off]");
        let c = dianmo_rime::set_fuzzy(&opts, [false; 7])?;
        println!("  set_fuzzy -> {c:?}");
        let t = Instant::now();
        opts.shuangpin = ShuangpinScheme::Flypy;
        e.reload(&opts)?;
        println!("  reload: {:.1} ms, user build: {}", ms(t.elapsed()), e.uses_user_build());
        probe_input(&mut e, "全拼", "zi", "知");
        let (p, w) = mem();
        println!("end: private {p:.1} MB, working set {w:.1} MB");
        drop(e);
        println!("user dir:");
        list_dir(&opts.user_data_dir);
        Ok(())
    }

    /// The free functions (no engine in this process).
    fn dict(a: &Args, dll: PathBuf) -> Res<()> {
        let opts = opts_for(a, dll)?;
        let file = a.file.clone().ok_or("--file needed")?;
        println!("count: {:?}", dianmo_rime::user_word_count(&opts));
        let n = dianmo_rime::export_user_dict(&opts, &file)?;
        println!("export: {n}");
        dianmo_rime::clear_user_dict(&opts)?;
        println!("clear ok; count: {:?}", dianmo_rime::user_word_count(&opts));
        let n = dianmo_rime::import_user_dict(&opts, &file)?;
        println!("import: {n}; count: {:?}", dianmo_rime::user_word_count(&opts));
        Ok(())
    }

    fn first(v: &[String], n: usize) -> &[String] {
        &v[..v.len().min(n)]
    }
}
