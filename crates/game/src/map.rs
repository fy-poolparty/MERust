//! Turns `me_level` output (Unreal space) into Bevy meshes, textures and materials.

use crate::ue_to_bevy;
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::NotShadowCaster,
    mesh::Indices,
    prelude::*,
    render::render_resource::{Extent3d, PrimitiveTopology, TextureFormat},
};
use me_level::{Batch, Blend, MaterialInfo, TextureData, TextureFormat as Fmt};
use std::collections::HashMap;

/// The render half of a loaded me_level::Level (collision goes to the simulation).
pub struct RenderLevel {
    pub batches: Vec<Batch>,
    pub materials: Vec<MaterialInfo>,
    pub textures: Vec<TextureData>,
}

pub fn to_image(t: &TextureData) -> Option<Image> {
    let format = match (t.format, t.srgb) {
        (Fmt::Dxt1, true) => TextureFormat::Bc1RgbaUnormSrgb,
        (Fmt::Dxt1, false) => TextureFormat::Bc1RgbaUnorm,
        (Fmt::Dxt3, true) => TextureFormat::Bc2RgbaUnormSrgb,
        (Fmt::Dxt3, false) => TextureFormat::Bc2RgbaUnorm,
        (Fmt::Dxt5, true) => TextureFormat::Bc3RgbaUnormSrgb,
        (Fmt::Dxt5, false) => TextureFormat::Bc3RgbaUnorm,
        (Fmt::Bgra8, true) => TextureFormat::Bgra8UnormSrgb,
        (Fmt::Bgra8, false) => TextureFormat::Bgra8Unorm,
        (Fmt::G8, _) => TextureFormat::R8Unorm,
        _ => return None,
    };
    let mut img = Image::default();
    img.data = Some(t.mips.concat());
    img.texture_descriptor.size = Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 };
    img.texture_descriptor.format = format;
    img.texture_descriptor.mip_level_count = t.mips.len() as u32;
    img.asset_usage = RenderAssetUsages::RENDER_WORLD;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    Some(img)
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn spawn_level(
    level: &RenderLevel,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) {
    let textures: Vec<Option<Handle<Image>>> = level.textures.iter().map(|t| to_image(t).map(|i| images.add(i))).collect();
    let mut merged: HashMap<(usize, usize), Handle<Image>> = HashMap::new();
    let mats: Vec<Handle<StandardMaterial>> = level
        .materials
        .iter()
        .map(|m| {
            let tex = match (m.diffuse, m.opacity) {
                (Some(d), Some(o)) => Some(
                    merged
                        .entry((d, o))
                        .or_insert_with(|| {
                            let (w, h, mips) = me_level::decode::merge_opacity(&level.textures[d], &level.textures[o], m.opacity_channel);
                            let rgba = TextureData {
                                name: String::new(),
                                width: w,
                                height: h,
                                format: Fmt::Bgra8,
                                srgb: true,
                                // to_image expects BGRA for Bgra8; swap back from RGBA.
                                mips: mips
                                    .into_iter()
                                    .map(|m| m.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect())
                                    .collect(),
                            };
                            images.add(to_image(&rgba).unwrap())
                        })
                        .clone(),
                ),
                (Some(d), None) => textures[d].clone(),
                _ => None,
            };
            let untextured = tex.is_none();
            materials.add(StandardMaterial {
                base_color: if untextured && !m.sky { Color::srgb(0.85, 0.86, 0.88) } else { Color::linear_rgb(m.tint[0], m.tint[1], m.tint[2]) },
                base_color_texture: tex,
                perceptual_roughness: 0.85,
                reflectance: 0.3,
                unlit: m.unlit,
                double_sided: m.two_sided,
                cull_mode: if m.two_sided { None } else { Some(bevy::render::render_resource::Face::Back) },
                alpha_mode: match m.blend {
                    Blend::Opaque => AlphaMode::Opaque,
                    _ if m.opacity.is_some() && m.blend == Blend::Masked => AlphaMode::Mask(0.4),
                    Blend::Masked => AlphaMode::Mask(0.33),
                    // Wire fences etc.: crisp alpha test instead of sorted blending.
                    Blend::Translucent if m.opacity.is_some() => AlphaMode::Mask(0.4),
                    Blend::Translucent => AlphaMode::Blend,
                    Blend::Additive => AlphaMode::Add,
                    Blend::Modulate => AlphaMode::Multiply,
                },
                fog_enabled: !m.sky,
                uv_transform: bevy::math::Affine2::from_scale(Vec2::new(m.uv_scale[0], m.uv_scale[1])),
                ..default()
            })
        })
        .collect();

    for b in &level.batches {
        let info = &level.materials[b.material];
        let pos: Vec<[f32; 3]> = b.positions.iter().map(|&p| ue_to_bevy(p.into()).into()).collect();
        let nrm: Vec<[f32; 3]> = b.normals.iter().map(|n| [n[0], n[2], n[1]]).collect();
        // Unreal front faces are clockwise in its left-handed space; the Y/Z swap mirrors that
        // into Bevy's counter-clockwise front faces, so indices carry over unchanged.
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, b.uvs.clone())
            .with_inserted_indices(Indices::U32(b.indices.clone()));
        if b.colors.len() == b.positions.len() {
            let cols: Vec<[f32; 4]> = b
                .colors
                .iter()
                .map(|c| [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), c[3] as f32 / 255.0])
                .collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, cols);
        }
        let mut e = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mats[b.material].clone())));
        if info.sky || info.unlit || info.blend != Blend::Opaque && info.blend != Blend::Masked {
            e.insert(NotShadowCaster);
        }
    }
}
