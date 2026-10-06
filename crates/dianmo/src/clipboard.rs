//! Clipboard history (TODO #32, DESIGN.md §2「选择、复制与剪贴板」).
//!
//! - [`ClipStore`]: the history (most recent first): at most [`MAX_RECENT`] unpinned entries in
//!   memory; pinned entries are kept in `clips.txt` in the data directory (`%APPDATA%\Dianmo`, or
//!   `Dianmo-<name>` with `--instance`).
//! - [`start`] (Windows): a background thread with a message-only window registered with
//!   `AddClipboardFormatListener`. Each `WM_CLIPBOARDUPDATE` reads `CF_UNICODETEXT` and posts a
//!   [`ClipEvent`] to the UI thread; nothing runs between events (idle CPU stays 0). Content
//!   marked private by password managers (`ExcludeClipboardContentFromMonitorProcessing`,
//!   `CanIncludeInClipboardHistory = 0`, `CanUploadToCloudClipboard = 0`) is not reported, and
//!   neither are the clipboard changes 点墨 makes itself ([`ClipboardWatcher::paste`]).

use std::path::Path;

use dianmo_ui::ClipItem;

/// Unpinned entries kept in memory.
pub const MAX_RECENT: usize = 30;
/// Pinned entries kept (on disk too).
pub const MAX_PINNED: usize = 50;
/// Longer clipboard text is not recorded (memory).
pub const MAX_CLIP_CHARS: usize = 100_000;
/// Longer text is pasted through the clipboard (Ctrl+V) instead of being typed.
pub const TYPE_MAX_CHARS: usize = 2000;

#[derive(Debug, Default)]
pub struct ClipStore {
    items: Vec<ClipItem>,
    next_id: u64,
}

impl ClipStore {
    /// A store holding `pinned` (from `clips.txt`).
    pub fn with_pinned(pinned: Vec<String>) -> Self {
        let mut s = Self::default();
        for text in pinned.into_iter().take(MAX_PINNED) {
            if !text.is_empty() && !s.items.iter().any(|c| c.text == text) {
                s.next_id += 1;
                s.items.push(ClipItem { id: s.next_id, text, pinned: true });
            }
        }
        s
    }

    pub fn items(&self) -> &[ClipItem] {
        &self.items
    }

    /// Records copied text as the most recent entry (an existing equal entry moves to the front).
    /// False if it is not worth keeping (blank, too long).
    pub fn add(&mut self, text: String) -> bool {
        if text.trim().is_empty() || text.chars().count() > MAX_CLIP_CHARS {
            return false;
        }
        let item = match self.items.iter().position(|c| c.text == text) {
            Some(i) => self.items.remove(i),
            None => {
                self.next_id += 1;
                ClipItem { id: self.next_id, text, pinned: false }
            }
        };
        self.items.insert(0, item);
        let mut recent = 0;
        self.items.retain(|c| {
            if c.pinned {
                return true;
            }
            recent += 1;
            recent <= MAX_RECENT
        });
        true
    }

    /// Pins or unpins an entry. True if the pinned set changed (save it).
    pub fn pin(&mut self, id: u64, pinned: bool) -> bool {
        let count = self.items.iter().filter(|c| c.pinned).count();
        match self.items.iter_mut().find(|c| c.id == id) {
            Some(c) if c.pinned != pinned && (!pinned || count < MAX_PINNED) => {
                c.pinned = pinned;
                true
            }
            _ => false,
        }
    }

    /// Removes an entry. True if it was pinned (save).
    pub fn delete(&mut self, id: u64) -> bool {
        match self.items.iter().position(|c| c.id == id) {
            Some(i) => self.items.remove(i).pinned,
            None => false,
        }
    }

    /// Removes every unpinned entry.
    pub fn clear_unpinned(&mut self) {
        self.items.retain(|c| c.pinned);
    }

    /// `clips.txt`: one pinned entry per line, `\` `\n` `\r` escaped.
    pub fn serialize_pinned(&self) -> String {
        let mut out = String::new();
        for c in self.items.iter().filter(|c| c.pinned) {
            for ch in c.text.chars() {
                match ch {
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    _ => out.push(ch),
                }
            }
            out.push('\n');
        }
        out
    }

    pub fn parse_pinned(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in text.trim_start_matches('\u{feff}').lines() {
            let mut s = String::with_capacity(line.len());
            let mut it = line.chars();
            while let Some(c) = it.next() {
                if c != '\\' {
                    s.push(c);
                    continue;
                }
                match it.next() {
                    Some('n') => s.push('\n'),
                    Some('r') => s.push('\r'),
                    Some(other) => s.push(other),
                    None => s.push('\\'),
                }
            }
            if !s.is_empty() {
                out.push(s);
            }
        }
        out
    }

