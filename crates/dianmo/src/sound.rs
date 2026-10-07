//! 按键音 (TODO #38): a short click on every key press.
//!
//! Four tiny WAVs (`res/sounds/`, synthesized by `gen.py`, 48 kHz mono 16-bit, 4–5 KB each) are
//! compiled into the exe. [`KeySound`] (Windows) owns one audio thread that plays them through
//! WASAPI shared mode, event-driven:
//!
//! - The thread starts when 按键音 is turned on (and at start-up if it is on); it opens the
//!   default output device right away (stream initialized, not started) so the first key press
//!   doesn't pay for that. Turned off, the thread exits and releases the device: nothing of the
//!   audio stack stays loaded or running.
//! - A key press posts a command and sets an event; the thread wakes, mixes the click into the
//!   stream's buffer (up to [`MAX_VOICES`] overlapping clicks, so fast typing never cuts one
//!   off) and starts the stream. While a click plays the thread wakes once per device period
//!   (~10 ms) to top up the buffer; when the buffer has drained it stops the stream and waits on
//!   its command event again (idle CPU 0).
//! - Latency: wake-up (< 1 ms) + the audio engine's period and the device latency, typically
//!   15–25 ms from touch-down.
//! - The default device is checked again before a click that follows a pause (headphones
//!   plugged in / out). Any WASAPI failure drops the stream; the next click opens it again. If
//!   WASAPI can't be used at all, `PlaySound(SND_MEMORY | SND_ASYNC)` from winmm (loaded only
//!   then) plays the clicks: slower and a new click cuts the previous one off, but audible.
//!
//! The format handling, resampling and mixing below are plain Rust and tested on Linux.

use dianmo_ui::KeyClick;
use dianmo_ui::settings::KeySoundStyle;

/// Overlapping clicks mixed at once; a new one beyond this replaces the oldest.
pub const MAX_VOICES: usize = 4;

const CRISP_CHAR: &[u8] = include_bytes!("../res/sounds/crisp_char.wav");
const CRISP_FUNC: &[u8] = include_bytes!("../res/sounds/crisp_func.wav");
const SOFT_CHAR: &[u8] = include_bytes!("../res/sounds/soft_char.wav");
const SOFT_FUNC: &[u8] = include_bytes!("../res/sounds/soft_func.wav");

/// The embedded WAV for a click.
pub fn wav(style: KeySoundStyle, click: KeyClick) -> &'static [u8] {
    match (style, click) {
        (KeySoundStyle::Crisp, KeyClick::Char) => CRISP_CHAR,
        (KeySoundStyle::Crisp, KeyClick::Func) => CRISP_FUNC,
        (KeySoundStyle::Soft, KeyClick::Char) => SOFT_CHAR,
        (KeySoundStyle::Soft, KeyClick::Func) => SOFT_FUNC,
    }
}

/// Index of a click in [`Clips`].
fn slot(click: KeyClick) -> usize {
    match click {
        KeyClick::Char => 0,
        KeyClick::Func => 1,
    }
}

/// Sample rate and samples of a mono 16-bit PCM WAV (the only kind we embed).
pub fn parse_wav(bytes: &[u8]) -> Option<(u32, Vec<i16>)> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let u16_at = |i: usize| bytes.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let (mut rate, mut ok_fmt) = (0, false);
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32_at(i + 4)? as usize;
        let body = i + 8;
        if id == b"fmt " {
            let (tag, channels, bits) = (u16_at(body)?, u16_at(body + 2)?, u16_at(body + 14)?);
            rate = u32_at(body + 4)?;
            ok_fmt = tag == 1 && channels == 1 && bits == 16 && rate > 0;
        } else if id == b"data" && ok_fmt {
            let data = bytes.get(body..body + len)?;
            return Some((rate, data.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect()));
        }
        i = body + len + (len & 1);
    }
    None
}

/// Linear resampling to the device rate, as floats in -1..1.
pub fn resample(samples: &[i16], from: u32, to: u32) -> Vec<f32> {
    let src: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
    if from == to || src.len() < 2 {
        return src;
    }
    let n = ((src.len() as u64 * to as u64) / from as u64).max(1) as usize;
    let step = from as f64 / to as f64;
    (0..n)
        .map(|i| {
            let p = i as f64 * step;
            let j = p as usize;
            let f = (p - j as f64) as f32;
            let a = src[j.min(src.len() - 1)];
            let b = src[(j + 1).min(src.len() - 1)];
            a + (b - a) * f
        })
        .collect()
}

