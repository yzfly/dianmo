//! Dianmo local speech recognition (prototype, TODO #40).
//!
//! Streaming Chinese ASR with a small sherpa-onnx model (≈25 MB int8). The engine is the official
//! prebuilt `sherpa-onnx-c-api.dll` + `onnxruntime.dll` (MSVC, static CRT), loaded at runtime with
//! `LoadLibraryExW` like `rime.dll`, so nothing here links against MSVC.
//!
//! ```no_run
//! # fn main() -> Result<(), String> {
//! let mut rec = dianmo_asr::Recognizer::new(std::path::Path::new(r"C:\dev\dianmo-asr\models\small-ctc"))?;
//! let samples: Vec<f32> = vec![0.0; 1600]; // 100 ms of 16 kHz mono audio, -1.0..=1.0
//! rec.accept_waveform(&samples);           // decodes as soon as a chunk is ready
//! println!("{}", rec.partial());           // text so far (changes while speaking)
//! println!("{}", rec.finish());            // flush, final text; ready for the next utterance
//! # Ok(()) }
//! ```

pub mod eval;
mod ffi;
mod model;
pub mod text;
pub mod wav;

pub use model::{ModelFiles, ModelKind};

use std::path::{Path, PathBuf};

/// Sample rate of [`Recognizer::accept_waveform`] input.
pub const SAMPLE_RATE: u32 = 16_000;

/// Name of the C API library. `onnxruntime.dll` must sit next to it.
pub const DLL_NAME: &str = "sherpa-onnx-c-api.dll";

#[derive(Clone, Debug)]
pub struct Options {
    /// onnxruntime intra-op threads. 1 is enough for the small models and keeps CPU low.
    pub threads: i32,
    /// Directory containing [`DLL_NAME`]; `None`: `$DIANMO_SHERPA_DIR`, then next to the exe,
    /// then `<exe dir>\sherpa`.
    pub dll_dir: Option<PathBuf>,
    /// Let sherpa-onnx detect end of utterance by trailing silence ([`Recognizer::is_endpoint`]).
    pub endpoint: bool,
    /// Silence fed by [`Recognizer::finish`] before ending the input. Streaming models only emit
    /// the last characters once enough right context has arrived; `InputFinished` alone drops
    /// them (seen as missing last 1–2 chars). The sherpa-onnx examples use 300–660 ms.
    pub tail_padding_ms: u32,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            threads: 1,
            dll_dir: None,
            endpoint: true,
            tail_padding_ms: 500,
        }
    }
}

/// One streaming recognizer + its current utterance stream.
pub struct Recognizer {
    api: ffi::Api,
    rec: ffi::Rec,
    stream: ffi::Stream,
    files: ModelFiles,
    tail_padding: usize,
}

// sherpa-onnx objects may be used from any thread, one at a time.
unsafe impl Send for Recognizer {}

impl Recognizer {
    /// Load the model in `model_dir` (layout detected from the file names, int8 preferred).
    pub fn new(model_dir: &Path) -> Result<Self, String> {
        Self::with_options(model_dir, &Options::default())
    }