    /// Pinned entries from `path` (missing or unreadable → none).
    pub fn load(path: &Path) -> Self {
        let pinned = std::fs::read_to_string(path).map(|t| Self::parse_pinned(&t)).unwrap_or_default();
        Self::with_pinned(pinned)
    }

    /// Writes the pinned entries (temp file + rename).
    pub fn save_pinned(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("txt.tmp");
        std::fs::write(&tmp, self.serialize_pinned())?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(windows)]
pub use win::{ClipEvent, ClipboardWatcher, start};

#[cfg(windows)]
mod win {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread::JoinHandle;
    use std::time::Duration;

    use dianmo_core::KeyChord;
    use dianmo_win::HostProxy;
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::DataExchange::{
        AddClipboardFormatListener, CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
        GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
        RemoveClipboardFormatListener, SetClipboardData,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG,
        PostMessageW, PostQuitMessage, RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLIPBOARDUPDATE,
        WM_CLOSE, WM_DESTROY, WNDCLASSW,
    };
    use windows::core::w;

    use crate::log;

    const CF_UNICODETEXT: u32 = 13;
    /// Posted by [`ClipboardWatcher::paste`]: paste the queued text.
    const WM_APP_PASTE: u32 = WM_APP + 21;
    /// How long the target app gets to read the clipboard after Ctrl+V before it is restored.
    const PASTE_SETTLE_MS: u64 = 600;
    /// Clipboard formats bigger than this (all together) are not saved / restored.
    const MAX_SAVE_BYTES: usize = 64 << 20;

    /// Posted to the UI thread (`App::on_event`).
    #[derive(Debug)]
    pub enum ClipEvent {
        /// New clipboard text (not private, not ours).
        Text(String),
        /// The clipboard changed to something without text (an image, files, emptied).
        NoText,
        /// The clipboard holds private content (password manager): not recorded.
        Private,
        /// [`ClipboardWatcher::paste`] could not use the clipboard; type the text instead.
        PasteFailed(String),
    }

    /// The running watcher thread. Dropping it closes the window and joins the thread.
    pub struct ClipboardWatcher {
        hwnd: isize,
        queue: Arc<Mutex<VecDeque<String>>>,
        join: Option<JoinHandle<()>>,
    }

    impl ClipboardWatcher {
        /// Pastes long text: saves the clipboard, puts `text` on it, sends Ctrl+V, and restores the
        /// original contents after the app has had time to read it (on the watcher thread; the UI
        /// stays responsive). Our own clipboard changes are not recorded.
        pub fn paste(&self, text: String) {
            if let Ok(mut q) = self.queue.lock() {
                q.push_back(text);
            }
            unsafe {
                let _ = PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_APP_PASTE, WPARAM(0), LPARAM(0));
            }
        }
    }

    impl Drop for ClipboardWatcher {
        fn drop(&mut self) {
            unsafe {
                let _ = PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    struct Formats {
        exclude: u32,
        history: u32,
        cloud: u32,
    }

    thread_local! {
        static PROXY: Cell<Option<HostProxy>> = const { Cell::new(None) };
        static QUEUE: RefCell<Option<Arc<Mutex<VecDeque<String>>>>> = const { RefCell::new(None) };
        /// Clipboard sequence numbers caused by our own paste (inclusive range).
        static IGNORE: Cell<(u32, u32)> = const { Cell::new((1, 0)) };
        static FORMATS: Cell<(u32, u32, u32)> = const { Cell::new((0, 0, 0)) };
    }

    /// Starts the watcher thread; events go to `proxy`.
    pub fn start(proxy: HostProxy) -> Result<ClipboardWatcher, String> {
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let q2 = queue.clone();
        let (tx, rx) = mpsc::channel::<Result<isize, String>>();
        let join = std::thread::Builder::new()
            .name("dianmo-clipboard".into())
            .stack_size(256 * 1024)
            .spawn(move || run(proxy, q2, tx))
            .map_err(|e| format!("spawn: {e}"))?;
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(hwnd)) => Ok(ClipboardWatcher { hwnd, queue, join: Some(join) }),
            Ok(Err(e)) => {
                let _ = join.join();
                Err(e)
            }
            Err(_) => Err("clipboard thread did not start".into()),
        }
    }

