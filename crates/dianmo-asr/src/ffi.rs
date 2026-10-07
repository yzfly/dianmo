//! `#[repr(C)]` mirrors of the sherpa-onnx 1.13.8 C API (`sherpa-onnx/c-api/c-api.h`), streaming
//! (online) recognizer part only. Field order and types follow the header exactly; every struct is
//! zero-initialised before use, and the library replaces 0 / NULL with its defaults
//! (`SHERPA_ONNX_OR`), so unused fields stay zero.

#![allow(dead_code)]

use std::ffi::c_char;

pub type CStr = *const c_char;

#[repr(C)]
pub struct OnlineTransducerModelConfig {
    pub encoder: CStr,
    pub decoder: CStr,
    pub joiner: CStr,
}

#[repr(C)]
pub struct OnlineParaformerModelConfig {
    pub encoder: CStr,
    pub decoder: CStr,
}

/// `SherpaOnnxOnlineZipformer2CtcModelConfig` / `…NemoCtc…` / `…ToneCtc…`: a single model path.
#[repr(C)]
pub struct OnlineSingleModelConfig {
    pub model: CStr,
}

#[repr(C)]
pub struct OnlineModelConfig {
    pub transducer: OnlineTransducerModelConfig,
    pub paraformer: OnlineParaformerModelConfig,
    pub zipformer2_ctc: OnlineSingleModelConfig,
    pub tokens: CStr,
    pub num_threads: i32,
    pub provider: CStr,
    pub debug: i32,
    pub model_type: CStr,
    pub modeling_unit: CStr,
    pub bpe_vocab: CStr,
    pub tokens_buf: CStr,
    pub tokens_buf_size: i32,
    pub nemo_ctc: OnlineSingleModelConfig,
    pub t_one_ctc: OnlineSingleModelConfig,
}

#[repr(C)]
pub struct FeatureConfig {
    pub sample_rate: i32,
    pub feature_dim: i32,
}

#[repr(C)]
pub struct OnlineCtcFstDecoderConfig {
    pub graph: CStr,
    pub max_active: i32,
}

#[repr(C)]
pub struct HomophoneReplacerConfig {
    pub dict_dir: CStr,
    pub lexicon: CStr,
    pub rule_fsts: CStr,
}

#[repr(C)]
pub struct OnlineRecognizerConfig {
    pub feat_config: FeatureConfig,
    pub model_config: OnlineModelConfig,
    pub decoding_method: CStr,
    pub max_active_paths: i32,
    pub enable_endpoint: i32,
    pub rule1_min_trailing_silence: f32,
    pub rule2_min_trailing_silence: f32,
    pub rule3_min_utterance_length: f32,
    pub hotwords_file: CStr,
    pub hotwords_score: f32,
    pub ctc_fst_decoder_config: OnlineCtcFstDecoderConfig,
    pub rule_fsts: CStr,
    pub rule_fars: CStr,
    pub blank_penalty: f32,
    pub hotwords_buf: CStr,
    pub hotwords_buf_size: i32,
    pub hr: HomophoneReplacerConfig,
}

#[repr(C)]
pub struct OnlineRecognizerResult {
    pub text: CStr,
    pub tokens: CStr,
    pub tokens_arr: *const CStr,
    pub timestamps: *mut f32,
    pub count: i32,
    pub json: CStr,
}

/// Opaque handles.
#[repr(C)]
pub struct OnlineRecognizer {
    _p: [u8; 0],
}
#[repr(C)]
pub struct OnlineStream {
    _p: [u8; 0],
}

pub type Rec = *const OnlineRecognizer;
pub type Stream = *const OnlineStream;

/// The functions Dianmo calls, resolved by name from `sherpa-onnx-c-api.dll`.
#[derive(Clone, Copy)]
pub struct Api {
    pub version: unsafe extern "C" fn() -> CStr,
    pub create_recognizer: unsafe extern "C" fn(*const OnlineRecognizerConfig) -> Rec,
    pub destroy_recognizer: unsafe extern "C" fn(Rec),
    pub create_stream: unsafe extern "C" fn(Rec) -> Stream,
    pub destroy_stream: unsafe extern "C" fn(Stream),
    pub accept_waveform: unsafe extern "C" fn(Stream, i32, *const f32, i32),
    pub is_ready: unsafe extern "C" fn(Rec, Stream) -> i32,
    pub decode: unsafe extern "C" fn(Rec, Stream),
    pub get_result: unsafe extern "C" fn(Rec, Stream) -> *const OnlineRecognizerResult,
    pub destroy_result: unsafe extern "C" fn(*const OnlineRecognizerResult),
    pub reset: unsafe extern "C" fn(Rec, Stream),
    pub input_finished: unsafe extern "C" fn(Stream),
    pub is_endpoint: unsafe extern "C" fn(Rec, Stream) -> i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    /// Sizes on a 64-bit target, computed by hand from c-api.h (pointer 8, int32/float 4, natural alignment).
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn layout_matches_header() {
        assert_eq!(size_of::<OnlineTransducerModelConfig>(), 24);
        // 24 + 16 + 8 + tokens 8 + num_threads 4(+4) + provider 8 + debug 4(+4) + 3*8 + 8 + 4(+4) + 8 + 8
        assert_eq!(size_of::<OnlineModelConfig>(), 136);
        // 8 + 136 + 8 + 4 + 4 + 3*4(+4) + 8 + 4(+4) + 16 + 8 + 8 + 4(+4) + 8 + 4(+4) + 24 (checked with gcc on c-api.h)
        assert_eq!(size_of::<OnlineRecognizerConfig>(), 272);
        assert_eq!(size_of::<OnlineRecognizerResult>(), 48);
    }
}
