//! Minimal WAV reader for tests and the command-line example: PCM 8/16/24/32-bit or IEEE float,
//! any sample rate; multi-channel input keeps channel 0.

pub struct Wav {
    pub sample_rate: u32,
    /// Channel 0, scaled to -1.0..=1.0.
    pub samples: Vec<f32>,
}

impl Wav {
    pub fn duration(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64
    }
}

pub fn read(path: &std::path::Path) -> Result<Wav, String> {
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&b).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn parse(b: &[u8]) -> Result<Wav, String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("不是 RIFF/WAVE 文件".into());
    }
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut pos = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None; // format, channels, rate, bits
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let len = u32_at(pos + 4) as usize;
        let body = pos + 8;
        let end = (body + len).min(b.len());
        if id == b"fmt " && len >= 16 {
            let mut format = u16_at(body);
            if format == 0xFFFE && len >= 26 {
                format = u16_at(body + 24); // WAVE_FORMAT_EXTENSIBLE: first 2 bytes of the sub-format GUID
            }
            fmt = Some((
                format,
                u16_at(body + 2),
                u32_at(body + 4),
                u16_at(body + 14),
            ));
        } else if id == b"data" {
            let (format, channels, rate, bits) = fmt.ok_or("data 块在 fmt 块之前")?;
            let data = &b[body..end];
            let width = (bits as usize).div_ceil(8);
            let frame = width * channels.max(1) as usize;
            if frame == 0 {
                return Err("无效的 fmt".into());
            }
            let samples =
                data.chunks_exact(frame)
                    .map(|f| {
                        let s = &f[..width];
                        match (format, bits) {
                            (1, 8) => Ok((s[0] as f32 - 128.0) / 128.0),
                            (1, 16) => Ok(i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0),
                            (1, 24) => Ok((i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32
                                / 8_388_608.0),
                            (1, 32) => Ok(i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32
                                / 2_147_483_648.0),
                            (3, 32) => Ok(f32::from_le_bytes([s[0], s[1], s[2], s[3]])),
                            _ => Err(format!("不支持的 WAV 格式 {format}/{bits} 位")),
                        }
                    })
                    .collect::<Result<Vec<f32>, String>>()?;
            return Ok(Wav {
                sample_rate: rate,
                samples,
            });
        }
        pos = body + len + (len & 1);
    }
    Err("没有 data 块".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav16(rate: u32, channels: u16, pcm: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut v = b"RIFF".to_vec();
        v.extend((36 + data.len() as u32).to_le_bytes());
        v.extend(b"WAVEfmt ");
        v.extend(16u32.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(channels.to_le_bytes());
        v.extend(rate.to_le_bytes());
        v.extend((rate * 2 * channels as u32).to_le_bytes());
        v.extend((2 * channels).to_le_bytes());
        v.extend(16u16.to_le_bytes());
        v.extend(b"LIST");
        v.extend(3u32.to_le_bytes());
        v.extend([1, 2, 3, 0]); // odd-sized chunk + pad byte
        v.extend(b"data");
        v.extend((data.len() as u32).to_le_bytes());
        v.extend(data);
        v
    }

    #[test]
    fn mono_16bit() {
        let w = parse(&wav16(16000, 1, &[0, 16384, -32768])).unwrap();
        assert_eq!(w.sample_rate, 16000);
        assert_eq!(w.samples, vec![0.0, 0.5, -1.0]);
    }

    #[test]
    fn stereo_keeps_left() {
        let w = parse(&wav16(8000, 2, &[16384, 1, -16384, 2])).unwrap();
        assert_eq!(w.samples, vec![0.5, -0.5]);
        assert!((w.duration() - 2.0 / 8000.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(b"hello").is_err());
    }
}