/// The two clicks of a style at the device rate.
pub struct Clips([Vec<f32>; 2]);

impl Clips {
    pub fn render(style: KeySoundStyle, rate: u32) -> Clips {
        let one = |click| parse_wav(wav(style, click)).map(|(r, s)| resample(&s, r, rate)).unwrap_or_default();
        Clips([one(KeyClick::Char), one(KeyClick::Func)])
    }
}

/// Mixes overlapping clicks (mono).
#[derive(Default)]
pub struct Mixer {
    /// (clip slot, position) per playing click, oldest first.
    voices: Vec<(usize, usize)>,
}

impl Mixer {
    pub fn play(&mut self, click: KeyClick) {
        if self.voices.len() >= MAX_VOICES {
            self.voices.remove(0);
        }
        self.voices.push((slot(click), 0));
    }

    pub fn active(&self) -> bool {
        !self.voices.is_empty()
    }

    pub fn clear(&mut self) {
        self.voices.clear();
    }

    /// Frames left until every click has finished.
    pub fn remaining(&self, clips: &Clips) -> usize {
        self.voices.iter().map(|&(c, p)| clips.0[c].len().saturating_sub(p)).max().unwrap_or(0)
    }

    /// Mixes the next `out.len()` frames (times `gain`) into `out` (zeroed first) and advances.
    pub fn fill(&mut self, clips: &Clips, gain: f32, out: &mut [f32]) {
        out.fill(0.0);
        for (c, pos) in &mut self.voices {
            let clip = &clips.0[*c];
            let n = clip.len().saturating_sub(*pos).min(out.len());
            for (o, s) in out[..n].iter_mut().zip(&clip[*pos..*pos + n]) {
                *o += s * gain;
            }
            *pos += n;
        }
        self.voices.retain(|&(c, p)| p < clips.0[c].len());
        for o in out.iter_mut() {
            *o = o.clamp(-1.0, 1.0);
        }
    }
}

/// Sample formats of the shared-mode mix format we can write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormat {
    F32,
    I16,
    /// 32-bit container (24 valid bits or 32).
    I32,
}

impl SampleFormat {
    pub fn bytes(self) -> usize {
        match self {
            SampleFormat::I16 => 2,
            SampleFormat::F32 | SampleFormat::I32 => 4,
        }
    }
}

/// Writes mono frames to an interleaved buffer of `channels` channels (the same sample in
/// each).
pub fn write_frames(dst: &mut [u8], mono: &[f32], channels: usize, fmt: SampleFormat) {
    let b = fmt.bytes();
    for (frame, &v) in dst.chunks_exact_mut(b * channels).zip(mono) {
        for ch in frame.chunks_exact_mut(b) {
            match fmt {
                SampleFormat::F32 => ch.copy_from_slice(&v.to_le_bytes()),
                SampleFormat::I16 => ch.copy_from_slice(&((v * 32767.0) as i16).to_le_bytes()),
                SampleFormat::I32 => ch.copy_from_slice(&((v as f64 * 2147483647.0) as i32).to_le_bytes()),
            }
        }
    }
}

/// A complete mono 16-bit WAV of `wav` with `gain` applied (the `PlaySound` fallback).
pub fn scaled_wav(wav: &[u8], gain: f32) -> Option<Vec<u8>> {
    let (rate, samples) = parse_wav(wav)?;
    let data: Vec<u8> = samples.iter().flat_map(|&s| ((s as f32 * gain) as i16).to_le_bytes()).collect();
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    Some(out)
}

#[cfg(windows)]
pub use imp::KeySound;

