//! UStaticMesh for ArVer 536 (Mirror's Edge uses the stock UE3 layout): LOD 0 geometry only.

use crate::package::Package;
use crate::props::export_props;
use crate::reader::{Reader, Result};

#[derive(Debug, Clone)]
pub struct Section {
    /// Material object reference in the owning package.
    pub material: i32,
    pub first_index: u32,
    pub num_faces: u32,
}

#[derive(Debug, Clone, Default)]
pub struct StaticMesh {
    /// Unreal space, local to the mesh.
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Vertex colours, RGBA (stored as BGRA in the stream).
    pub colors: Vec<[u8; 4]>,
    /// Triangle list in Unreal winding.
    pub indices: Vec<u32>,
    pub sections: Vec<Section>,
    /// kDOP collision triangles (indices into `positions`), what UE3 traces pawns against.
    pub collision: Vec<[u32; 3]>,
    pub bounds_origin: [f32; 3],
    pub bounds_extent: [f32; 3],
}

/// `TArray::BulkSerialize`: element size, count, raw elements.
fn bulk<'a>(r: &mut Reader<'a>) -> Result<(usize, usize, &'a [u8])> {
    let elem = r.i32()?;
    let n = r.count(1 << 24)?;
    if !(0..=4096).contains(&elem) {
        return r.err(format!("bad bulk element size {elem}"));
    }
    let data = r.bytes(elem as usize * n)?;
    Ok((elem as usize, n, data))
}

fn packed_normal(v: u32) -> [f32; 3] {
    let b = v.to_le_bytes();
    let f = |x: u8| x as f32 / 127.5 - 1.0;
    [f(b[0]), f(b[1]), f(b[2])]
}

fn half(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as f32;
    match e {
        0 => s * m * 2f32.powi(-24),
        31 => s * f32::INFINITY,
        _ => s * (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

pub fn read_static_mesh(pkg: &Package, export: usize) -> Result<StaticMesh> {
    let data = pkg.export_bytes(export);
    let (_, end) = export_props(pkg, export)?;
    let mut r = Reader::at(data, end);

    let bounds_origin = r.vec3()?;
    let bounds_extent = r.vec3()?;
    r.f32()?; // SphereRadius
    r.i32()?; // BodySetup
    bulk(&mut r)?; // kDOP nodes
    let (elem, _, raw) = bulk(&mut r)?; // kDOP triangles: v1, v2, v3, material (u16 each)
    let collision: Vec<[u32; 3]> = if elem == 8 {
        raw.chunks_exact(8)
            .map(|c| {
                let i = |k: usize| u16::from_le_bytes([c[k], c[k + 1]]) as u32;
                [i(0), i(2), i(4)]
            })
            .collect()
    } else {
        Vec::new()
    };
    let internal_version = r.i32()?;
    if internal_version >= 17 {
        let n = r.count(10_000)?;
        r.skip(n * 8)?; // TArray<FName>
    }
    let lods = r.count(64)?;
    if lods == 0 {
        return r.err("static mesh has no LODs");
    }

    // FByteBulkData header: flags, element count, size on disk, offset in file.
    let flags = r.u32()?;
    r.i32()?;
    let size = r.i32()?.max(0) as usize;
    r.i32()?;
    if flags & 0x21 == 0 {
        r.skip(size)?;
    }

    let nsec = r.count(4096)?;
    let mut sections = Vec::with_capacity(nsec);
    for _ in 0..nsec {
        let material = r.i32()?;
        r.i32()?;
        r.i32()?;
        r.i32()?; // bEnableShadowCasting
        let first_index = r.i32()? as u32;
        let num_faces = r.i32()? as u32;
        r.i32()?;
        r.i32()?;
        r.i32()?; // Index
        let n = r.count(1 << 20)?;
        r.skip(n * 8)?;
        sections.push(Section { material, first_index, num_faces });
    }

    // Position stream.
    r.i32()?; // VertexSize
    let num_verts = r.count(1 << 24)?;
    let (elem, n, raw) = bulk(&mut r)?;
    if elem != 12 || n != num_verts {
        return r.err(format!("unexpected vertex stream {elem}x{n}"));
    }
    let positions: Vec<[f32; 3]> = raw
        .chunks_exact(12)
        .map(|c| {
            let f = |i: usize| f32::from_le_bytes(c[i..i + 4].try_into().unwrap());
            [f(0), f(4), f(8)]
        })
        .collect();

    // Tangent/normal/UV stream.
    let num_tc = r.i32()?.clamp(0, 8) as usize;
    r.i32()?; // ItemSize
    r.i32()?; // NumVerts
    let full_uv = r.i32()? != 0;
    let (elem, n, raw) = bulk(&mut r)?;
    let uv_size = if full_uv { 8 } else { 4 };
    // Normal[0], Normal[2] (packed), Color (434 <= ArVer < 615), then UVs.
    if n != num_verts || elem < 12 + uv_size * num_tc.max(1) {
        return r.err(format!("unexpected uv stream {elem}x{n} tc={num_tc}"));
    }
    let mut normals = Vec::with_capacity(n);
    let mut uvs = Vec::with_capacity(n);
    let mut colors = Vec::with_capacity(n);
    for c in raw.chunks_exact(elem) {
        normals.push(packed_normal(u32::from_le_bytes(c[4..8].try_into().unwrap())));
        colors.push([c[10], c[9], c[8], c[11]]);
        let u = &c[12..];
        uvs.push(if full_uv {
            [f32::from_le_bytes(u[0..4].try_into().unwrap()), f32::from_le_bytes(u[4..8].try_into().unwrap())]
        } else {
            [half(u16::from_le_bytes([u[0], u[1]])), half(u16::from_le_bytes([u[2], u[3]]))]
        });
    }

    // Shadow volume stream (ArVer < 686), NumVerts, then the index buffer.
    r.i32()?;
    r.i32()?;
    bulk(&mut r)?;
    r.i32()?;
    let (elem, _, raw) = bulk(&mut r)?;
    if elem != 2 {
        return r.err(format!("unexpected index size {elem}"));
    }
    let indices: Vec<u32> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect();
    if indices.iter().any(|&i| i as usize >= positions.len()) {
        return r.err("index out of range");
    }
    let collision = collision.into_iter().filter(|t| t.iter().all(|&i| (i as usize) < positions.len())).collect();

    Ok(StaticMesh { positions, normals, uvs, colors, indices, sections, collision, bounds_origin, bounds_extent })
}