    pub fn with_options(model_dir: &Path, opts: &Options) -> Result<Self, String> {
        let files = ModelFiles::detect(model_dir)?;
        let api = loader::load(opts.dll_dir.as_deref())?;
        let c = |p: &Path| {
            std::ffi::CString::new(p.to_string_lossy().into_owned()).map_err(|e| e.to_string())
        };
        let none = || Ok::<_, String>(std::ffi::CString::default());
        let (a, b, d) = match &files.kind {
            ModelKind::Transducer {
                encoder,
                decoder,
                joiner,
            } => (c(encoder)?, c(decoder)?, c(joiner)?),
            ModelKind::Zipformer2Ctc { model } => (c(model)?, none()?, none()?),
        };
        let tokens = c(&files.tokens)?;
        // SAFETY: plain-old-data struct, all-zero is "use the defaults" for the C API.
        let mut cfg: ffi::OnlineRecognizerConfig = unsafe { std::mem::zeroed() };
        cfg.feat_config = ffi::FeatureConfig {
            sample_rate: SAMPLE_RATE as i32,
            feature_dim: 80,
        };
        match files.kind {
            ModelKind::Transducer { .. } => {
                cfg.model_config.transducer = ffi::OnlineTransducerModelConfig {
                    encoder: a.as_ptr(),
                    decoder: b.as_ptr(),
                    joiner: d.as_ptr(),
                };
            }
            ModelKind::Zipformer2Ctc { .. } => cfg.model_config.zipformer2_ctc.model = a.as_ptr(),
        }
        cfg.model_config.tokens = tokens.as_ptr();
        cfg.model_config.num_threads = opts.threads.max(1);
        cfg.model_config.provider = c"cpu".as_ptr();
        cfg.decoding_method = c"greedy_search".as_ptr();
        cfg.enable_endpoint = opts.endpoint as i32;
        cfg.rule1_min_trailing_silence = 2.4;
        cfg.rule2_min_trailing_silence = 1.2;
        cfg.rule3_min_utterance_length = 20.0;
        let rec = unsafe { (api.create_recognizer)(&cfg) };
        if rec.is_null() {
            return Err(format!("sherpa-onnx 无法加载模型 {}", model_dir.display()));
        }
        let stream = unsafe { (api.create_stream)(rec) };
        if stream.is_null() {
            unsafe { (api.destroy_recognizer)(rec) };
            return Err("sherpa-onnx 无法创建识别流".into());
        }
        let tail_padding = (SAMPLE_RATE * opts.tail_padding_ms / 1000) as usize;
        Ok(Recognizer {
            api,
            rec,
            stream,
            files,
            tail_padding,
        })
    }

    pub fn files(&self) -> &ModelFiles {
        &self.files
    }

    /// sherpa-onnx version string of the loaded DLL.
    pub fn version(&self) -> String {
        unsafe { cstr((self.api.version)()) }
    }

    /// Feed 16 kHz mono samples (-1.0..=1.0) and decode every chunk that became ready.
    pub fn accept_waveform(&mut self, samples: &[f32]) {
        self.accept_waveform_at(SAMPLE_RATE, samples)
    }

    /// Same at another sample rate (sherpa-onnx resamples internally).
    pub fn accept_waveform_at(&mut self, sample_rate: u32, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        unsafe {
            (self.api.accept_waveform)(
                self.stream,
                sample_rate as i32,
                samples.as_ptr(),
                samples.len() as i32,
            );
        }
        self.decode_ready();
    }

    fn decode_ready(&mut self) {
        unsafe {
            while (self.api.is_ready)(self.rec, self.stream) != 0 {
                (self.api.decode)(self.rec, self.stream);
            }
        }
    }

    /// Text recognized so far in the current utterance (may still change), cleaned by
    /// [`text::tidy`].
    pub fn partial(&self) -> String {
        text::tidy(&self.raw_partial())
    }

    /// Same, exactly as the model emitted it.
    pub fn raw_partial(&self) -> String {
        unsafe {
            let r = (self.api.get_result)(self.rec, self.stream);
            if r.is_null() {
                return String::new();
            }
            let s = cstr((*r).text);
            (self.api.destroy_result)(r);
            s.trim().to_string()
        }
    }

    /// Trailing silence long enough to end the utterance (needs `Options::endpoint`).
    pub fn is_endpoint(&self) -> bool {
        unsafe { (self.api.is_endpoint)(self.rec, self.stream) != 0 }
    }

    /// End of input: decode the tail, return the final text and start a fresh utterance.
    pub fn finish(&mut self) -> String {
        if self.tail_padding > 0 {
            self.accept_waveform(&vec![0.0; self.tail_padding]);
        }
        unsafe { (self.api.input_finished)(self.stream) };
        self.decode_ready();
        let text = self.partial();
        unsafe {
            (self.api.destroy_stream)(self.stream);
            self.stream = (self.api.create_stream)(self.rec);
        }
        text
    }

