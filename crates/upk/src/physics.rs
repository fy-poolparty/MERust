//! PhysicsAsset: the ragdoll's rigid bodies (RB_BodySetup: per-bone KAggregateGeom of spheres,
//! boxes and capsules) and joints (RB_ConstraintSetup: frames and swing/twist limits).

use crate::package::Package;
use crate::props::{export_props, find, read_props, struct_array, Prop, Value};

/// Unreal to physics scale (U2PScale): constraint positions are stored in physics units.
pub const U2P_SCALE: f32 = 0.02;

/// A row-major FMatrix (rows X, Y, Z axes and the origin), in the bone's space.
pub type Matrix = [[f32; 4]; 4];

#[derive(Debug, Clone)]
pub enum Shape {
    Sphere { tm: Matrix, radius: f32 },
    /// Half extents are X/2, Y/2, Z/2.
    Box { tm: Matrix, x: f32, y: f32, z: f32 },
    /// A capsule along the element's Z axis: cylinder of `length` plus two `radius` caps.
    Sphyl { tm: Matrix, radius: f32, length: f32 },
}

#[derive(Debug, Clone)]
pub struct BodySetup {
    pub bone: String,
    pub shapes: Vec<Shape>,
}

#[derive(Debug, Clone)]
pub struct ConstraintSetup {
    pub joint: String,
    /// The child body.
    pub bone1: String,
    /// The parent body.
    pub bone2: String,
    /// Frames in each body's bone space, positions in Unreal units.
    pub pos1: [f32; 3],
    pub pri1: [f32; 3],
    pub sec1: [f32; 3],
    pub pos2: [f32; 3],
    pub pri2: [f32; 3],
    pub sec2: [f32; 3],
    pub swing_limited: bool,
    pub twist_limited: bool,
    /// Degrees.
    pub swing1: f32,
    pub swing2: f32,
    pub twist: f32,
}

#[derive(Debug, Clone, Default)]
pub struct PhysicsAsset {
    pub bodies: Vec<BodySetup>,
    pub constraints: Vec<ConstraintSetup>,
}

/// The cooked Matrix struct stores each FPlane as (W, X, Y, Z).
fn matrix(p: &Prop, data: &[u8]) -> Matrix {
    let b = p.bytes(data);
    let f = |i: usize| b.get(i * 4..i * 4 + 4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).unwrap_or(0.0);
    let mut m = [[0.0; 4]; 4];
    for (r, row) in m.iter_mut().enumerate() {
        *row = [f(r * 4 + 1), f(r * 4 + 2), f(r * 4 + 3), f(r * 4)];
    }
    m
}

fn float(props: &[Prop], name: &str, default: f32) -> f32 {
    match find(props, name).map(|p| &p.value) {
        Some(Value::Float(v)) => *v,
        _ => default,
    }
}

fn boolean(props: &[Prop], name: &str) -> bool {
    matches!(find(props, name).map(|p| &p.value), Some(Value::Bool(true)))
}

fn name(props: &[Prop], n: &str) -> String {
    match find(props, n).map(|p| &p.value) {
        Some(Value::Name(s)) => s.clone(),
        _ => String::new(),
    }
}

fn vec3(props: &[Prop], data: &[u8], n: &str, default: [f32; 3]) -> [f32; 3] {
    find(props, n).and_then(|p| p.as_vec3(data)).unwrap_or(default)
}

/// Read the PhysicsAsset export `export` and its body and constraint setups.
pub fn read_physics_asset(pkg: &Package, export: usize) -> Result<PhysicsAsset, String> {
    let (props, _) = export_props(pkg, export).map_err(|e| format!("{e:?}"))?;
    let data = pkg.export_bytes(export);
    let refs = |n: &str| find(&props, n).map(|p| p.as_i32_array(data)).unwrap_or_default();
    let mut out = PhysicsAsset::default();
    for r in refs("BodySetup") {
        if r <= 0 {
            continue;
        }
        let e = (r - 1) as usize;
        let (bp, _) = export_props(pkg, e).map_err(|e| format!("{e:?}"))?;
        let d = pkg.export_bytes(e);
        let mut shapes = Vec::new();
        if let Some(agg) = find(&bp, "AggGeom") {
            let (inner, _) = read_props(pkg, d, agg.start).map_err(|e| format!("{e:?}"))?;
            for arr in &inner {
                for el in struct_array(pkg, d, arr) {
                    let tm = find(&el, "TM").map(|p| matrix(p, d)).unwrap_or([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
                    if boolean(&el, "bNoRBCollision") {
                        continue;
                    }
                    match arr.name.as_str() {
                        "SphereElems" => shapes.push(Shape::Sphere { tm, radius: float(&el, "Radius", 1.0) }),
                        "BoxElems" => shapes.push(Shape::Box { tm, x: float(&el, "X", 1.0), y: float(&el, "Y", 1.0), z: float(&el, "Z", 1.0) }),
                        "SphylElems" => shapes.push(Shape::Sphyl { tm, radius: float(&el, "Radius", 1.0), length: float(&el, "Length", 1.0) }),
                        _ => {}
                    }
                }
            }
        }
        out.bodies.push(BodySetup { bone: name(&bp, "BoneName"), shapes });
    }
    for r in refs("ConstraintSetup") {
        if r <= 0 {
            continue;
        }
        let e = (r - 1) as usize;
        let (cp, _) = export_props(pkg, e).map_err(|e| format!("{e:?}"))?;
        let d = pkg.export_bytes(e);
        let s = 1.0 / U2P_SCALE;
        let pos = |n: &str| {
            let v = vec3(&cp, d, n, [0.0; 3]);
            [v[0] * s, v[1] * s, v[2] * s]
        };
        out.constraints.push(ConstraintSetup {
            joint: name(&cp, "JointName"),
            bone1: name(&cp, "ConstraintBone1"),
            bone2: name(&cp, "ConstraintBone2"),
            pos1: pos("Pos1"),
            pri1: vec3(&cp, d, "PriAxis1", [1.0, 0.0, 0.0]),
            sec1: vec3(&cp, d, "SecAxis1", [0.0, 1.0, 0.0]),
            pos2: pos("Pos2"),
            pri2: vec3(&cp, d, "PriAxis2", [1.0, 0.0, 0.0]),
            sec2: vec3(&cp, d, "SecAxis2", [0.0, 1.0, 0.0]),
            swing_limited: boolean(&cp, "bSwingLimited"),
            twist_limited: boolean(&cp, "bTwistLimited"),
            swing1: float(&cp, "Swing1LimitAngle", 45.0),
            swing2: float(&cp, "Swing2LimitAngle", 45.0),
            twist: float(&cp, "TwistLimitAngle", 45.0),
        });
    }
    Ok(out)
}