    fn run(proxy: HostProxy, queue: Arc<Mutex<VecDeque<String>>>, tx: mpsc::Sender<Result<isize, String>>) {
        PROXY.with(|p| p.set(Some(proxy)));
        QUEUE.with(|q| *q.borrow_mut() = Some(queue));
        let formats = unsafe {
            (
                RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing")),
                RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")),
                RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")),
            )
        };
        FORMATS.with(|f| f.set(formats));
        let hwnd = unsafe {
            let instance = match GetModuleHandleW(None) {
                Ok(h) => h.into(),
                Err(e) => {
                    let _ = tx.send(Err(format!("GetModuleHandle: {e}")));
                    return;
                }
            };
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                lpszClassName: w!("DianmoClipboard"),
                ..Default::default()
            };
            RegisterClassW(&class);
            match CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("DianmoClipboard"),
                w!("点墨剪贴板"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                None,
            ) {
                Ok(h) => h,
                Err(e) => {
                    let _ = tx.send(Err(format!("CreateWindow: {e}")));
                    return;
                }
            }
        };
        if let Err(e) = unsafe { AddClipboardFormatListener(hwnd) } {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            let _ = tx.send(Err(format!("AddClipboardFormatListener: {e}")));
            return;
        }
        let _ = tx.send(Ok(hwnd.0 as isize));
        let mut msg = MSG::default();
        unsafe {
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
        }
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_CLIPBOARDUPDATE => {
                on_update(hwnd);
                LRESULT(0)
            }
            WM_APP_PASTE => {
                let next = QUEUE.with(|q| q.borrow().as_ref().and_then(|q| q.lock().ok().and_then(|mut q| q.pop_front())));
                if let Some(text) = next {
                    paste_now(hwnd, text);
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                unsafe {
                    let _ = RemoveClipboardFormatListener(hwnd);
                    PostQuitMessage(0);
                }
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        }
    }

    fn post(ev: ClipEvent) {
        if let Some(p) = PROXY.with(|p| p.get()) {
            p.post(ev);
        }
    }