    /// Drop the current utterance without decoding the tail.
    pub fn reset(&mut self) {
        unsafe { (self.api.reset)(self.rec, self.stream) };
    }
}

impl Drop for Recognizer {
    fn drop(&mut self) {
        unsafe {
            if !self.stream.is_null() {
                (self.api.destroy_stream)(self.stream);
            }
            (self.api.destroy_recognizer)(self.rec);
        }
    }
}

unsafe fn cstr(p: *const std::ffi::c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(windows)]
mod loader {
    use crate::ffi::Api;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use windows::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW,
    };
    use windows::core::{PCSTR, PCWSTR};

    static API: OnceLock<Result<Api, String>> = OnceLock::new();

    fn candidates(dir: Option<&Path>) -> Vec<PathBuf> {
        if let Some(d) = dir {
            return vec![d.join(crate::DLL_NAME)];
        }
        let mut v = Vec::new();
        if let Some(d) = std::env::var_os("DIANMO_SHERPA_DIR") {
            v.push(PathBuf::from(d).join(crate::DLL_NAME));
        }
        if let Some(exe_dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
        {
            v.push(exe_dir.join(crate::DLL_NAME));
            v.push(exe_dir.join("sherpa").join(crate::DLL_NAME));
        }
        v
    }

    /// Load the DLL once per process (the first call's `dir` wins).
    pub fn load(dir: Option<&Path>) -> Result<Api, String> {
        API.get_or_init(|| {
            let tried = candidates(dir);
            let dll = tried.iter().find(|p| p.is_file()).ok_or_else(|| {
                format!(
                    "找不到 {}（找过：{}）",
                    crate::DLL_NAME,
                    tried
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join("；")
                )
            })?;
            unsafe { load_from(dll) }
        })
        .clone()
    }

    // The target type of each transmute is the matching `Api` field (signatures from c-api.h).
    #[allow(clippy::missing_transmute_annotations)]
    unsafe fn load_from(dll: &Path) -> Result<Api, String> {
        let wide: Vec<u16> = dll.as_os_str().encode_wide().chain(Some(0)).collect();
        // ALTERED_SEARCH_PATH: onnxruntime.dll resolves next to the C API DLL.
        let m =
            unsafe { LoadLibraryExW(PCWSTR(wide.as_ptr()), None, LOAD_WITH_ALTERED_SEARCH_PATH) }
                .map_err(|e| format!("{}: {e}", dll.display()))?;
        macro_rules! sym {
            ($name:literal) => {{
                let p = unsafe { GetProcAddress(m, PCSTR(concat!($name, "\0").as_ptr())) }
                    .ok_or_else(|| format!("{} 没有导出 {}", crate::DLL_NAME, $name))?;
                unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, _>(p) }
            }};
        }
        Ok(Api {
            version: sym!("SherpaOnnxGetVersionStr"),
            create_recognizer: sym!("SherpaOnnxCreateOnlineRecognizer"),
            destroy_recognizer: sym!("SherpaOnnxDestroyOnlineRecognizer"),
            create_stream: sym!("SherpaOnnxCreateOnlineStream"),
            destroy_stream: sym!("SherpaOnnxDestroyOnlineStream"),
            accept_waveform: sym!("SherpaOnnxOnlineStreamAcceptWaveform"),
            is_ready: sym!("SherpaOnnxIsOnlineStreamReady"),
            decode: sym!("SherpaOnnxDecodeOnlineStream"),
            get_result: sym!("SherpaOnnxGetOnlineStreamResult"),
            destroy_result: sym!("SherpaOnnxDestroyOnlineRecognizerResult"),
            reset: sym!("SherpaOnnxOnlineStreamReset"),
            input_finished: sym!("SherpaOnnxOnlineStreamInputFinished"),
            is_endpoint: sym!("SherpaOnnxOnlineStreamIsEndpoint"),
        })
    }
}

#[cfg(not(windows))]
mod loader {
    use crate::ffi::Api;
    use std::path::Path;
    pub fn load(_dir: Option<&Path>) -> Result<Api, String> {
        Err("dianmo-asr 目前只支持 Windows".into())
    }
}
