//! UE3 package container: summary, LZO chunk decompression, name/import/export tables.
//! Layout follows FPackageFileSummary / FObjectExport for ArVer 536 (Mirror's Edge).

use crate::reader::{ReadError, Reader, Result};
use std::path::Path;

pub const PACKAGE_TAG: u32 = 0x9E2A_83C1;
const COMPRESS_ZLIB: u32 = 1;
const COMPRESS_LZO: u32 = 2;

#[derive(Debug, Clone)]
pub struct Summary {
    pub file_version: u16,
    pub licensee_version: u16,
    pub header_size: i32,
    pub folder: String,
    pub package_flags: u32,
    pub name_count: usize,
    pub name_offset: usize,
    pub export_count: usize,
    pub export_offset: usize,
    pub import_count: usize,
    pub import_offset: usize,
    pub engine_version: i32,
    pub cooker_version: i32,
    pub compression_flags: u32,
    pub chunks: Vec<Chunk>,
}

#[derive(Debug, Clone, Copy)]
pub struct Chunk {
    pub uncompressed_offset: usize,
    pub uncompressed_size: usize,
    pub compressed_offset: usize,
    pub compressed_size: usize,
}

/// An FName: index into the name table plus instance number (0 = none, n = `_{n-1}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FName {
    pub index: i32,
    pub number: i32,
}

#[derive(Debug, Clone)]
pub struct Import {
    pub class_package: FName,
    pub class_name: FName,
    pub outer: i32,
    pub name: FName,
}

#[derive(Debug, Clone)]
pub struct Export {
    pub class: i32,
    pub super_: i32,
    pub outer: i32,
    pub name: FName,
    pub archetype: i32,
    pub flags: u64,
    pub serial_size: usize,
    pub serial_offset: usize,
    pub export_flags: u32,
}

pub struct Package {
    pub name: String,
    pub summary: Summary,
    /// The whole package with compressed chunks expanded in place.
    pub data: Vec<u8>,
    pub names: Vec<String>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}

fn read_summary(r: &mut Reader) -> Result<Summary> {
    if r.u32()? != PACKAGE_TAG {
        return r.err("not an Unreal package");
    }
    let file_version = r.u16()?;
    let licensee_version = r.u16()?;
    if file_version != 536 {
        return r.err(format!("package version {file_version} is not Mirror's Edge (536)"));
    }
    let header_size = r.i32()?;
    let folder = r.fstring()?;
    let package_flags = r.u32()?;
    let name_count = r.count(10_000_000)?;
    let name_offset = r.i32()? as usize;
    let export_count = r.count(10_000_000)?;
    let export_offset = r.i32()? as usize;
    let import_count = r.count(10_000_000)?;
    let import_offset = r.i32()? as usize;
    let _depends_offset = r.i32()?;
    r.skip(16)?; // Guid
    let generations = r.count(1000)?;
    r.skip(generations * 12)?; // ExportCount, NameCount, NetObjectCount
    let engine_version = r.i32()?;
    let cooker_version = r.i32()?;
    let compression_flags = r.u32()?;
    let n = r.count(100_000)?;
    let mut chunks = Vec::with_capacity(n);
    for _ in 0..n {
        chunks.push(Chunk {
            uncompressed_offset: r.i32()? as usize,
            uncompressed_size: r.i32()? as usize,
            compressed_offset: r.i32()? as usize,
            compressed_size: r.i32()? as usize,
        });
    }
    Ok(Summary {
        file_version,
        licensee_version,
        header_size,
        folder,
        package_flags,
        name_count,
        name_offset,
        export_count,
        export_offset,
        import_count,
        import_offset,
        engine_version,
        cooker_version,
        compression_flags,
        chunks,
    })
}

/// FCompressedChunkHeader + blocks, each block LZO1X.
fn decompress_chunk(file: &[u8], c: &Chunk, flags: u32, out: &mut [u8]) -> Result<()> {
    let mut r = Reader::at(file, c.compressed_offset);
    if r.u32()? != PACKAGE_TAG {
        return r.err("bad chunk tag");
    }
    let block_size = r.i32()?.max(1) as usize;
    let _total_c = r.i32()?;
    let total_u = r.i32()? as usize;
    let nblocks = total_u.div_ceil(block_size);
    let mut sizes = Vec::with_capacity(nblocks);
    for _ in 0..nblocks {
        sizes.push((r.i32()? as usize, r.i32()? as usize));
    }
    let mut dst = 0usize;
    for (cs, us) in sizes {
        let src = r.bytes(cs)?;
        let target = &mut out[dst..dst + us];
        match flags {
            COMPRESS_LZO => {
                let n = lzo::decompress_into(src, target)
                    .map_err(|e| ReadError { offset: r.pos, what: format!("lzo: {e:?}") })?;
                if n != us {
                    return r.err(format!("lzo block gave {n} bytes, expected {us}"));
                }
            }
            COMPRESS_ZLIB => return r.err("zlib chunks are not used by Mirror's Edge"),
            _ => return r.err(format!("unknown compression {flags}")),
        }
        dst += us;
    }
    Ok(())
}

