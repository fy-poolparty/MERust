//! UTexture2D (ArVer 536): mip table with bulk data either inline in the map package or in the
//! texture's source package (`StoreInSeparateFile`, usually LZO-compressed).

use crate::package::{PACKAGE_TAG, Package};
use crate::props::{Value, export_props, find};
use crate::reader::{ReadError, Reader, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const BULK_SEPARATE_FILE: u32 = 0x01;
const BULK_ZLIB: u32 = 0x02;
const BULK_LZO: u32 = 0x10;
const BULK_UNUSED: u32 = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Dxt1,
    Dxt3,
    Dxt5,
    Bgra8,
    G8,
    Other,
}

impl Format {
    fn from_name(n: &str) -> Format {
        match n {
            "PF_DXT1" => Format::Dxt1,
            "PF_DXT3" => Format::Dxt3,
            "PF_DXT5" => Format::Dxt5,
            "PF_A8R8G8B8" => Format::Bgra8,
            "PF_G8" => Format::G8,
            _ => Format::Other,
        }
    }

    /// Bytes for a `w` x `h` mip.
    pub fn mip_size(self, w: u32, h: u32) -> usize {
        let blocks = (w.div_ceil(4) * h.div_ceil(4)) as usize;
        match self {
            Format::Dxt1 => blocks * 8,
            Format::Dxt3 | Format::Dxt5 => blocks * 16,
            Format::Bgra8 => (w * h * 4) as usize,
            Format::G8 => (w * h) as usize,
            Format::Other => 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Mip {
    pub width: u32,
    pub height: u32,
    flags: u32,
    /// Uncompressed byte count.
    size: usize,
    size_on_disk: usize,
    /// Inline: offset into the map's uncompressed data. Separate: offset into the source file.
    offset: usize,
}

#[derive(Debug, Clone)]
pub struct Texture2D {
    pub name: String,
    /// Top-level package the texture belongs to; separate-file mips live in `<package>.upk`.
    pub source_package: String,
    pub format: Format,
    pub srgb: bool,
    pub mips: Vec<Mip>,
}

pub fn read_texture(pkg: &Package, export: usize) -> Result<Texture2D> {
    let data = pkg.export_bytes(export);
    let (props, end) = export_props(pkg, export)?;
    let format = match find(&props, "Format").map(|p| &p.value) {
        Some(Value::Name(n)) => Format::from_name(n),
        Some(Value::Byte(0)) | None => Format::Other,
        _ => Format::Other,
    };
    let srgb = !matches!(find(&props, "SRGB").map(|p| &p.value), Some(Value::Bool(false)));
    let mut r = Reader::at(data, end);
    // SourceArt bulk data header, never stored in cooked packages.
    let flags = r.u32()?;
    r.i32()?;
    let sz = r.i32()?.max(0) as usize;
    r.i32()?;
    if flags & (BULK_SEPARATE_FILE | BULK_UNUSED) == 0 {
        r.skip(sz)?;
    }
    let n = r.count(32)?;
    let base = pkg.exports[export].serial_offset;
    let mut mips = Vec::with_capacity(n);
    for _ in 0..n {
        let flags = r.u32()?;
        let size = r.i32()?.max(0) as usize;
        let size_on_disk = r.i32()?.max(0) as usize;
        let offset_in_file = r.i32()?.max(0) as usize;
        let inline_at = base + r.pos;
        if flags & (BULK_SEPARATE_FILE | BULK_UNUSED) == 0 {
            r.skip(size_on_disk)?;
        }
        let width = r.i32()?.max(0) as u32;
        let height = r.i32()?.max(0) as u32;
        let offset = if flags & BULK_SEPARATE_FILE != 0 { offset_in_file } else { inline_at };
        mips.push(Mip { width, height, flags, size, size_on_disk, offset });
    }
    let path = pkg.object_path(export as i32 + 1);
    let source_package = path.split('.').next().unwrap_or_default().to_string();
    Ok(Texture2D { name: path, source_package, format, srgb, mips })
}

/// Finds and caches source packages by name under CookedPC.
pub struct FileIndex {
    files: HashMap<String, PathBuf>,
    cache: HashMap<PathBuf, Vec<u8>>,
}

impl FileIndex {
    pub fn new(cooked: &Path) -> Self {
        fn walk(dir: &Path, out: &mut HashMap<String, PathBuf>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if let Some(stem) = p.file_stem() {
                    out.entry(stem.to_string_lossy().to_ascii_lowercase()).or_insert(p);
                }
            }
        }
        let mut files = HashMap::new();
        walk(cooked, &mut files);
        Self { files, cache: HashMap::new() }
    }

    pub fn path_of(&self, package: &str) -> Option<&PathBuf> {
        self.files.get(&package.to_ascii_lowercase())
    }

    fn bytes(&mut self, package: &str) -> Option<&[u8]> {
        let path = self.path_of(package)?.clone();
        if !self.cache.contains_key(&path) {
            let data = std::fs::read(&path).ok()?;
            self.cache.insert(path.clone(), data);
        }
        self.cache.get(&path).map(Vec::as_slice)
    }
}

/// Bulk data compressed with the package chunk format (tag, block size, blocks).
fn decompress_bulk(src: &[u8], out_len: usize) -> Result<Vec<u8>> {
    let mut r = Reader::new(src);
    if r.u32()? != PACKAGE_TAG {
        return r.err("bad compressed bulk tag");
    }
    let block_size = r.i32()?.max(1) as usize;
    r.i32()?;
    let total = r.i32()?.max(0) as usize;
    let nblocks = total.div_ceil(block_size);
    let mut sizes = Vec::with_capacity(nblocks);
    for _ in 0..nblocks {
        sizes.push((r.i32()?.max(0) as usize, r.i32()?.max(0) as usize));
    }
    let mut out = vec![0u8; total.max(out_len)];
    let mut dst = 0;
    for (cs, us) in sizes {
        let block = r.bytes(cs)?;
        let n = lzo::decompress_into(block, &mut out[dst..dst + us])
            .map_err(|e| ReadError { offset: r.pos, what: format!("lzo: {e:?}") })?;
        dst += n;
    }
    out.truncate(out_len);
    Ok(out)
}

impl Texture2D {
    /// Raw bytes of mip `i` in the texture's pixel format.
    pub fn mip_data(&self, pkg: &Package, i: usize, files: &mut FileIndex) -> Result<Vec<u8>> {
        let m = &self.mips[i];
        let fail = |what: String| Err(ReadError { offset: m.offset, what });
        if m.flags & BULK_UNUSED != 0 || m.size == 0 {
            return fail("mip not stored".into());
        }
        let raw: &[u8] = if m.flags & BULK_SEPARATE_FILE != 0 {
            let Some(file) = files.bytes(&self.source_package) else {
                return fail(format!("source package {} not found", self.source_package));
            };
            match file.get(m.offset..m.offset + m.size_on_disk) {
                Some(s) => s,
                None => return fail("mip outside source file".into()),
            }
        } else {
            match pkg.data.get(m.offset..m.offset + m.size_on_disk) {
                Some(s) => s,
                None => return fail("inline mip out of range".into()),
            }
        };
        if m.flags & BULK_LZO != 0 {
            decompress_bulk(raw, m.size)
        } else if m.flags & BULK_ZLIB != 0 {
            fail("zlib bulk data".into())
        } else {
            Ok(raw[..m.size.min(raw.len())].to_vec())
        }
    }
}
