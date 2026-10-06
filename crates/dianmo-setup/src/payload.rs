//! Payload format (see the crate docs): writing (packer) and reading/extracting (installer).

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const TRAILER_MAGIC: &[u8; 8] = b"DMSETUP1";
pub const TRAILER_LEN: u64 = 32;
const HEADER_MAGIC: &[u8; 4] = b"DMPL";
/// Sanity limit for one file (the biggest Rime dictionary is ~20 MB).
const MAX_FILE: u64 = 512 << 20;

fn bad(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

/// A relative path from the payload, checked: no absolute paths, drive letters, `..`, empty
/// components or backslashes (so extraction can't escape the target directory).
pub fn safe_relative(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.len() > 1024 {
        return None;
    }
    let mut out = PathBuf::new();
    for part in path.split('/') {
        let ok = !part.is_empty()
            && part != "."
            && part != ".."
            && !part.contains(['\\', ':', '\0'])
            && !part.ends_with([' ', '.']);
        if !ok {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------------------------

/// One file to pack: payload path ('/'-separated) and its compressed data.
pub struct Packed {
    pub path: String,
    pub size: u64,
    pub zlib: Vec<u8>,
}

/// Compresses `data` (zlib, level 9).
pub fn pack(path: String, data: &[u8]) -> Packed {
    let zlib = miniz_oxide::deflate::compress_to_vec_zlib(data, 9);
    Packed { path, size: data.len() as u64, zlib }
}

/// All files under `dir` (recursive), as (payload path, absolute path), sorted by payload path.
pub fn list_dir(dir: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                walk(base, &path, out)?;
            } else {
                let rel = path.strip_prefix(base).map_err(|_| bad("path outside base"))?;
                let parts: Vec<String> =
                    rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                let name = parts.join("/");
                if safe_relative(&name).is_none() {
                    return Err(bad(format!("unsupported file name {name:?}")));
                }
                out.push((name, path));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

/// Appends payload + trailer to `out` (positioned at the end of the stub, at `offset`).
pub fn write_payload(out: &mut impl Write, offset: u64, version: &str, files: &[Packed]) -> io::Result<u64> {
    let mut len = 0u64;
    let mut put = |out: &mut dyn Write, bytes: &[u8]| -> io::Result<()> {
        out.write_all(bytes)?;
        len += bytes.len() as u64;
        Ok(())
    };
    put(out, HEADER_MAGIC)?;
    put(out, &(files.len() as u32).to_le_bytes())?;
    put(out, &(version.len() as u16).to_le_bytes())?;
    put(out, version.as_bytes())?;
    let mut total = 0u64;
    for f in files {
        put(out, &(f.path.len() as u16).to_le_bytes())?;
        put(out, f.path.as_bytes())?;
        put(out, &f.size.to_le_bytes())?;
        put(out, &(f.zlib.len() as u64).to_le_bytes())?;
        put(out, &f.zlib)?;
        total += f.size;
    }
    out.write_all(TRAILER_MAGIC)?;
    out.write_all(&offset.to_le_bytes())?;
    out.write_all(&len.to_le_bytes())?;
    out.write_all(&total.to_le_bytes())?;
    Ok(len)
}

// ---------------------------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------------------------

/// Where the payload is inside the installer exe.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub offset: u64,
    pub len: u64,
    /// Sum of the unpacked file sizes.
    pub total: u64,
    pub count: u32,
    pub version: String,
}

fn read_u16(r: &mut impl Read) -> io::Result<u16> {
    let mut b = [0; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}

fn read_u32(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(r: &mut impl Read) -> io::Result<u64> {
    let mut b = [0; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_string(r: &mut impl Read, len: usize) -> io::Result<String> {
    let mut b = vec![0; len];
    r.read_exact(&mut b)?;
    String::from_utf8(b).map_err(|_| bad("name is not UTF-8"))
}

/// Finds the payload of the installer `file` (reads the trailer and the header).
pub fn info(file: &mut (impl Read + Seek)) -> io::Result<Info> {
    let size = file.seek(SeekFrom::End(0))?;
    if size < TRAILER_LEN {
        return Err(bad("no payload"));
    }
    file.seek(SeekFrom::Start(size - TRAILER_LEN))?;
    let mut magic = [0; 8];
    file.read_exact(&mut magic)?;
    if &magic != TRAILER_MAGIC {
        return Err(bad("no payload"));
    }
    let (offset, len, total) = (read_u64(file)?, read_u64(file)?, read_u64(file)?);
    if offset.checked_add(len) != Some(size - TRAILER_LEN) {
        return Err(bad("payload truncated"));
    }
    file.seek(SeekFrom::Start(offset))?;
    let mut hm = [0; 4];
    file.read_exact(&mut hm)?;
    if &hm != HEADER_MAGIC {
        return Err(bad("bad payload header"));
    }
    let count = read_u32(file)?;
    let vlen = read_u16(file)? as usize;
    let version = read_string(file, vlen)?;
    Ok(Info { offset, len, total, count, version })
}

/// Extracts every file into `dest` (created if missing). `progress(done, total)` is called after
/// each file with unpacked byte counts.
pub fn extract(file: &mut (impl Read + Seek), dest: &Path, mut progress: impl FnMut(u64, u64)) -> io::Result<Info> {
    let info = info(file)?;
    let end = info.offset + info.len;
    let mut r = BufReader::with_capacity(1 << 16, &mut *file);
    // Positioned after the header by `info`.
    let mut done = 0u64;
    std::fs::create_dir_all(dest)?;
    for _ in 0..info.count {
        let plen = read_u16(&mut r)? as usize;
        let name = read_string(&mut r, plen)?;
        let rel = safe_relative(&name).ok_or_else(|| bad(format!("unsafe path {name:?}")))?;
        let size = read_u64(&mut r)?;
        let packed = read_u64(&mut r)?;
        if size > MAX_FILE || packed > MAX_FILE.max(size + 1024) {
            return Err(bad(format!("{name}: implausible size")));
        }
        let mut zlib = vec![0; packed as usize];
        r.read_exact(&mut zlib)?;
        let data = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&zlib, size as usize)
            .map_err(|e| bad(format!("{name}: corrupt data ({:?})", e.status)))?;
        if data.len() as u64 != size {
            return Err(bad(format!("{name}: size mismatch")));
        }
        let path = dest.join(rel);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        File::create(&path)?.write_all(&data)?;
        done += size;
        progress(done, info.total);
    }
    let pos = r.stream_position()?;
    if pos != end {
        return Err(bad("payload has trailing data"));
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn installer(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut exe = b"MZ fake stub".to_vec();
        let offset = exe.len() as u64;
        let packed: Vec<Packed> = files.iter().map(|(p, d)| pack(p.to_string(), d)).collect();
        write_payload(&mut exe, offset, "1.2.3", &packed).unwrap();
        exe
    }

    #[test]
    fn roundtrip() {
        let big: Vec<u8> = (0..300_000u32).flat_map(|i| (i % 251).to_le_bytes()).collect();
        let exe = installer(&[("dianmo.exe", b"exe bytes"), ("data/rime/build/x.bin", &big), ("empty.txt", b"")]);
        let mut cur = Cursor::new(exe);
        let i = info(&mut cur).unwrap();
        assert_eq!((i.count, i.version.as_str(), i.total), (3, "1.2.3", 9 + big.len() as u64));
        let dir = std::env::temp_dir().join(format!("dianmo-payload-test-{}", std::process::id()));
        let mut calls = 0;
        extract(&mut cur, &dir, |done, total| {
            calls += 1;
            assert!(done <= total);
        })
        .unwrap();
        assert_eq!(calls, 3);
        assert_eq!(std::fs::read(dir.join("dianmo.exe")).unwrap(), b"exe bytes");
        assert_eq!(std::fs::read(dir.join("data/rime/build/x.bin")).unwrap(), big);
        assert_eq!(std::fs::read(dir.join("empty.txt")).unwrap(), b"");
        // list_dir sees what was extracted, '/'-separated and sorted.
        let names: Vec<String> = list_dir(&dir).unwrap().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["data/rime/build/x.bin", "dianmo.exe", "empty.txt"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_damage() {
        assert!(info(&mut Cursor::new(b"MZ no payload at all, just an exe".to_vec())).is_err());
        let mut exe = installer(&[("a.txt", b"hello hello hello hello")]);
        // Truncated.
        assert!(info(&mut Cursor::new(exe[3..].to_vec())).is_err());
        // Flip a byte inside the compressed data: Adler-32 (or inflate) catches it.
        let n = exe.len() - TRAILER_LEN as usize - 3;
        exe[n] ^= 0x55;
        let dir = std::env::temp_dir().join(format!("dianmo-payload-bad-{}", std::process::id()));
        assert!(extract(&mut Cursor::new(exe), &dir, |_, _| {}).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn path_safety() {
        assert!(safe_relative("data/rime/x.bin").is_some());
        for bad in ["", "/abs", "a//b", "../x", "a/../b", "c:/x", "a\\b", "a/./b", "trail.", "x/"] {
            assert!(safe_relative(bad).is_none(), "{bad:?}");
        }
    }
}