impl Package {
    pub fn open(path: impl AsRef<Path>) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();
        let file = std::fs::read(path)?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Self::from_bytes(name, file)?)
    }

    pub fn from_bytes(name: String, file: Vec<u8>) -> Result<Self> {
        let mut r = Reader::new(&file);
        let summary = read_summary(&mut r)?;
        let data = if summary.chunks.is_empty() {
            file
        } else {
            let end = summary.chunks.iter().map(|c| c.uncompressed_offset + c.uncompressed_size).max().unwrap();
            let start = summary.chunks.iter().map(|c| c.uncompressed_offset).min().unwrap();
            let mut data = vec![0u8; end];
            data[..start.min(file.len())].copy_from_slice(&file[..start.min(file.len())]);
            for c in &summary.chunks {
                let (a, b) = (c.uncompressed_offset, c.uncompressed_offset + c.uncompressed_size);
                decompress_chunk(&file, c, summary.compression_flags, &mut data[a..b])?;
            }
            data
        };

        let mut r = Reader::at(&data, summary.name_offset);
        let mut names = Vec::with_capacity(summary.name_count);
        for _ in 0..summary.name_count {
            names.push(r.fstring()?);
            r.u64()?; // flags
        }

        let mut r = Reader::at(&data, summary.import_offset);
        let mut imports = Vec::with_capacity(summary.import_count);
        for _ in 0..summary.import_count {
            let class_package = FName { index: r.i32()?, number: r.i32()? };
            let class_name = FName { index: r.i32()?, number: r.i32()? };
            let outer = r.i32()?;
            let name = FName { index: r.i32()?, number: r.i32()? };
            imports.push(Import { class_package, class_name, outer, name });
        }

        let mut r = Reader::at(&data, summary.export_offset);
        let mut exports = Vec::with_capacity(summary.export_count);
        for _ in 0..summary.export_count {
            let class = r.i32()?;
            let super_ = r.i32()?;
            let outer = r.i32()?;
            let name = FName { index: r.i32()?, number: r.i32()? };
            let archetype = r.i32()?;
            let flags = r.u64()?;
            let serial_size = r.i32()?.max(0) as usize;
            let serial_offset = r.i32()?.max(0) as usize;
            let components = r.count(100_000)?;
            r.skip(components * 12)?; // TMap<FName, int>
            let export_flags = r.u32()?;
            let net = r.count(100_000)?;
            r.skip(net * 4 + 16)?; // NetObjectCount, Guid
            r.u32()?; // PackageFlags
            exports.push(Export { class, super_, outer, name, archetype, flags, serial_size, serial_offset, export_flags });
        }

        Ok(Self { name, summary, data, names, imports, exports })
    }

    pub fn name(&self, n: FName) -> String {
        let base = self.names.get(n.index as usize).map(String::as_str).unwrap_or("<bad-name>");
        if n.number > 0 { format!("{base}_{}", n.number - 1) } else { base.to_string() }
    }

    pub fn name_str(&self, n: FName) -> &str {
        self.names.get(n.index as usize).map(String::as_str).unwrap_or("<bad-name>")
    }

    /// Object reference: >0 export (1-based), <0 import, 0 none.
    pub fn object_name(&self, index: i32) -> String {
        match index {
            0 => "None".into(),
            i if i > 0 => self.exports.get(i as usize - 1).map_or("<bad-export>".into(), |e| self.name(e.name)),
            i => self.imports.get((-i) as usize - 1).map_or("<bad-import>".into(), |e| self.name(e.name)),
        }
    }

    pub fn outer_of(&self, index: i32) -> i32 {
        match index {
            0 => 0,
            i if i > 0 => self.exports.get(i as usize - 1).map_or(0, |e| e.outer),
            i => self.imports.get((-i) as usize - 1).map_or(0, |e| e.outer),
        }
    }

    /// `Outer.Outer.Name` path of an object reference.
    pub fn object_path(&self, index: i32) -> String {
        let mut parts = vec![self.object_name(index)];
        let mut o = self.outer_of(index);
        let mut guard = 0;
        while o != 0 && guard < 32 {
            parts.push(self.object_name(o));
            o = self.outer_of(o);
            guard += 1;
        }
        parts.reverse();
        parts.join(".")
    }

    /// Class name of an object reference.
    pub fn class_of(&self, index: i32) -> String {
        match index {
            0 => "None".into(),
            i if i > 0 => {
                let e = &self.exports[i as usize - 1];
                if e.class == 0 { "Class".into() } else { self.object_name(e.class) }
            }
            i => self.name(self.imports[(-i) as usize - 1].class_name),
        }
    }

    pub fn export_class(&self, export: usize) -> String {
        self.class_of(export as i32 + 1)
    }

    pub fn export_bytes(&self, export: usize) -> &[u8] {
        let e = &self.exports[export];
        &self.data[e.serial_offset..(e.serial_offset + e.serial_size).min(self.data.len())]
    }

    /// Find an export by name (case-insensitive), optionally of a given class.
    pub fn find_export(&self, name: &str, class: Option<&str>) -> Option<usize> {
        self.exports.iter().enumerate().position(|(i, e)| {
            self.name(e.name).eq_ignore_ascii_case(name)
                && class.is_none_or(|c| self.export_class(i).eq_ignore_ascii_case(c))
        })
    }
}
