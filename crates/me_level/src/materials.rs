//! Resolves UE3 materials to what a simple renderer needs: a diffuse texture, blend mode,
//! lighting model and sidedness. Material graphs aren't evaluated; the diffuse texture is the
//! instance's `DiffuseTexture`-like parameter or a referenced texture named like one.

use std::collections::HashMap;
use upk::texture::{FileIndex, Format, read_texture};
use upk::{Package, Value, export_props, find, struct_array};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Opaque,
    Masked,
    Translucent,
    Additive,
    Modulate,
}

#[derive(Clone, Debug)]
pub struct MaterialInfo {
    pub name: String,
    pub diffuse: Option<usize>,
    /// Separate opacity / opacity-mask texture, read from opacity_channel (0 = R .. 3 = A).
    pub opacity: Option<usize>,
    pub opacity_channel: usize,
    /// UV multiplier of the diffuse sample (from TextureCoordinate tiling).
    pub uv_scale: [f32; 2],
    pub blend: Blend,
    pub unlit: bool,
    pub two_sided: bool,
    /// Sky dome: unlit, tinted by vertex colour, never shadowed.
    pub sky: bool,
    /// Runner Vision ("level of interest") tint: the material lerps its colour to the LOI_Color
    /// parameter by LOI_Strength (red on the soft-landing package stacks), linear RGB.
    pub tint: [f32; 3],
}

#[derive(Clone)]
pub struct TextureData {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub srgb: bool,
    /// Mip chain from `width` x `height` down, in the source pixel format.
    pub mips: Vec<Vec<u8>>,
}

pub struct Materials {
    pub infos: Vec<MaterialInfo>,
    pub textures: Vec<TextureData>,
    by_path: HashMap<String, usize>,
    tex_by_path: HashMap<String, Option<usize>>,
    max_size: u32,
}

fn texture_score(name: &str) -> i32 {
    let n = name.rsplit('.').next().unwrap_or(name).to_ascii_lowercase();
    // Normal, specular, cubemap, mask and alpha maps are never the colour.
    let not_colour = ["_n", "_normal", "_s", "_spec", "_m", "_mask", "_a"];
    if n.starts_with("cm_") || n.contains("_n_") || not_colour.iter().any(|s| n.ends_with(s)) {
        return -1;
    }
    if n.ends_with("_d") || n.ends_with("_da") || n.contains("diffuse") || n.contains("_d_") {
        return 2;
    }
    1
}

/// A texture sample found in a material graph.
#[derive(Clone, Debug)]
struct Sample {
    param: Option<String>,
    tex: i32,
    /// UV multiplier from a TextureCoordinate node feeding the sample.
    tiling: [f32; 2],
}

/// The expression connected to an input (ExpressionInput / *MaterialInput are tagged structs)
/// and the channel it reads (0 = R .. 3 = A).
fn input_of(pkg: &Package, data: &[u8], p: &upk::Prop) -> Option<(i32, usize)> {
    let (props, _) = upk::props::read_props(pkg, data, p.start).ok()?;
    let expr = match find(&props, "Expression").map(|p| &p.value) {
        Some(Value::Object(o)) => *o,
        _ => return None,
    };
    let on = |n: &str| matches!(find(&props, n).map(|p| &p.value), Some(Value::Int(1)));
    let channel = if on("MaskA") && !on("MaskR") {
        3
    } else if on("MaskG") && !on("MaskR") {
        1
    } else if on("MaskB") && !on("MaskR") {
        2
    } else {
        0
    };
    Some((expr, channel))
}

fn float_of(props: &[upk::Prop], name: &str, default: f32) -> f32 {
    match find(props, name).map(|p| &p.value) {
        Some(Value::Float(f)) => *f,
        _ => default,
    }
}

