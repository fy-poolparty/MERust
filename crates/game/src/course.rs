//! Renders `tdsim::testmap` blocks with world-space tiled UVs so speed reads on screen.

use crate::ue_to_bevy;
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::Indices,
    prelude::*,
    render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
};
use std::collections::BTreeMap;
use tdsim::testmap::{self, Kind};

/// Metres per texture repeat.
const TILE_M: f32 = 1.0;

fn kind_color(k: Kind) -> Color {
    match k {
        Kind::Floor => Color::srgb(0.93, 0.94, 0.95),
        Kind::Wall => Color::srgb(0.84, 0.86, 0.88),
        Kind::Red => Color::srgb(0.86, 0.08, 0.05),
        Kind::Blue => Color::srgb(0.06, 0.32, 0.86),
        Kind::Yellow => Color::srgb(0.96, 0.76, 0.06),
        Kind::Dark => Color::srgb(0.30, 0.32, 0.35),
    }
}

/// A light concrete tile: flat fill, darker seams, a little noise.
fn tile_texture() -> Image {
    const N: u32 = 128;
    let mut data = Vec::with_capacity((N * N * 4) as usize);
    let mut seed = 0x2545_f491u32;
    for y in 0..N {
        for x in 0..N {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let noise = (seed % 9) as i32 - 4;
            let seam = x < 2 || y < 2;
            let v = (if seam { 196 } else { 246 } + noise).clamp(0, 255) as u8;
            data.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let mut img = Image::new(
        Extent3d { width: N, height: N, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    img
}

#[derive(Default)]
struct MeshBuf {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    idx: Vec<u32>,
}

impl MeshBuf {
    /// A hexahedron from Unreal-space corners (bit 0 = +x, bit 1 = +y, bit 2 = +z).
    fn add_hex(&mut self, c: [tdsim::Vec3; 8]) {
        let p: Vec<Vec3> = c.iter().map(|&v| ue_to_bevy(v)).collect();
        // faces as corner quads (any winding; fixed up by the normal test below)
        const FACES: [[usize; 4]; 6] = [[0, 2, 6, 4], [1, 3, 7, 5], [0, 1, 5, 4], [2, 3, 7, 6], [0, 1, 3, 2], [4, 5, 7, 6]];
        let centre = p.iter().copied().sum::<Vec3>() / 8.0;
        for f in FACES {
            let q = [p[f[0]], p[f[1]], p[f[2]], p[f[3]]];
            let mut n = (q[1] - q[0]).cross(q[2] - q[0]).normalize_or_zero();
            let fc = (q[0] + q[1] + q[2] + q[3]) * 0.25;
            if n.dot(fc - centre) < 0.0 {
                n = -n;
            }
            // world-space UVs on the two axes most perpendicular to the normal
            let a = n.abs();
            let (ua, va) = if a.y >= a.x && a.y >= a.z { (0, 2) } else if a.x >= a.z { (2, 1) } else { (0, 1) };
            let base = self.pos.len() as u32;
            for v in q {
                self.pos.push(v.into());
                self.nrm.push(n.into());
                self.uv.push([v[ua] / TILE_M, v[va] / TILE_M]);
            }
            if (q[1] - q[0]).cross(q[2] - q[0]).dot(n) > 0.0 {
                self.idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            } else {
                self.idx.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
            }
        }
    }

    fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}

pub fn spawn_course(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) {
    let tex = images.add(tile_texture());
    let mut by_kind: BTreeMap<Kind, MeshBuf> = BTreeMap::new();
    for b in testmap::blocks() {
        by_kind.entry(b.kind).or_default().add_hex(testmap::corners(&b));
    }
    for (k, buf) in by_kind {
        commands.spawn((
            Mesh3d(meshes.add(buf.into_mesh())),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: kind_color(k),
                base_color_texture: Some(tex.clone()),
                perceptual_roughness: 0.75,
                ..default()
            })),
        ));
    }
}