#[cfg(windows)]
mod imp {
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use dianmo_ui::KeyClick;
    use dianmo_ui::settings::KeySoundStyle;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Media::Audio::{
        AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, IAudioClient, IAudioRenderClient, IMMDevice,
        IMMDeviceEnumerator, MMDeviceEnumerator, WAVE_FORMAT_PCM, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eConsole, eRender,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
    use windows::Win32::System::Threading::{
        CreateEventW, GetCurrentThread, INFINITE, SetEvent, SetThreadPriority, THREAD_PRIORITY_HIGHEST, WaitForMultipleObjects,
    };
    use windows::core::{GUID, PCWSTR, s, w};

    use super::{Clips, Mixer, SampleFormat, scaled_wav, wav, write_frames};
    use crate::log;

    enum Cmd {
        Play(KeyClick),
        Style(KeySoundStyle),
        Gain(f32),
        Quit,
    }

    /// The event handle, shared with the audio thread.
    #[derive(Clone, Copy)]
    struct Wake(HANDLE);
    // SAFETY: an event handle may be signalled and waited on from any thread.
    unsafe impl Send for Wake {}

    /// The 按键音 player: one audio thread (see the module docs). Dropping it stops the thread and
    /// releases the audio device.
    pub struct KeySound {
        tx: Sender<Cmd>,
        wake: Wake,
        thread: Option<JoinHandle<()>>,
    }

    impl KeySound {
        /// Starts the audio thread (and opens the output device on it). `None` if the thread or
        /// its event can't be created.
        pub fn start(style: KeySoundStyle, gain: f32) -> Option<KeySound> {
            let event = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }.ok()?;
            let wake = Wake(event);
            let (tx, rx) = channel();
            let spawned = std::thread::Builder::new()
                .name("dianmo-sound".into())
                .stack_size(256 * 1024)
                .spawn(move || audio_thread(rx, wake, style, gain));
            match spawned {
                Ok(t) => Some(KeySound { tx, wake, thread: Some(t) }),
                Err(e) => {
                    log!("key sound: no audio thread: {e}");
                    unsafe {
                        let _ = CloseHandle(event);
                    }
                    None
                }
            }
        }

        fn send(&self, cmd: Cmd) {
            if self.tx.send(cmd).is_ok() {
                unsafe {
                    let _ = SetEvent(self.wake.0);
                }
            }
        }

        /// Plays a click (returns at once).
        pub fn play(&self, click: KeyClick) {
            self.send(Cmd::Play(click));
        }

        pub fn set_style(&self, style: KeySoundStyle) {
            self.send(Cmd::Style(style));
        }

        pub fn set_gain(&self, gain: f32) {
            self.send(Cmd::Gain(gain));
        }
    }

    impl Drop for KeySound {
        fn drop(&mut self) {
            self.send(Cmd::Quit);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
            unsafe {
                let _ = CloseHandle(self.wake.0);
            }
        }
    }

    /// Requested buffer: 40 ms (the engine still wakes us every device period).
    const BUFFER_HNS: i64 = 400_000;
    /// After this long without a click, check the default device before the next one.
    const RECHECK_AFTER: Duration = Duration::from_secs(3);
    /// Don't retry opening a failed device more often than this.
    const RETRY_AFTER: Duration = Duration::from_secs(5);

    const SUBTYPE_PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
    const SUBTYPE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
    const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
    const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

    /// A shared-mode render stream on the default device, initialized and stopped.
    struct Stream {
        client: IAudioClient,
        render: IAudioRenderClient,
        event: HANDLE,
        frames: u32,
        /// Audio queued ahead is kept at about this many frames (two device periods): a click
        /// that comes while another plays is mixed in after what is already queued, so less
        /// queued audio = less delay, but at least a period must stay queued.
        lead: u32,
        channels: usize,
        format: SampleFormat,
        rate: u32,
        device_id: String,
        clips: Clips,
        started: bool,
        /// Scratch for one buffer of mono frames.
        mono: Vec<f32>,
    }

    impl Drop for Stream {
        fn drop(&mut self) {
            unsafe {
                if self.started {
                    let _ = self.client.Stop();
                }
                let _ = CloseHandle(self.event);
            }
        }
    }

