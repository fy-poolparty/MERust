//! USkeletalMesh (UE3, ArVer 536, Mirror's Edge licensee 43): bounds, materials, origin,
//! reference skeleton, and LOD 0 geometry with skin weights.

use crate::package::Package;
use crate::props::export_props;
use crate::reader::{Reader, Result};

#[derive(Clone, Debug)]
pub struct Bone {
    pub name: String,
    pub flags: u32,
    /// Local rotation (x, y, z, w) relative to the parent, Unreal convention.
    pub orientation: [f32; 4],
    pub position: [f32; 3],
    pub num_children: i32,
    pub parent: i32,
}

#[derive(Clone, Debug, Default)]
pub struct SkelMesh {
    pub name: String,
    pub materials: Vec<i32>,
    pub origin: [f32; 3],
    /// Pitch, yaw, roll (65536 = 360 degrees).
    pub rot_origin: [i32; 3],
    pub bones: Vec<Bone>,
    pub skeletal_depth: i32,
    pub lod: Option<SkelLod>,
}

#[derive(Clone, Debug, Default)]
pub struct SkelSection {
    pub material: u16,
    pub first_index: u32,
    pub num_triangles: u32,
}

#[derive(Clone, Debug, Default)]
pub struct SkelVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// Up to four (bone index into `SkelMesh::bones`, weight 0..1).
    pub influences: [(u16, f32); 4],
}

#[derive(Clone, Debug, Default)]
pub struct SkelLod {
    pub sections: Vec<SkelSection>,
    pub indices: Vec<u32>,
    pub vertices: Vec<SkelVertex>,
}

fn name(pkg: &Package, r: &mut Reader) -> Result<String> {
    let idx = r.i32()?;
    let num = r.i32()?;
    let mut s = pkg.names.get(idx as usize).cloned().unwrap_or_default();
    if num > 0 {
        s = format!("{s}_{}", num - 1);
    }
    Ok(s)
}

/// Read the mesh up to and including the reference skeleton (enough for animation).
pub fn read_skeleton(pkg: &Package, export: usize) -> Result<(SkelMesh, usize)> {
    let (_, end) = export_props(pkg, export)?;
    let data = pkg.export_bytes(export);
    let mut r = Reader::at(data, end);
    let mut m = SkelMesh { name: pkg.object_name(export as i32 + 1), ..Default::default() };
    if pkg.summary.licensee_version >= 15 {
        let _unk = r.i32()?;
    }
    // FBoxSphereBounds
    r.skip(28)?;
    let n = r.count(1024)?;
    for _ in 0..n {
        m.materials.push(r.i32()?);
    }
    m.origin = r.vec3()?;
    m.rot_origin = [r.i32()?, r.i32()?, r.i32()?];
    let nb = r.count(4096)?;
    for _ in 0..nb {
        let name = name(pkg, &mut r)?;
        let flags = r.u32()?;
        let orientation = [r.f32()?, r.f32()?, r.f32()?, r.f32()?];
        let position = r.vec3()?;
        let num_children = r.i32()?;
        let parent = r.i32()?;
        let _color = r.u32()?; // ArVer >= 515
        m.bones.push(Bone { name, flags, orientation, position, num_children, parent });
    }
    m.skeletal_depth = r.i32()?;
    Ok((m, r.pos))
}