    /// Opens the clipboard, retrying briefly while another app holds it.
    fn open(hwnd: HWND) -> bool {
        for i in 0..8 {
            if unsafe { OpenClipboard(Some(hwnd)) }.is_ok() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10 + 10 * i));
        }
        false
    }

    fn on_update(hwnd: HWND) {
        let seq = unsafe { GetClipboardSequenceNumber() };
        let (from, to) = IGNORE.with(|c| c.get());
        if (from..=to).contains(&seq) {
            return;
        }
        let (exclude, history, cloud) = FORMATS.with(|f| f.get());
        let formats = Formats { exclude, history, cloud };
        if formats.exclude != 0 && unsafe { IsClipboardFormatAvailable(formats.exclude) }.is_ok() {
            post(ClipEvent::Private);
            return;
        }
        if !open(hwnd) {
            log!("clipboard busy; change not recorded");
            return;
        }
        let ev = read_open(&formats);
        unsafe {
            let _ = CloseClipboard();
        }
        post(ev);
    }

    /// With the clipboard open: the text, unless it is marked private.
    fn read_open(f: &Formats) -> ClipEvent {
        for fmt in [f.history, f.cloud] {
            if fmt != 0 && read_dword(fmt) == Some(0) {
                return ClipEvent::Private;
            }
        }
        match read_text() {
            Some(t) => ClipEvent::Text(t),
            None => ClipEvent::NoText,
        }
    }

    fn with_global<R>(fmt: u32, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
        unsafe {
            if IsClipboardFormatAvailable(fmt).is_err() {
                return None;
            }
            let h = GetClipboardData(fmt).ok()?;
            let g = HGLOBAL(h.0);
            let size = GlobalSize(g);
            let p = GlobalLock(g);
            if p.is_null() {
                return None;
            }
            let r = f(std::slice::from_raw_parts(p as *const u8, size));
            let _ = GlobalUnlock(g);
            Some(r)
        }
    }

    fn read_dword(fmt: u32) -> Option<u32> {
        with_global(fmt, |b| (b.len() >= 4).then(|| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))).flatten()
    }

    fn read_text() -> Option<String> {
        with_global(CF_UNICODETEXT, |b| {
            let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
            String::from_utf16_lossy(&units)
        })
    }

    /// Clipboard formats whose data is not a plain memory block (GDI handles, owner display,
    /// private handles): not saved.
    fn skip_format(f: u32) -> bool {
        matches!(f, 2 | 3 | 9 | 14 | 0x80 | 0x82 | 0x83 | 0x8E) || (0x200..=0x3FF).contains(&f)
    }

    fn save_all() -> Vec<(u32, Vec<u8>)> {
        let mut out = Vec::new();
        let mut total = 0usize;
        let mut f = 0;
        loop {
            f = unsafe { EnumClipboardFormats(f) };
            if f == 0 {
                break;
            }
            if skip_format(f) {
                continue;
            }
            if let Some(Some(data)) = with_global(f, |b| (total + b.len() <= MAX_SAVE_BYTES).then(|| b.to_vec())) {
                total += data.len();
                out.push((f, data));
            }
        }
        out
    }

    fn set_bytes(fmt: u32, data: &[u8]) -> bool {
        unsafe {
            let Ok(g) = GlobalAlloc(GMEM_MOVEABLE, data.len().max(1)) else { return false };
            let p = GlobalLock(g);
            if p.is_null() {
                let _ = GlobalFree(Some(g));
                return false;
            }
            std::ptr::copy_nonoverlapping(data.as_ptr(), p as *mut u8, data.len());
            let _ = GlobalUnlock(g);
            if SetClipboardData(fmt, Some(HANDLE(g.0))).is_err() {
                let _ = GlobalFree(Some(g));
                return false;
            }
            true
        }
    }

    fn set_text(text: &str) -> bool {
        let mut bytes: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        bytes.shrink_to_fit();
        set_bytes(CF_UNICODETEXT, &bytes)
    }

    /// Clipboard → text, Ctrl+V, wait, restore. Runs on the watcher thread.
    fn paste_now(hwnd: HWND, text: String) {
        let before = unsafe { GetClipboardSequenceNumber() };
        IGNORE.with(|c| c.set((before.wrapping_add(1), u32::MAX)));
        if !open(hwnd) {
            IGNORE.with(|c| c.set((1, 0)));
            post(ClipEvent::PasteFailed(text));
            return;
        }
        let saved = save_all();
        let ok = unsafe { EmptyClipboard() }.is_ok() && set_text(&text);
        unsafe {
            let _ = CloseClipboard();
        }
        if !ok {
            IGNORE.with(|c| c.set((1, 0)));
            post(ClipEvent::PasteFailed(text));
            return;
        }
        dianmo_win::send_chord(KeyChord::PASTE);
        std::thread::sleep(Duration::from_millis(PASTE_SETTLE_MS));
        if open(hwnd) {
            unsafe {
                let _ = EmptyClipboard();
            }
            for (f, data) in &saved {
                set_bytes(*f, data);
            }
            unsafe {
                let _ = CloseClipboard();
            }
        } else {
            log!("clipboard busy; could not restore it after pasting");
        }
        let after = unsafe { GetClipboardSequenceNumber() };
        IGNORE.with(|c| c.set((before.wrapping_add(1), after)));
        log!("pasted {} chars through the clipboard ({} formats restored)", text.chars().count(), saved.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_first_dedup_and_cap() {
        let mut s = ClipStore::default();
        assert!(s.add("a".into()));
        assert!(s.add("b".into()));
        assert!(s.add("a".into()));
        let texts: Vec<&str> = s.items().iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["a", "b"]);
        assert!(!s.add("  \n".into()));
        assert!(!s.add("x".repeat(MAX_CLIP_CHARS + 1)));
        let id_b = s.items()[1].id;
        assert!(s.pin(id_b, true));
        assert!(!s.pin(id_b, true));
        for i in 0..40 {
            s.add(format!("n{i}"));
        }
        assert_eq!(s.items().iter().filter(|c| !c.pinned).count(), MAX_RECENT);
        assert!(s.items().iter().any(|c| c.text == "b" && c.pinned), "pinned entries survive the cap");
        assert_eq!(s.items()[0].text, "n39");
        s.clear_unpinned();
        assert_eq!(s.items().len(), 1);
        assert!(s.delete(id_b));
        assert!(s.items().is_empty());
    }

    #[test]
    fn pinned_file_roundtrip() {
        let mut s = ClipStore::default();
        s.add("第一行\r\n第二行 \\ 反斜杠".into());
        s.add("plain".into());
        s.add("unpinned".into());
        let ids: Vec<u64> = s.items().iter().map(|c| c.id).collect();
        s.pin(ids[1], true);
        s.pin(ids[2], true);
        let text = s.serialize_pinned();
        assert_eq!(text.lines().count(), 2);
        let back = ClipStore::with_pinned(ClipStore::parse_pinned(&text));
        let texts: Vec<&str> = back.items().iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["plain", "第一行\r\n第二行 \\ 反斜杠"]);
        assert!(back.items().iter().all(|c| c.pinned));

        let dir = std::env::temp_dir().join(format!("dianmo-clips-test-{}", std::process::id()));
        let path = dir.join("clips.txt");
        back.save_pinned(&path).unwrap();
        assert_eq!(ClipStore::load(&path).items(), back.items());
        assert!(ClipStore::load(&dir.join("missing.txt")).items().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