    fn device_id(device: &IMMDevice) -> String {
        unsafe {
            match device.GetId() {
                Ok(p) => {
                    let id = p.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(p.0 as *const _));
                    id
                }
                Err(_) => String::new(),
            }
        }
    }

    fn default_device(e: &IMMDeviceEnumerator) -> windows::core::Result<IMMDevice> {
        unsafe { e.GetDefaultAudioEndpoint(eRender, eConsole) }
    }

    /// What we can write for a mix format.
    fn sample_format(f: &WAVEFORMATEX) -> Option<SampleFormat> {
        let (tag, bits) = (f.wFormatTag, f.wBitsPerSample);
        let sub = if tag == WAVE_FORMAT_EXTENSIBLE && f.cbSize >= 22 {
            // SAFETY: cbSize says the extensible fields follow.
            let x = unsafe { std::ptr::read_unaligned(f as *const WAVEFORMATEX as *const WAVEFORMATEXTENSIBLE) };
            Some(x.SubFormat)
        } else {
            None
        };
        let float = tag == WAVE_FORMAT_IEEE_FLOAT || sub == Some(SUBTYPE_FLOAT);
        let pcm = tag == WAVE_FORMAT_PCM as u16 || sub == Some(SUBTYPE_PCM);
        match (float, pcm, bits) {
            (true, _, 32) => Some(SampleFormat::F32),
            (_, true, 16) => Some(SampleFormat::I16),
            (_, true, 32) => Some(SampleFormat::I32),
            _ => None,
        }
    }

    impl Stream {
        fn open(e: &IMMDeviceEnumerator, style: KeySoundStyle) -> Result<Stream, String> {
            unsafe {
                let device = default_device(e).map_err(|e| format!("no output device: {e}"))?;
                let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(|e| format!("Activate: {e}"))?;
                let mix = client.GetMixFormat().map_err(|e| format!("GetMixFormat: {e}"))?;
                let f = std::ptr::read_unaligned(mix);
                let fmt = sample_format(&f);
                let (tag, bits) = (f.wFormatTag, f.wBitsPerSample);
                let init = match fmt {
                    Some(_) => client
                        .Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, BUFFER_HNS, 0, mix, None)
                        .map_err(|e| format!("Initialize: {e}")),
                    None => Err(format!("unsupported mix format (tag {tag:#x}, {bits} bits)")),
                };
                CoTaskMemFree(Some(mix as *const _));
                init?;
                let format = fmt.unwrap_or(SampleFormat::F32);
                let event = CreateEventW(None, false, false, PCWSTR::null()).map_err(|e| format!("CreateEvent: {e}"))?;
                let setup = (|| -> windows::core::Result<(IAudioRenderClient, u32, i64)> {
                    client.SetEventHandle(event)?;
                    let mut period = 0i64;
                    client.GetDevicePeriod(Some(&mut period), None)?;
                    Ok((client.GetService()?, client.GetBufferSize()?, period))
                })();
                let (render, frames, period) = match setup {
                    Ok(x) => x,
                    Err(err) => {
                        let _ = CloseHandle(event);
                        return Err(format!("stream setup: {err}"));
                    }
                };
                let rate = f.nSamplesPerSec;
                let period_frames = (rate as i64 * period.max(30_000) / 10_000_000) as u32;
                Ok(Stream {
                    client,
                    render,
                    event,
                    frames,
                    lead: (period_frames * 2).clamp(1, frames),
                    channels: f.nChannels.max(1) as usize,
                    format,
                    rate,
                    device_id: device_id(&device),
                    clips: Clips::render(style, rate),
                    started: false,
                    mono: vec![0.0; frames as usize],
                })
            }
        }

        /// Tops up the buffer from the mixer; starts the stream when there is something to play,
        /// stops it once everything has been played. Returns whether it is running.
        fn pump(&mut self, mixer: &mut Mixer, gain: f32) -> windows::core::Result<bool> {
            unsafe {
                let padding = self.client.GetCurrentPadding()?;
                if mixer.active() {
                    let room = self.lead.saturating_sub(padding) as usize;
                    let n = room.min(mixer.remaining(&self.clips));
                    if n > 0 {
                        let data = self.render.GetBuffer(n as u32)?;
                        let bytes = std::slice::from_raw_parts_mut(data, n * self.channels * self.format.bytes());
                        mixer.fill(&self.clips, gain, &mut self.mono[..n]);
                        write_frames(bytes, &self.mono[..n], self.channels, self.format);
                        self.render.ReleaseBuffer(n as u32, 0)?;
                    }
                    if !self.started {
                        self.client.Start()?;
                        self.started = true;
                    }
                    return Ok(true);
                }
                if self.started && padding == 0 {
                    // Played out: stop and wait for the next click without waking.
                    self.client.Stop()?;
                    self.client.Reset()?;
                    self.started = false;
                }
                Ok(self.started)
            }
        }
    }

    /// `PlaySoundW` from winmm.dll, loaded on first use (WASAPI unavailable).
    struct Fallback {
        play: Option<unsafe extern "system" fn(*const u8, isize, u32) -> i32>,
        /// The WAVs being played must outlive the asynchronous playback.
        cache: Vec<(KeySoundStyle, KeyClick, u32, Vec<u8>)>,
    }

    impl Fallback {
        fn new() -> Fallback {
            let play = unsafe {
                LoadLibraryExW(w!("winmm.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
                    .ok()
                    .and_then(|m| GetProcAddress(m, s!("PlaySoundW")))
                    .map(|f| std::mem::transmute::<_, unsafe extern "system" fn(*const u8, isize, u32) -> i32>(f))
            };
            Fallback { play, cache: Vec::new() }
        }

        fn play(&mut self, style: KeySoundStyle, click: KeyClick, gain: f32) {
            const SND_ASYNC: u32 = 0x1;
            const SND_NODEFAULT: u32 = 0x2;
            const SND_MEMORY: u32 = 0x4;
            let Some(play) = self.play else { return };
            let key = (gain * 1000.0) as u32;
            if !self.cache.iter().any(|(s, c, g, _)| *s == style && *c == click && *g == key)
                && let Some(bytes) = scaled_wav(wav(style, click), gain)
            {
                self.cache.push((style, click, key, bytes));
            }
            if let Some((.., bytes)) = self.cache.iter().find(|(s, c, g, _)| *s == style && *c == click && *g == key) {
                unsafe {
                    play(bytes.as_ptr(), 0, SND_ASYNC | SND_NODEFAULT | SND_MEMORY);
                }
            }
        }
    }

    fn audio_thread(rx: Receiver<Cmd>, wake: Wake, mut style: KeySoundStyle, mut gain: f32) {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
        }
        let enumerator: Option<IMMDeviceEnumerator> = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.ok();
        let mut stream: Option<Stream> = None;
        let mut fallback: Option<Fallback> = None;
        let mut failed_at: Option<Instant> = None;
        let mut last_click = Instant::now();
        let mut mixer = Mixer::default();
        let open = |style, failed_at: &mut Option<Instant>| -> Option<Stream> {
            let e = enumerator.as_ref()?;
            match Stream::open(e, style) {
                Ok(s) => {
                    log!(
                        "key sound: WASAPI {} Hz, {} ch, {:?}, buffer {} frames, lead {} frames",
                        s.rate,
                        s.channels,
                        s.format,
                        s.frames,
                        s.lead
                    );
                    *failed_at = None;
                    Some(s)
                }
                Err(err) => {
                    log!("key sound: WASAPI unavailable ({err})");
                    *failed_at = Some(Instant::now());
                    None
                }
            }
        };
        // Warm up: open the device now so the first click is as fast as the others.
        stream = stream.or_else(|| open(style, &mut failed_at));
        'run: loop {
            let running = stream.as_ref().is_some_and(|s| s.started);
            unsafe {
                match &stream {
                    Some(s) if running => {
                        let _ = WaitForMultipleObjects(&[wake.0, s.event], false, 200);
                    }
                    _ => {
                        let _ = WaitForMultipleObjects(&[wake.0], false, INFINITE);
                    }
                }
            }
            let mut clicks = Vec::new();
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    Cmd::Quit => break 'run,
                    Cmd::Play(c) => clicks.push(c),
                    Cmd::Gain(g) => gain = g.clamp(0.0, 1.0),
                    Cmd::Style(s) => {
                        style = s;
                        mixer.clear();
                        if let Some(st) = &mut stream {
                            st.clips = Clips::render(style, st.rate);
                        }
                    }
                }
            }
            if !clicks.is_empty() {
                let paused = last_click.elapsed() > RECHECK_AFTER;
                last_click = Instant::now();
                // After a pause: still the default device? (headphones plugged in / out)
                if paused
                    && let (Some(st), Some(e)) = (&stream, &enumerator)
                    && !st.started
                    && default_device(e).map(|d| device_id(&d)).ok().as_deref() != Some(st.device_id.as_str())
                {
                    log!("key sound: default output device changed");
                    stream = None;
                }
                if stream.is_none() && failed_at.is_none_or(|t| t.elapsed() > RETRY_AFTER) {
                    stream = open(style, &mut failed_at);
                }
                if stream.is_some() {
                    for c in clicks {
                        mixer.play(c);
                    }
                } else {
                    let fb = fallback.get_or_insert_with(Fallback::new);
                    if let Some(&c) = clicks.last() {
                        fb.play(style, c, gain);
                    }
                }
            }
            if let Some(st) = &mut stream
                && let Err(e) = st.pump(&mut mixer, gain)
            {
                log!("key sound: stream failed ({e}); reopening on the next click");
                stream = None;
                mixer.clear();
                failed_at = None;
            }
        }
        drop(stream);
        drop(enumerator);
        unsafe { CoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_clicks_are_short_mono_pcm() {
        for style in KeySoundStyle::ALL {
            for click in [KeyClick::Char, KeyClick::Func] {
                let bytes = wav(style, click);
                assert!(bytes.len() < 8 * 1024, "{style:?} {click:?}: {} bytes", bytes.len());
                let (rate, samples) = parse_wav(bytes).expect("mono 16-bit PCM");
                assert_eq!(rate, 48_000);
                let ms = samples.len() as u32 * 1000 / rate;
                assert!((20..=80).contains(&ms), "{ms} ms");
                let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
                assert!(peak > 16_000 && peak < 32_000, "normalized below full scale ({peak})");
                assert!(samples.last().unwrap().unsigned_abs() < 200, "fades out (no click at the end)");
            }
        }
        assert_ne!(wav(KeySoundStyle::Crisp, KeyClick::Char), wav(KeySoundStyle::Soft, KeyClick::Char));
        assert!(parse_wav(b"RIFF....WAVEjunk").is_none());
    }

    #[test]
    fn resampling_keeps_duration_and_shape() {
        let s: Vec<i16> = (0..480).map(|i| (i * 60) as i16).collect();
        let r = resample(&s, 48_000, 44_100);
        assert_eq!(r.len(), 441);
        assert!((r[0] - 0.0).abs() < 1e-6);
        assert!(r.windows(2).all(|w| w[1] >= w[0]), "monotonic ramp stays monotonic");
        assert_eq!(resample(&s, 48_000, 48_000).len(), 480);
        assert_eq!(resample(&s, 48_000, 96_000).len(), 960);
    }

    #[test]
    fn mixer_overlaps_and_finishes() {
        let clips = Clips::render(KeySoundStyle::Crisp, 48_000);
        let len = clips.0[0].len();
        let mut m = Mixer::default();
        assert!(!m.active());
        m.play(KeyClick::Char);
        let mut out = vec![0.0; 100];
        m.fill(&clips, 1.0, &mut out);
        let solo = out.clone();
        // A second click 100 frames later overlaps the first.
        m.play(KeyClick::Char);
        assert_eq!(m.remaining(&clips), len);
        let mut out = vec![0.0; len * 2];
        m.fill(&clips, 0.5, &mut out);
        assert!(!m.active(), "both done");
        assert!(out[len..].iter().all(|&v| v == 0.0), "silence after");
        // Gain applies.
        m.play(KeyClick::Char);
        let mut half = vec![0.0; 100];
        m.fill(&clips, 0.5, &mut half);
        assert!(solo.iter().zip(&half).all(|(a, b)| (a * 0.5 - b).abs() < 1e-6));
        // Never more than MAX_VOICES.
        for _ in 0..10 {
            m.play(KeyClick::Func);
        }
        assert_eq!(m.voices.len(), MAX_VOICES);
        m.clear();
        assert!(!m.active());
    }

    #[test]
    fn frames_are_written_in_the_device_format() {
        let mono = [0.5f32, -1.0];
        let mut f = vec![0u8; 2 * 2 * 4];
        write_frames(&mut f, &mono, 2, SampleFormat::F32);
        assert_eq!(f32::from_le_bytes(f[4..8].try_into().unwrap()), 0.5, "same sample on both channels");
        assert_eq!(f32::from_le_bytes(f[8..12].try_into().unwrap()), -1.0);
        let mut i = vec![0u8; 2 * 2];
        write_frames(&mut i, &mono, 1, SampleFormat::I16);
        assert_eq!(i16::from_le_bytes([i[0], i[1]]), 16383);
        assert_eq!(i16::from_le_bytes([i[2], i[3]]), -32767);
        let mut w = vec![0u8; 4];
        write_frames(&mut w, &mono[..1], 1, SampleFormat::I32);
        assert_eq!(i32::from_le_bytes(w[..4].try_into().unwrap()), 1_073_741_823);
    }

    #[test]
    fn fallback_wav_is_scaled() {
        let src = wav(KeySoundStyle::Soft, KeyClick::Func);
        let out = scaled_wav(src, 0.5).unwrap();
        let (rate, a) = parse_wav(src).unwrap();
        let (rate2, b) = parse_wav(&out).unwrap();
        assert_eq!((rate, a.len()), (rate2, b.len()));
        let (pa, pb) = (a.iter().map(|s| s.unsigned_abs()).max().unwrap(), b.iter().map(|s| s.unsigned_abs()).max().unwrap());
        assert!((pb as i32 - pa as i32 / 2).abs() <= 1, "{pa} → {pb}");
    }
}