/// UV tiling of a Coordinates input: TextureCoordinate, optionally multiplied by a constant.
fn tiling_of(pkg: &Package, expr: i32, depth: u32) -> [f32; 2] {
    if expr <= 0 || depth > 4 {
        return [1.0, 1.0];
    }
    let ex = expr as usize - 1;
    let Ok((props, _)) = export_props(pkg, ex) else { return [1.0, 1.0] };
    let data = pkg.export_bytes(ex);
    match pkg.class_of(expr).as_str() {
        "MaterialExpressionTextureCoordinate" => [float_of(&props, "UTiling", 1.0), float_of(&props, "VTiling", 1.0)],
        "MaterialExpressionConstant" | "MaterialExpressionScalarParameter" => {
            let v = float_of(&props, "R", float_of(&props, "DefaultValue", 1.0));
            [v, v]
        }
        "MaterialExpressionMultiply" => {
            let mut t = [1.0, 1.0];
            for name in ["A", "B"] {
                if let Some((e, _)) = find(&props, name).and_then(|p| input_of(pkg, data, p)) {
                    let s = tiling_of(pkg, e, depth + 1);
                    t = [t[0] * s[0], t[1] * s[1]];
                }
            }
            t
        }
        _ => [1.0, 1.0],
    }
}

/// Walk a material expression graph from `expr` to the first texture sample, preferring the
/// first inputs (Lerp A before B, never the Alpha or UV inputs). `allow_masks` admits textures
/// that don't look like diffuse maps (for opacity inputs).
fn base_texture(pkg: &Package, expr: i32, depth: u32, allow_masks: bool) -> Option<Sample> {
    if expr <= 0 || depth > 12 {
        return None;
    }
    let ex = expr as usize - 1;
    let (props, _) = export_props(pkg, ex).ok()?;
    let data = pkg.export_bytes(ex);
    if pkg.class_of(expr).contains("TextureSample") {
        let tex = match find(&props, "Texture").map(|p| &p.value) {
            Some(Value::Object(t)) => *t,
            _ => 0,
        };
        let param = match find(&props, "ParameterName").map(|p| &p.value) {
            Some(Value::Name(n)) => Some(n.clone()),
            _ => None,
        };
        let usable = allow_masks || tex == 0 || texture_score(&pkg.object_name(tex)) >= 0;
        let tiling = find(&props, "Coordinates")
            .and_then(|p| input_of(pkg, data, p))
            .map_or([1.0, 1.0], |(e, _)| tiling_of(pkg, e, 0));
        return usable.then_some(Sample { param, tex, tiling });
    }
    for p in &props {
        if p.struct_name != "ExpressionInput" || matches!(p.name.as_str(), "Alpha" | "Coordinates" | "UVs" | "Exponent") {
            continue;
        }
        let Some((input, _)) = input_of(pkg, data, p) else { continue };
        if let Some(found) = base_texture(pkg, input, depth + 1, allow_masks) {
            return Some(found);
        }
    }
    None
}

impl Materials {
    pub fn new(max_size: u32) -> Self {
        Self { infos: Vec::new(), textures: Vec::new(), by_path: HashMap::new(), tex_by_path: HashMap::new(), max_size }
    }

    fn texture(&mut self, pkg: &Package, files: &mut FileIndex, tex: i32) -> Option<usize> {
        if tex <= 0 {
            return None;
        }
        let path = pkg.object_path(tex);
        if let Some(&t) = self.tex_by_path.get(&path) {
            return t;
        }
        let loaded = (|| {
            if pkg.class_of(tex) != "Texture2D" {
                return None;
            }
            let t = read_texture(pkg, tex as usize - 1).ok()?;
            if t.format == Format::Other {
                return None;
            }
            let block = matches!(t.format, Format::Dxt1 | Format::Dxt3 | Format::Dxt5);
            let first = t.mips.iter().position(|m| m.width <= self.max_size && m.height <= self.max_size && m.width > 0)?;
            let mut mips = Vec::new();
            for i in first..t.mips.len() {
                let m = &t.mips[i];
                if block && (m.width < 4 || m.height < 4) {
                    break;
                }
                match t.mip_data(pkg, i, files) {
                    Ok(d) if d.len() == t.format.mip_size(m.width, m.height) => mips.push(d),
                    _ => break,
                }
            }
            if mips.is_empty() {
                return None;
            }
            Some(TextureData {
                name: path.clone(),
                width: t.mips[first].width,
                height: t.mips[first].height,
                format: t.format,
                srgb: t.srgb,
                mips,
            })
        })();
        let idx = loaded.map(|t| {
            self.textures.push(t);
            self.textures.len() - 1
        });
        self.tex_by_path.insert(path, idx);
        idx
    }

