//! Find the model files in a sherpa-onnx model directory.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum ModelKind {
    /// Streaming zipformer transducer: `encoder*.onnx`, `decoder*.onnx`, `joiner*.onnx`.
    Transducer {
        encoder: PathBuf,
        decoder: PathBuf,
        joiner: PathBuf,
    },
    /// Streaming zipformer2 CTC: `model*.onnx` or `ctc*.onnx`.
    Zipformer2Ctc { model: PathBuf },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelFiles {
    pub kind: ModelKind,
    pub tokens: PathBuf,
}

impl ModelFiles {
    pub fn detect(dir: &Path) -> Result<Self, String> {
        let names: Vec<String> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        Self::from_names(dir, &names)
    }

    /// Pure part of [`detect`](Self::detect): pick files by name (int8 preferred).
    pub fn from_names(dir: &Path, names: &[String]) -> Result<Self, String> {
        let pick = |prefix: &str| -> Option<PathBuf> {
            let mut c: Vec<&String> = names
                .iter()
                .filter(|n| n.starts_with(prefix) && n.ends_with(".onnx"))
                .collect();
            // int8 first, then shortest name (e.g. no "-fp16")
            c.sort_by_key(|n| (!n.ends_with(".int8.onnx"), n.contains("fp16"), n.len()));
            c.first().map(|n| dir.join(n))
        };
        if !names.iter().any(|n| n == "tokens.txt") {
            return Err(format!("{} 里没有 tokens.txt", dir.display()));
        }
        let tokens = dir.join("tokens.txt");
        let kind = if let Some(encoder) = pick("encoder") {
            let decoder = pick("decoder").ok_or("缺少 decoder*.onnx")?;
            let joiner = pick("joiner").ok_or("缺少 joiner*.onnx")?;
            ModelKind::Transducer {
                encoder,
                decoder,
                joiner,
            }
        } else if let Some(model) = pick("model").or_else(|| pick("ctc")) {
            ModelKind::Zipformer2Ctc { model }
        } else {
            return Err(format!("{} 里没有认识的 .onnx 模型", dir.display()));
        };
        Ok(ModelFiles { kind, tokens })
    }

    /// All model files (for size accounting).
    pub fn paths(&self) -> Vec<&Path> {
        let mut v: Vec<&Path> = match &self.kind {
            ModelKind::Transducer {
                encoder,
                decoder,
                joiner,
            } => vec![encoder, decoder, joiner],
            ModelKind::Zipformer2Ctc { model } => vec![model],
        };
        v.push(&self.tokens);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn transducer_prefers_int8() {
        let d = Path::new("m");
        let f = ModelFiles::from_names(
            d,
            &n(&[
                "tokens.txt",
                "encoder-epoch-99-avg-1.onnx",
                "encoder-epoch-99-avg-1.int8.onnx",
                "decoder-epoch-99-avg-1.onnx",
                "decoder-epoch-99-avg-1.int8.onnx",
                "joiner-epoch-99-avg-1.int8.onnx",
                "joiner-epoch-99-avg-1.onnx",
            ]),
        )
        .unwrap();
        assert_eq!(
            f.kind,
            ModelKind::Transducer {
                encoder: d.join("encoder-epoch-99-avg-1.int8.onnx"),
                decoder: d.join("decoder-epoch-99-avg-1.int8.onnx"),
                joiner: d.join("joiner-epoch-99-avg-1.int8.onnx"),
            }
        );
    }

    #[test]
    fn transducer_fp32_decoder_only() {
        let d = Path::new("x");
        let f = ModelFiles::from_names(
            d,
            &n(&[
                "tokens.txt",
                "encoder.int8.onnx",
                "decoder.onnx",
                "joiner.int8.onnx",
            ]),
        )
        .unwrap();
        assert!(
            matches!(f.kind, ModelKind::Transducer { ref decoder, .. } if decoder == &d.join("decoder.onnx"))
        );
    }

    #[test]
    fn ctc_models() {
        let d = Path::new("c");
        let f = ModelFiles::from_names(d, &n(&["tokens.txt", "model.int8.onnx", "bbpe.model"]))
            .unwrap();
        assert_eq!(
            f.kind,
            ModelKind::Zipformer2Ctc {
                model: d.join("model.int8.onnx")
            }
        );
        let f = ModelFiles::from_names(
            d,
            &n(&[
                "tokens.txt",
                "ctc-epoch-20-avg-1-chunk-16-left-128.int8.onnx",
            ]),
        )
        .unwrap();
        assert_eq!(
            f.kind,
            ModelKind::Zipformer2Ctc {
                model: d.join("ctc-epoch-20-avg-1-chunk-16-left-128.int8.onnx")
            }
        );
    }

    #[test]
    fn missing_tokens() {
        assert!(ModelFiles::from_names(Path::new("c"), &n(&["model.onnx"])).is_err());
    }
}
