//! Unreal Engine 3 math as Mirror's Edge uses it: Unreal units, Z up, left-handed (X forward,
//! Y right), and 16-bit rotators (65536 = one turn).

pub use parry3d::math::Vec3;

pub const SMALL_NUMBER: f32 = 1e-8;
pub const KINDA_SMALL_NUMBER: f32 = 1e-4;
/// Unreal angle units per radian.
pub const URU_PER_RAD: f32 = 32768.0 / std::f32::consts::PI;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rotator {
    pub pitch: i32,
    pub yaw: i32,
    pub roll: i32,
}

impl Rotator {
    pub const ZERO: Rotator = Rotator { pitch: 0, yaw: 0, roll: 0 };

    pub const fn new(pitch: i32, yaw: i32, roll: i32) -> Self {
        Self { pitch, yaw, roll }
    }

    /// `FRotationMatrix(R)` rows: X (forward), Y (right), Z (up).
    pub fn axes(self) -> (Vec3, Vec3, Vec3) {
        let (sp, cp) = (self.pitch as f32 / URU_PER_RAD).sin_cos();
        let (sy, cy) = (self.yaw as f32 / URU_PER_RAD).sin_cos();
        let (sr, cr) = (self.roll as f32 / URU_PER_RAD).sin_cos();
        let x = Vec3::new(cp * cy, cp * sy, sp);
        let y = Vec3::new(sr * sp * cy - cr * sy, sr * sp * sy + cr * cy, -sr * cp);
        let z = Vec3::new(-(cr * sp * cy + sr * sy), cy * sr - cr * sp * sy, cr * cp);
        (x, y, z)
    }

    /// `Vector(R)`.
    pub fn vector(self) -> Vec3 {
        self.axes().0
    }

    /// `Rotator(V)`: yaw and pitch from a direction, roll zero (truncated like `appTrunc`).
    pub fn from_vector(v: Vec3) -> Self {
        let yaw = (v.y.atan2(v.x) * URU_PER_RAD) as i32;
        let pitch = (v.z.atan2((v.x * v.x + v.y * v.y).sqrt()) * URU_PER_RAD) as i32;
        Self { pitch, yaw, roll: 0 }
    }

    /// `Normalize(R)`: each axis into [-32768, 32767].
    pub fn normalize(self) -> Self {
        Self { pitch: norm_axis(self.pitch), yaw: norm_axis(self.yaw), roll: norm_axis(self.roll) }
    }
}

impl std::ops::Add for Rotator {
    type Output = Rotator;
    fn add(self, o: Rotator) -> Rotator {
        Rotator::new(self.pitch + o.pitch, self.yaw + o.yaw, self.roll + o.roll)
    }
}
impl std::ops::Sub for Rotator {
    type Output = Rotator;
    fn sub(self, o: Rotator) -> Rotator {
        Rotator::new(self.pitch - o.pitch, self.yaw - o.yaw, self.roll - o.roll)
    }
}
impl std::ops::Mul<f32> for Rotator {
    type Output = Rotator;
    /// `Rotator * float` truncates each axis like UnrealScript.
    fn mul(self, s: f32) -> Rotator {
        Rotator::new((self.pitch as f32 * s) as i32, (self.yaw as f32 * s) as i32, (self.roll as f32 * s) as i32)
    }
}

/// `NormalizeRotAxis`.
pub fn norm_axis(a: i32) -> i32 {
    let a = a & 0xFFFF;
    if a > 32767 { a - 65536 } else { a }
}

pub trait UeVec {
    fn safe_normal(self) -> Vec3;
    fn safe_normal_2d(self) -> Vec3;
    fn size_2d(self) -> f32;
    fn size_sq_2d(self) -> f32;
    fn is_nearly_zero(self) -> bool;
    fn is_zero(self) -> bool;
}