    /// Material index for an object reference in `pkg` (0 / imports get an untextured default).
    pub fn resolve(&mut self, pkg: &Package, files: &mut FileIndex, mat: i32) -> usize {
        let path = if mat == 0 { "None".to_string() } else { pkg.object_path(mat) };
        if let Some(&i) = self.by_path.get(&path) {
            return i;
        }
        let mut info = MaterialInfo {
            name: path.clone(),
            diffuse: None,
            opacity: None,
            opacity_channel: 0,
            uv_scale: [1.0, 1.0],
            blend: Blend::Opaque,
            unlit: false,
            two_sided: false,
            sky: path.to_ascii_lowercase().contains("sky"),
            tint: [1.0; 3],
        };
        let mut loi_color: Option<[f32; 3]> = None;
        let mut loi_strength: Option<f32> = None;
        // Most-derived instance overrides win, so only fill names not seen yet.
        let mut overrides: HashMap<String, i32> = HashMap::new();
        let mut diffuse_ref = 0;
        let mut opacity_ref = 0;
        let mut e = mat;
        for _ in 0..8 {
            if e <= 0 {
                break;
            }
            let ex = e as usize - 1;
            let Ok((props, _)) = export_props(pkg, ex) else { break };
            let data = pkg.export_bytes(ex);
            let class = pkg.class_of(e);
            if class.starts_with("MaterialInstance") {
                if let Some(arr) = find(&props, "TextureParameterValues") {
                    for el in struct_array(pkg, data, arr) {
                        let (Some(Value::Name(n)), Some(Value::Object(t))) =
                            (find(&el, "ParameterName").map(|p| &p.value), find(&el, "ParameterValue").map(|p| &p.value))
                        else {
                            continue;
                        };
                        overrides.entry(n.to_ascii_lowercase()).or_insert(*t);
                    }
                }
                if let Some(arr) = find(&props, "VectorParameterValues") {
                    for el in struct_array(pkg, data, arr) {
                        let is_loi = matches!(find(&el, "ParameterName").map(|p| &p.value), Some(Value::Name(n)) if n.eq_ignore_ascii_case("LOI_Color"));
                        if let (true, None, Some(v)) = (is_loi, loi_color, find(&el, "ParameterValue")) {
                            let f = |k: usize| f32::from_le_bytes(data[v.start + k * 4..v.start + k * 4 + 4].try_into().unwrap());
                            loi_color = Some([f(0), f(1), f(2)]);
                        }
                    }
                }
                if let Some(arr) = find(&props, "ScalarParameterValues") {
                    for el in struct_array(pkg, data, arr) {
                        let is_loi = matches!(find(&el, "ParameterName").map(|p| &p.value), Some(Value::Name(n)) if n.eq_ignore_ascii_case("LOI_Strength"));
                        if let (true, None, Some(Value::Float(v))) = (is_loi, loi_strength, find(&el, "ParameterValue").map(|p| &p.value)) {
                            loi_strength = Some(*v);
                        }
                    }
                }
                e = match find(&props, "Parent").map(|p| &p.value) {
                    Some(Value::Object(o)) => *o,
                    _ => 0,
                };
                continue;
            }
            // Base Material.
            if let Some(Value::Name(b)) = find(&props, "BlendMode").map(|p| &p.value) {
                info.blend = match b.as_str() {
                    "BLEND_Masked" => Blend::Masked,
                    "BLEND_Translucent" => Blend::Translucent,
                    "BLEND_Additive" => Blend::Additive,
                    "BLEND_Modulate" => Blend::Modulate,
                    _ => Blend::Opaque,
                };
            }
            if let Some(Value::Name(l)) = find(&props, "LightingModel").map(|p| &p.value) {
                info.unlit = l == "MLM_Unlit";
            }
            if let Some(Value::Bool(t)) = find(&props, "TwoSided").map(|p| &p.value) {
                info.two_sided = *t;
            }
            // Follow DiffuseColor (or EmissiveColor for unlit/sky materials) to its texture.
            for input in ["DiffuseColor", "EmissiveColor"] {
                if diffuse_ref != 0 {
                    break;
                }
                let Some((expr, _)) = find(&props, input).and_then(|p| input_of(pkg, data, p)) else { continue };
                if let Some(s) = base_texture(pkg, expr, 0, false) {
                    diffuse_ref = s.param.and_then(|n| overrides.get(&n.to_ascii_lowercase()).copied()).unwrap_or(s.tex);
                    info.uv_scale = s.tiling;
                }
            }
            // Opacity: a separate texture (wire fences) or the diffuse's own alpha channel.
            if matches!(info.blend, Blend::Masked | Blend::Translucent) {
                for input in ["OpacityMask", "Opacity"] {
                    let Some((expr, channel)) = find(&props, input).and_then(|p| input_of(pkg, data, p)) else { continue };
                    if let Some(s) = base_texture(pkg, expr, 0, true) {
                        opacity_ref = s.param.and_then(|n| overrides.get(&n.to_ascii_lowercase()).copied()).unwrap_or(s.tex);
                        info.opacity_channel = channel;
                        break;
                    }
                }
            }
            if diffuse_ref == 0 {
                // Fall back to naming: the first diffuse-looking referenced texture.
                let mut best = (0, 0);
                let refs = find(&props, "ReferencedTextures").map(|p| p.as_i32_array(data)).unwrap_or_default();
                for t in overrides.values().copied().chain(refs) {
                    let s = texture_score(&pkg.object_name(t));
                    if t > 0 && s > best.0 {
                        best = (s, t);
                    }
                }
                diffuse_ref = best.1;
            }
            break;
        }
        if diffuse_ref == 0 {
            // The chain left the package (a parent imported from a shared material package such
            // as CH_Materials): the instance's diffuse-named texture parameter, else naming.
            diffuse_ref = overrides.iter().filter(|(n, t)| **t > 0 && n.contains("diffuse")).map(|(_, t)| *t).next().unwrap_or(0);
            if diffuse_ref == 0 {
                let mut best = (0, 0);
                for t in overrides.values().copied() {
                    let s = texture_score(&pkg.object_name(t));
                    if t > 0 && s > best.0 {
                        best = (s, t);
                    }
                }
                diffuse_ref = best.1;
            }
        }
        // still nothing: resolve the imported parent (CH_Materials.M_Eyes_SH for the SWAT
        // eyes) in its own package and take its diffuse
        let mut imported_diffuse = None;
        if diffuse_ref == 0 && e < 0 {
            let path = pkg.object_path(e);
            let name = pkg.object_name(e);
            if let Some((p, _)) = path.split_once('.') {
                if let Some(other) = files.path_of(p).cloned().and_then(|f| Package::open(f).ok()) {
                    if let Some(i) = other.find_export(&name, None) {
                        let j = self.resolve(&other, files, i as i32 + 1);
                        imported_diffuse = self.infos[j].diffuse;
                    }
                }
            }
        }
        if info.sky {
            info.unlit = true;
        }
        info.diffuse = if diffuse_ref == 0 && imported_diffuse.is_some() { imported_diffuse } else { self.texture(pkg, files, diffuse_ref) };
        if opacity_ref != diffuse_ref {
            info.opacity = self.texture(pkg, files, opacity_ref);
        }
        if let (Some(c), Some(st)) = (loi_color, loi_strength) {
            info.tint = std::array::from_fn(|k| 1.0 + (c[k].clamp(0.0, 1.0) - 1.0) * st.clamp(0.0, 1.0));
        }
        self.infos.push(info);
        let i = self.infos.len() - 1;
        self.by_path.insert(path, i);
        i
    }
}
