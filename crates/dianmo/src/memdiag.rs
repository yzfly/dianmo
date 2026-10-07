//! Memory breakdown for the log (`DIANMO_MEMLOG=1`, diagnostics only): where the process's
//! private memory sits — each heap's committed / in-use / free bytes, private allocations that
//! belong to no heap (VirtualAlloc'd: graphics drivers, WARP, thread stacks), thread count and
//! the loaded modules. Used to find what a closed settings window leaves behind (v0.2.2).

use std::collections::BTreeSet;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HMODULE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::Memory::{
    GetProcessHeaps, HeapLock, HeapUnlock, HeapWalk, MEM_COMMIT, MEM_PRIVATE, MEMORY_BASIC_INFORMATION,
    PROCESS_HEAP_ENTRY, VirtualQuery,
};
use windows::Win32::System::ProcessStatus::{K32EnumProcessModules, K32GetModuleBaseNameW};
use windows::Win32::System::Threading::GetCurrentProcess;

use crate::log;

const REGION: u16 = 1;
const BUSY: u16 = 4;

pub fn enabled() -> bool {
    std::env::var_os("DIANMO_MEMLOG").is_some()
}

/// Logs the breakdown under `stage` (only with `DIANMO_MEMLOG`).
pub fn report(stage: &str) {
    if !enabled() {
        return;
    }
    crate::app::log_memory(stage);
    // Heaps: committed, busy, free; and every address range a heap uses.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut heaps = vec![HANDLE::default(); 128];
    let n = unsafe { GetProcessHeaps(&mut heaps) } as usize;
    let mut lines = Vec::new();
    for (i, &h) in heaps.iter().take(n.min(128)).enumerate() {
        let (mut committed, mut busy, mut free, mut big) = (0usize, 0usize, 0usize, 0usize);
        if unsafe { HeapLock(h) }.is_err() {
            continue;
        }
        let mut e = PROCESS_HEAP_ENTRY::default();
        while unsafe { HeapWalk(h, &mut e) }.is_ok() {
            if e.wFlags & REGION != 0 {
                let r = unsafe { e.Anonymous.Region };
                committed += r.dwCommittedSize as usize;
                ranges.push((e.lpData as usize, r.dwCommittedSize as usize + r.dwUnCommittedSize as usize));
            } else if e.wFlags & BUSY != 0 {
                busy += e.cbData as usize;
                let a = e.lpData as usize;
                if !ranges.iter().any(|&(s, l)| a >= s && a < s + l) {
                    // A large block in its own VirtualAlloc'd range.
                    big += e.cbData as usize;
                    ranges.push((a, e.cbData as usize));
                }
            } else if e.wFlags & 2 == 0 {
                free += e.cbData as usize;
            }
        }
        let _ = unsafe { HeapUnlock(h) };
        if committed + big > 256 * 1024 {
            lines.push(format!(
                "heap {i} {:?}: committed {:.1} MB (+{:.1} MB large blocks), busy {:.1} MB, free {:.1} MB",
                h.0,
                mb(committed),
                mb(big),
                mb(busy),
                mb(free)
            ));
        }
    }
    // Private committed allocations (by allocation base), those outside every heap range.
    let mut addr = 0usize;
    let mut total = 0usize;
    let mut allocs: Vec<(usize, usize, u32)> = Vec::new();
    loop {
        let mut m = MEMORY_BASIC_INFORMATION::default();
        if unsafe { VirtualQuery(Some(addr as *const _), &mut m, size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 {
            break;
        }
        if m.State == MEM_COMMIT && m.Type == MEM_PRIVATE {
            total += m.RegionSize;
            let base = m.AllocationBase as usize;
            match allocs.last_mut() {
                Some(last) if last.0 == base => last.1 += m.RegionSize,
                _ => allocs.push((base, m.RegionSize, m.Protect.0)),
            }
        }
        let next = m.BaseAddress as usize + m.RegionSize;
        if next <= addr {
            break;
        }
        addr = next;
    }
    let in_heap = |base: usize, len: usize| ranges.iter().any(|&(s, l)| s < base + len && base < s + l);
    let mut outside: Vec<_> = allocs.iter().filter(|a| !in_heap(a.0, a.1)).collect();
    outside.sort_by_key(|a| std::cmp::Reverse(a.1));
    let out_total: usize = outside.iter().map(|a| a.1).sum();
    log!(
        "memdiag ({stage}): private committed {:.1} MB in {} allocations; outside heaps {:.1} MB in {}; threads {}",
        mb(total),
        allocs.len(),
        mb(out_total),
        outside.len(),
        thread_count()
    );
    for l in lines {
        log!("memdiag   {l}");
    }
    for a in outside.iter().take(12) {
        log!("memdiag   alloc 0x{:012X} {:>8.1} KB prot 0x{:X}", a.0, a.1 as f64 / 1024.0, a.2);
    }
    // Modules, as a diff against the previous report.
    static LAST: std::sync::Mutex<Option<BTreeSet<String>>> = std::sync::Mutex::new(None);
    let now = modules();
    if let Ok(mut last) = LAST.lock() {
        match last.as_ref() {
            None => log!("memdiag   modules ({}): {}", now.len(), now.iter().cloned().collect::<Vec<_>>().join(" ")),
            Some(prev) => {
                let added: Vec<_> = now.difference(prev).cloned().collect();
                let gone: Vec<_> = prev.difference(&now).cloned().collect();
                log!("memdiag   modules +[{}] -[{}]", added.join(" "), gone.join(" "));
            }
        }
        *last = Some(now);
    }
}

fn mb(b: usize) -> f64 {
    b as f64 / 1048576.0
}

fn thread_count() -> usize {
    let pid = std::process::id();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else { return 0 };
        let mut te = THREADENTRY32 { dwSize: size_of::<THREADENTRY32>() as u32, ..Default::default() };
        let mut n = 0;
        let mut ok = Thread32First(snap, &mut te).is_ok();
        while ok {
            if te.th32OwnerProcessID == pid {
                n += 1;
            }
            ok = Thread32Next(snap, &mut te).is_ok();
        }
        let _ = CloseHandle(snap);
        n
    }
}

fn modules() -> BTreeSet<String> {
    let mut mods = vec![HMODULE::default(); 512];
    let mut needed = 0u32;
    let p = unsafe { GetCurrentProcess() };
    let ok = unsafe { K32EnumProcessModules(p, mods.as_mut_ptr(), (mods.len() * size_of::<HMODULE>()) as u32, &mut needed) };
    if !ok.as_bool() {
        return BTreeSet::new();
    }
    let n = (needed as usize / size_of::<HMODULE>()).min(mods.len());
    mods[..n]
        .iter()
        .filter_map(|&m| {
            let mut buf = [0u16; 260];
            let len = unsafe { K32GetModuleBaseNameW(p, Some(m), &mut buf) } as usize;
            (len > 0).then(|| String::from_utf16_lossy(&buf[..len]).to_lowercase())
        })
        .collect()
}