impl UeVec for Vec3 {
    /// `FVector::SafeNormal()`.
    fn safe_normal(self) -> Vec3 {
        let sq = self.x * self.x + self.y * self.y + self.z * self.z;
        if sq == 1.0 {
            self
        } else if sq < SMALL_NUMBER {
            Vec3::ZERO
        } else {
            self * (1.0 / sq.sqrt())
        }
    }
    /// `FVector::SafeNormal2D()`.
    fn safe_normal_2d(self) -> Vec3 {
        let sq = self.x * self.x + self.y * self.y;
        if sq == 1.0 {
            Vec3::new(self.x, self.y, 0.0)
        } else if sq < SMALL_NUMBER {
            Vec3::ZERO
        } else {
            let s = 1.0 / sq.sqrt();
            Vec3::new(self.x * s, self.y * s, 0.0)
        }
    }
    fn size_2d(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
    fn size_sq_2d(self) -> f32 {
        self.x * self.x + self.y * self.y
    }
    /// `FVector::IsNearlyZero()` (per component, KINDA_SMALL_NUMBER).
    fn is_nearly_zero(self) -> bool {
        self.x.abs() < KINDA_SMALL_NUMBER && self.y.abs() < KINDA_SMALL_NUMBER && self.z.abs() < KINDA_SMALL_NUMBER
    }
    fn is_zero(self) -> bool {
        self.x == 0.0 && self.y == 0.0 && self.z == 0.0
    }
}

/// UnrealScript `VSize2D`.
pub fn vsize2d(v: Vec3) -> f32 {
    v.size_2d()
}

/// `FInterpCurveFloat` with the point modes Mirror's Edge uses (linear, constant, curve).
#[derive(Clone, Debug, Default)]
pub struct InterpCurve {
    pub points: Vec<CurvePoint>,
}

#[derive(Clone, Copy, Debug)]
pub struct CurvePoint {
    pub in_val: f32,
    pub out_val: f32,
    pub arrive: f32,
    pub leave: f32,
    pub mode: CurveMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveMode {
    Linear,
    Curve,
    Constant,
}

impl InterpCurve {
    pub fn linear(points: &[(f32, f32)]) -> Self {
        Self {
            points: points
                .iter()
                .map(|&(i, o)| CurvePoint { in_val: i, out_val: o, arrive: 0.0, leave: 0.0, mode: CurveMode::Linear })
                .collect(),
        }
    }

    /// `FInterpCurve::AddPoint` keeps points sorted by input.
    pub fn add_point(&mut self, in_val: f32, out_val: f32) {
        let i = self.points.iter().position(|p| p.in_val >= in_val).unwrap_or(self.points.len());
        self.points.insert(i, CurvePoint { in_val, out_val, arrive: 0.0, leave: 0.0, mode: CurveMode::Linear });
    }

    /// `FInterpCurve::Eval` (IMT_UseFixedTangentEval).
    pub fn eval(&self, x: f32, default: f32) -> f32 {
        let p = &self.points;
        let n = p.len();
        if n == 0 {
            return default;
        }
        if n < 2 || x <= p[0].in_val {
            return p[0].out_val;
        }
        if x >= p[n - 1].in_val {
            return p[n - 1].out_val;
        }
        let mut i = 1;
        while i < n && p[i].in_val <= x {
            i += 1;
        }
        let a = p[i - 1];
        let b = p[i];
        let diff = b.in_val - a.in_val;
        if diff <= 0.0 || a.mode == CurveMode::Constant {
            return a.out_val;
        }
        let t = (x - a.in_val) / diff;
        match a.mode {
            CurveMode::Linear => a.out_val + (b.out_val - a.out_val) * t,
            _ => {
                // Hermite with fixed tangents scaled by the segment length.
                let (t2, t3) = (t * t, t * t * t);
                let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
                let h10 = t3 - 2.0 * t2 + t;
                let h01 = -2.0 * t3 + 3.0 * t2;
                let h11 = t3 - t2;
                h00 * a.out_val + h10 * a.leave * diff + h01 * b.out_val + h11 * b.arrive * diff
            }
        }
    }
}
