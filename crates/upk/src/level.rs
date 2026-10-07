//! Placed static geometry of a cooked map: StaticMeshActor / InterpActor components with their
//! world transforms, in Unreal space.

use crate::package::Package;
use crate::props::{Prop, Value, export_props, find};

/// Row-vector affine transform like Unreal's FMatrix: `p' = p.x*x + p.y*y + p.z*z + t`.
#[derive(Debug, Clone, Copy)]
pub struct Affine {
    pub x: [f32; 3],
    pub y: [f32; 3],
    pub z: [f32; 3],
    pub t: [f32; 3],
}

impl Affine {
    pub const IDENTITY: Affine = Affine { x: [1., 0., 0.], y: [0., 1., 0.], z: [0., 0., 1.], t: [0., 0., 0.] };

    /// FScaleRotationTranslationMatrix. Rotator in Unreal units (65536 = 360 degrees).
    pub fn srt(scale: [f32; 3], rot: [i32; 3], trans: [f32; 3]) -> Affine {
        let a = |u: i32| (u.rem_euclid(65536) as f32) * (std::f32::consts::TAU / 65536.0);
        let (sp, cp) = a(rot[0]).sin_cos();
        let (sy, cy) = a(rot[1]).sin_cos();
        let (sr, cr) = a(rot[2]).sin_cos();
        let x = [cp * cy, cp * sy, sp];
        let y = [sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp];
        let z = [-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp];
        let s = |v: [f32; 3], k: f32| [v[0] * k, v[1] * k, v[2] * k];
        Affine { x: s(x, scale[0]), y: s(y, scale[1]), z: s(z, scale[2]), t: trans }
    }

    pub fn translation(t: [f32; 3]) -> Affine {
        Affine { t, ..Affine::IDENTITY }
    }

    pub fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let mut o = self.t;
        for i in 0..3 {
            o[i] += p[0] * self.x[i] + p[1] * self.y[i] + p[2] * self.z[i];
        }
        o
    }

    pub fn vector(&self, v: [f32; 3]) -> [f32; 3] {
        let mut o = [0.0; 3];
        for i in 0..3 {
            o[i] = v[0] * self.x[i] + v[1] * self.y[i] + v[2] * self.z[i];
        }
        o
    }

    /// `self` then `then` (Unreal's `A * B` for row vectors).
    pub fn then(&self, then: &Affine) -> Affine {
        Affine { x: then.vector(self.x), y: then.vector(self.y), z: then.vector(self.z), t: then.point(self.t) }
    }

    pub fn determinant(&self) -> f32 {
        let (a, b, c) = (self.x, self.y, self.z);
        a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])
    }
}

#[derive(Debug, Clone)]
pub struct MeshInstance {
    /// Object reference to the StaticMesh in the package.
    pub mesh: i32,
    pub transform: Affine,
    /// Per-section material overrides from the component (0 = use the mesh's).
    pub materials: Vec<i32>,
    pub collides: bool,
    pub visible: bool,
    pub actor: String,
}

/// Look up a property on an export, falling back through its archetype chain.
fn prop_inherited(pkg: &Package, export: usize, name: &str) -> Option<(usize, Prop)> {
    let mut e = export;
    for _ in 0..8 {
        if let Ok((props, _)) = export_props(pkg, e) {
            if let Some(p) = find(&props, name) {
                return Some((e, p.clone()));
            }
        }
        let arch = pkg.exports[e].archetype;
        if arch <= 0 {
            return None;
        }
        e = arch as usize - 1;
    }
    None
}

fn vec3_prop(pkg: &Package, export: usize, name: &str, default: [f32; 3]) -> [f32; 3] {
    prop_inherited(pkg, export, name)
        .and_then(|(e, p)| p.as_vec3(pkg.export_bytes(e)))
        .unwrap_or(default)
}

fn rot_prop(pkg: &Package, export: usize, name: &str) -> [i32; 3] {
    prop_inherited(pkg, export, name)
        .and_then(|(e, p)| p.as_rotator(pkg.export_bytes(e)))
        .unwrap_or([0, 0, 0])
}

fn float_prop(pkg: &Package, export: usize, name: &str, default: f32) -> f32 {
    match prop_inherited(pkg, export, name).map(|(_, p)| p.value) {
        Some(Value::Float(f)) => f,
        _ => default,
    }
}

fn bool_prop(pkg: &Package, export: usize, name: &str, default: bool) -> bool {
    match prop_inherited(pkg, export, name).map(|(_, p)| p.value) {
        Some(Value::Bool(b)) => b,
        _ => default,
    }
}

fn object_prop(pkg: &Package, export: usize, name: &str) -> i32 {
    match prop_inherited(pkg, export, name).map(|(_, p)| p.value) {
        Some(Value::Object(o)) => o,
        _ => 0,
    }
}

const MESH_ACTORS: &[&str] = &["StaticMeshActor", "InterpActor", "TdMovingPlatform", "KActor", "StaticMeshActor_Sliced"];

/// All static mesh instances placed in the package's level.
pub fn mesh_instances(pkg: &Package) -> Vec<MeshInstance> {
    let mut out = Vec::new();
    for i in 0..pkg.exports.len() {
        let class = pkg.export_class(i);
        if !MESH_ACTORS.contains(&class.as_str()) {
            continue;
        }
        let path = pkg.object_path(i as i32 + 1);
        if !path.contains(".PersistentLevel.") {
            continue; // archetypes and prefab templates, not placed actors
        }
        let comp = object_prop(pkg, i, "StaticMeshComponent");
        if comp <= 0 {
            continue;
        }
        let c = comp as usize - 1;
        let mesh = object_prop(pkg, c, "StaticMesh");
        if mesh == 0 {
            continue;
        }

        let actor_scale = float_prop(pkg, i, "DrawScale", 1.0);
        let s3 = vec3_prop(pkg, i, "DrawScale3D", [1.0; 3]);
        let actor = Affine::translation({
            let p = vec3_prop(pkg, i, "PrePivot", [0.0; 3]);
            [-p[0], -p[1], -p[2]]
        })
        .then(&Affine::srt(
            [s3[0] * actor_scale, s3[1] * actor_scale, s3[2] * actor_scale],
            rot_prop(pkg, i, "Rotation"),
            vec3_prop(pkg, i, "Location", [0.0; 3]),
        ));
        let cs = float_prop(pkg, c, "Scale", 1.0);
        let c3 = vec3_prop(pkg, c, "Scale3D", [1.0; 3]);
        let local = Affine::srt(
            [c3[0] * cs, c3[1] * cs, c3[2] * cs],
            rot_prop(pkg, c, "Rotation"),
            vec3_prop(pkg, c, "Translation", [0.0; 3]),
        );

        let materials = prop_inherited(pkg, c, "Materials")
            .map(|(e, p)| p.as_i32_array(pkg.export_bytes(e)))
            .unwrap_or_default();
        let hidden = bool_prop(pkg, i, "bHidden", false) || bool_prop(pkg, c, "HiddenGame", false);
        let collides = bool_prop(pkg, c, "CollideActors", true) && bool_prop(pkg, c, "BlockActors", true);
        out.push(MeshInstance {
            mesh,
            transform: local.then(&actor),
            materials,
            collides,
            visible: !hidden,
            actor: path,
        });
    }
    out
}
