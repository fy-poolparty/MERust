//! The death ragdoll (TdBotPawn.PlayDeathAnim -> EnableRagdoll / FullBodyRagdoll): the
//! PhysicsAsset's rigid bodies and joints, simulated in Unreal space (uu, Z up) with a small
//! XPBD solver (PhysX isn't portable) against the level's collision.
//!
//! Faithful to the asset: per-bone shapes (spheres, boxes), joint frames, swing/twist limits;
//! to the script: bodies stay fixed to the death animation until TimeToEnableRagdoll, then
//! RagdollBones go limp with angular motors towards the animation (MotorStrength, blended out
//! from TimeToBlendOutMotors to TimeToDisableMotors), the rest at TimeToFullRagdoll, the bone
//! impulse at TimeToBoneImpulse. Approximated: masses (shape volume), the motor stiffness
//! mapping, collision as spheres swept with box traces, contact friction.

use glam::{Quat, Vec3};
use tdsim::bots::DeathAnim;
use upk::physics::{PhysicsAsset, Shape};

const SUBSTEPS: usize = 8;
/// The motors' damping / spring (both MotorStrength): the drive's time constant, s.
const MOTOR_TIME_CONSTANT: f32 = 1.0;
/// Male3p_PhysMat (the cops' PhysicalMaterial): Friction 1.1, AngularDamping 2.0; LinearDamping
/// is the PhysicalMaterial default 0.01.
const FRICTION: f32 = 1.1;
/// The most a contact pushes a body out per substep (uu).
const MAX_DEPENETRATION: f32 = 3.0;
/// PhysX 2.x's default rigid body max angular velocity (rad/s).
const MAX_ANGULAR_VELOCITY: f32 = 7.0;
const LINEAR_DAMPING: f32 = 0.01;
const ANGULAR_DAMPING: f32 = 2.0;

/// TdBotPawn.RagdollBones: unfixed by EnableRagdoll (the rest wait for FullBodyRagdoll).
const RAGDOLL_BONES: [&str; 16] = [
    "Spine2", "Neck", "LeftShoulder", "LeftArm", "LeftForeArm", "LeftHand", "RightShoulder", "RightArm", "RightForeArm", "RightHand", "LeftUpLeg", "LeftLeg", "LeftFoot",
    "RightUpLeg", "RightLeg", "RightFoot",
];

#[derive(Clone, Copy, Debug)]
pub struct Xform {
    pub pos: Vec3,
    pub rot: Quat,
}

impl Xform {
    pub fn mul(&self, o: &Xform) -> Xform {
        Xform { pos: self.pos + self.rot * o.pos, rot: (self.rot * o.rot).normalize() }
    }
    pub fn inverse(&self) -> Xform {
        let r = self.rot.inverse();
        Xform { pos: -(r * self.pos), rot: r }
    }
    pub fn point(&self, p: Vec3) -> Vec3 {
        self.pos + self.rot * p
    }
}

struct Body {
    bone: usize,
    name: String,
    /// Centre of mass in the bone frame.
    com: Vec3,
    inv_mass: f32,
    /// Principal inverse inertia in the bone frame.
    inv_inertia: Vec3,
    /// World centre of mass and orientation (= the bone's).
    x: Vec3,
    q: Quat,
    v: Vec3,
    w: Vec3,
    x_prev: Vec3,
    q_prev: Quat,
    fixed: bool,
    motor: bool,
    /// This substep's contact normal (summed), if it touched the world.
    contact: Vec3,
    /// Collision spheres (bone frame centre, radius).
    spheres: Vec<(Vec3, f32)>,
}

struct Joint {
    child: usize,
    parent: usize,
    /// Anchors in each body's bone frame.
    pos1: Vec3,
    pos2: Vec3,
    /// Joint frames (primary = twist axis, secondary) in each bone frame.
    pri1: Vec3,
    sec1: Vec3,
    pri2: Vec3,
    sec2: Vec3,
    swing1: f32,
    swing2: f32,
    twist: f32,
}

pub struct Ragdoll {
    bodies: Vec<Body>,
    joints: Vec<Joint>,
    pub time: f32,
    death: DeathAnim,
    motor_scale: f32,
    enabled: bool,
    full: bool,
    impulse_done: bool,
    hit: (Vec3, Vec3),
    gravity: f32,
    /// The animated pose at the previous step (fixed bodies move along it over the substeps).
    last_anim: Vec<Xform>,
}

fn v(a: [f32; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn tv(a: Vec3) -> tdsim::Vec3 {
    tdsim::Vec3::new(a.x, a.y, a.z)
}

/// Rotation (rows X, Y, Z axes) to a quaternion.
fn rows_quat(m: &[[f32; 4]; 4]) -> Quat {
    let x = Vec3::new(m[0][0], m[0][1], m[0][2]);
    let y = Vec3::new(m[1][0], m[1][1], m[1][2]);
    let z = Vec3::new(m[2][0], m[2][1], m[2][2]);
    Quat::from_mat3(&glam::Mat3::from_cols(x, y, z)).normalize()
}

impl Ragdoll {
    /// Bodies at the current (animated) bone transforms `world` (Unreal space per skeleton
    /// bone), moving with `vel` (per bone, uu/s).
    pub fn new(asset: &PhysicsAsset, bone_index: impl Fn(&str) -> Option<usize>, world: &[Xform], vel: &[Vec3], death: DeathAnim, hit: (Vec3, Vec3), gravity: f32) -> Ragdoll {
        let mut bodies = Vec::new();
        for bs in &asset.bodies {
            let Some(bone) = bone_index(&bs.bone) else { continue };
            let mut spheres = Vec::new();
            let mut volume = 0.0;
            let mut com = Vec3::ZERO;
            let mut ext = Vec3::splat(1.0);
            for s in &bs.shapes {
                match s {
                    Shape::Sphere { tm, radius } => {
                        let c = Vec3::new(tm[3][0], tm[3][1], tm[3][2]);
                        spheres.push((c, *radius));
                        let vol = 4.18879 * radius * radius * radius;
                        com += c * vol;
                        volume += vol;
                        ext = ext.max(Vec3::splat(*radius));
                    }
                    Shape::Box { tm, x, y, z } => {
                        let c = Vec3::new(tm[3][0], tm[3][1], tm[3][2]);
                        let r = rows_quat(tm);
                        let half = Vec3::new(x * 0.5, y * 0.5, z * 0.5);
                        let vol = x * y * z;
                        com += c * vol;
                        volume += vol;
                        // spheres along the long axis
                        let axes = [r * Vec3::X, r * Vec3::Y, r * Vec3::Z];
                        let h = [half.x, half.y, half.z];
                        let long = (0..3).max_by(|a, b| h[*a].total_cmp(&h[*b])).unwrap();
                        let rad = (0..3).filter(|k| *k != long).map(|k| h[k]).fold(f32::MAX, f32::min);
                        let span = (h[long] - rad).max(0.0);
                        if span < 1.0 {
                            spheres.push((c, rad));
                        } else {
                            spheres.push((c + axes[long] * span, rad));
                            spheres.push((c - axes[long] * span, rad));
                        }
                        ext = ext.max(half);
                    }
                    Shape::Sphyl { tm, radius, length } => {
                        let c = Vec3::new(tm[3][0], tm[3][1], tm[3][2]);
                        let r = rows_quat(tm);
                        let axis = r * Vec3::Z;
                        spheres.push((c + axis * length * 0.5, *radius));
                        spheres.push((c - axis * length * 0.5, *radius));
                        let vol = 3.14159 * radius * radius * (length + radius * 4.0 / 3.0);
                        com += c * vol;
                        volume += vol;
                        ext = ext.max(Vec3::new(*radius, *radius, length * 0.5 + radius));
                    }
                }
            }
            if volume <= 0.0 {
                continue;
            }
            com /= volume;
            // mass from the volume (kg-ish), box inertia over the shapes' extent
            let mass = (volume * 0.001).max(0.5);
            let e2 = ext * ext * 4.0;
            let inertia = Vec3::new(e2.y + e2.z, e2.x + e2.z, e2.x + e2.y) * (mass / 12.0);
            let t = world[bone];
            let x = t.point(com);
            bodies.push(Body {
                bone,
                name: bs.bone.clone(),
                com,
                inv_mass: 1.0 / mass,
                inv_inertia: Vec3::ONE / inertia,
                x,
                q: t.rot,
                v: vel[bone],
                w: Vec3::ZERO,
                x_prev: x,
                q_prev: t.rot,
                fixed: true,
                motor: false,
                contact: Vec3::ZERO,
                spheres,
            });
        }
        let body_of = |name: &str| bodies.iter().position(|b| b.name.eq_ignore_ascii_case(name));
        let mut joints = Vec::new();
        for c in &asset.constraints {
            let (Some(child), Some(parent)) = (body_of(&c.bone1), body_of(&c.bone2)) else { continue };
            let rad = |d: f32| d.to_radians().max(0.01);
            joints.push(Joint {
                child,
                parent,
                pos1: v(c.pos1),
                pos2: v(c.pos2),
                pri1: v(c.pri1).normalize_or(Vec3::X),
                sec1: v(c.sec1).normalize_or(Vec3::Y),
                pri2: v(c.pri2).normalize_or(Vec3::X),
                sec2: v(c.sec2).normalize_or(Vec3::Y),
                swing1: if c.swing_limited { rad(c.swing1) } else { std::f32::consts::PI },
                swing2: if c.swing_limited { rad(c.swing2) } else { std::f32::consts::PI },
                twist: if c.twist_limited { rad(c.twist) } else { std::f32::consts::PI },
            });
        }
        Ragdoll { bodies, joints, time: 0.0, death, motor_scale: 1.0, enabled: false, full: false, impulse_done: false, hit, gravity, last_anim: world.to_vec() }
    }

    fn inv_inertia_world(b: &Body, q: Quat) -> impl Fn(Vec3) -> Vec3 + '_ {
        move |t: Vec3| {
            let local = q.inverse() * t;
            q * (local * b.inv_inertia)
        }
    }

    /// Advance by `dt`: the death-anim timeline, then the solver. `anim` is the animated pose
    /// (Unreal space per bone) the fixed bodies follow and the motors drive towards.
    pub fn step(&mut self, dt: f32, anim: &[Xform], world: &tdsim::collision::World, owner_velocity: Vec3) {
        let d = self.death;
        let prev_time = self.time;
        self.time += dt;
        let t = self.time;
        // EnableRagdoll / FullBodyRagdoll / motors / bone impulse
        if !self.enabled && t >= d.time_to_enable_ragdoll {
            self.enabled = true;
            for b in self.bodies.iter_mut() {
                if RAGDOLL_BONES.iter().any(|n| n.eq_ignore_ascii_case(&b.name)) {
                    b.fixed = false;
                    b.motor = d.use_motors;
                    // URB_BodyInstance::SetFixed(false): the body takes the owner's velocity
                    b.v = owner_velocity;
                    b.w = Vec3::ZERO;
                }
            }
        }
        if !self.full && t >= d.time_to_full_ragdoll {
            self.full = true;
            for b in self.bodies.iter_mut() {
                if b.fixed {
                    b.fixed = false;
                    b.v = owner_velocity;
                    b.w = Vec3::ZERO;
                }
            }
        }
        if d.use_motors {
            // StartBlendOutMotors at TimeToBlendOutMotors, TurnOffMotors at TimeToDisableMotors
            let blend = d.time_to_disable_motors - d.time_to_blend_out_motors;
            self.motor_scale = if t < d.time_to_blend_out_motors {
                1.0
            } else if blend > 0.0 && t < d.time_to_disable_motors {
                1.0 - (t - d.time_to_blend_out_motors) / blend
            } else {
                0.0
            };
            if t >= d.time_to_disable_motors {
                for b in self.bodies.iter_mut() {
                    b.motor = false;
                }
            }
        }
        if !self.impulse_done && d.bone_impulse > 0.0 && t >= d.time_to_bone_impulse && prev_time <= d.time_to_bone_impulse.max(prev_time) {
            self.impulse_done = true;
            // GiveBoneImpulse: along the killing hit at the body nearest the hit location
            let (loc, mom) = self.hit;
            let dir = mom.normalize_or_zero();
            if dir != Vec3::ZERO {
                if let Some(b) = self.bodies.iter_mut().filter(|b| !b.fixed).min_by(|a, b| (a.x - loc).length_squared().total_cmp(&(b.x - loc).length_squared())) {
                    // Mesh.AddImpulse(Normal(Momentum) * BoneImpulse): a momentum, so the
                    // velocity change is over the body's mass
                    b.v += dir * d.bone_impulse * b.inv_mass;
                }
            }
        }
        let h = dt / SUBSTEPS as f32;
        let g = Vec3::new(0.0, 0.0, self.gravity);
        if self.last_anim.len() != anim.len() {
            self.last_anim = anim.to_vec();
        }
        for step in 0..SUBSTEPS {
            let f = (step + 1) as f32 / SUBSTEPS as f32;
            // predict
            for b in self.bodies.iter_mut() {
                b.x_prev = b.x;
                b.q_prev = b.q;
                b.contact = Vec3::ZERO;
                if b.fixed {
                    let (a0, a1) = (self.last_anim[b.bone], anim[b.bone]);
                    let a = Xform { pos: a0.pos.lerp(a1.pos, f), rot: a0.rot.slerp(a1.rot, f) };
                    b.x = a.point(b.com);
                    b.q = a.rot;
                    continue;
                }
                b.v += g * h;
                b.x += b.v * h;
                let dq = Quat::from_xyzw(b.w.x, b.w.y, b.w.z, 0.0) * b.q;
                b.q = Quat::from_xyzw(b.q.x + 0.5 * h * dq.x, b.q.y + 0.5 * h * dq.y, b.q.z + 0.5 * h * dq.z, b.q.w + 0.5 * h * dq.w).normalize();
            }
            for j in 0..self.joints.len() {
                self.solve_joint(j, h);
            }
            if self.motor_scale > 0.0 {
                for j in 0..self.joints.len() {
                    self.solve_motor(j, h, anim);
                }
            }
            self.solve_contacts(world);
            // velocities
            for b in self.bodies.iter_mut() {
                b.v = (b.x - b.x_prev) / h;
                let dq = b.q * b.q_prev.inverse();
                let w = Vec3::new(dq.x, dq.y, dq.z) * (2.0 / h);
                b.w = if dq.w < 0.0 { -w } else { w };
                if !b.fixed {
                    b.v *= 1.0 - (LINEAR_DAMPING * h).min(1.0);
                    b.w *= 1.0 - (ANGULAR_DAMPING * h).min(1.0);
                    let wl = b.w.length();
                    if wl > MAX_ANGULAR_VELOCITY {
                        b.w *= MAX_ANGULAR_VELOCITY / wl;
                    }
                    // contacts are inelastic: pushing out of the world adds no outward speed
                    // (no restitution), and friction slows the slide
                    let n = b.contact.normalize_or_zero();
                    if n != Vec3::ZERO {
                        let vn = b.v.dot(n);
                        if vn > 0.0 {
                            b.v -= n * vn;
                        }
                        let vt = b.v - n * b.v.dot(n);
                        b.v -= vt * FRICTION.min(1.0) * (h * 30.0).min(1.0);
                        b.w *= 1.0 - (h * 10.0).min(1.0);
                    }
                }
            }
        }
        self.last_anim = anim.to_vec();
    }

    fn apply_positional(&mut self, a: usize, b: usize, ra: Vec3, rb: Vec3, corr: Vec3, compliance: f32) {
        let c = corr.length();
        if c < 1e-6 {
            return;
        }
        let n = corr / c;
        let wa = self.gen_inv_mass(a, ra, n);
        let wb = self.gen_inv_mass(b, rb, n);
        let w = wa + wb + compliance;
        if w < 1e-9 {
            return;
        }
        let p = n * (c / w);
        self.apply_impulse(a, ra, p);
        self.apply_impulse(b, rb, -p);
    }

    fn gen_inv_mass(&self, i: usize, r: Vec3, n: Vec3) -> f32 {
        let b = &self.bodies[i];
        if b.fixed {
            return 0.0;
        }
        let rn = r.cross(n);
        b.inv_mass + rn.dot(Self::inv_inertia_world(b, b.q)(rn))
    }

    fn apply_impulse(&mut self, i: usize, r: Vec3, p: Vec3) {
        let b = &mut self.bodies[i];
        if b.fixed {
            return;
        }
        b.x += p * b.inv_mass;
        let dw = {
            let local = b.q.inverse() * r.cross(p);
            b.q * (local * b.inv_inertia)
        };
        let dq = Quat::from_xyzw(dw.x, dw.y, dw.z, 0.0) * b.q;
        b.q = Quat::from_xyzw(b.q.x + 0.5 * dq.x, b.q.y + 0.5 * dq.y, b.q.z + 0.5 * dq.z, b.q.w + 0.5 * dq.w).normalize();
    }

    /// Rotate body `a` by `angle` about `axis` relative to `b` (split by inverse inertia).
    fn apply_angular(&mut self, a: usize, b: usize, axis: Vec3, angle: f32, compliance: f32) {
        let wa = if self.bodies[a].fixed { 0.0 } else { axis.dot(Self::inv_inertia_world(&self.bodies[a], self.bodies[a].q)(axis)) };
        let wb = if self.bodies[b].fixed { 0.0 } else { axis.dot(Self::inv_inertia_world(&self.bodies[b], self.bodies[b].q)(axis)) };
        let w = wa + wb + compliance;
        if w < 1e-12 {
            return;
        }
        let p = axis * (angle / w);
        for (i, s) in [(a, 1.0f32), (b, -1.0)] {
            let body = &mut self.bodies[i];
            if body.fixed {
                continue;
            }
            let dw = {
                let local = body.q.inverse() * (p * s);
                body.q * (local * body.inv_inertia)
            };
            let dq = Quat::from_xyzw(dw.x, dw.y, dw.z, 0.0) * body.q;
            body.q = Quat::from_xyzw(body.q.x + 0.5 * dq.x, body.q.y + 0.5 * dq.y, body.q.z + 0.5 * dq.z, body.q.w + 0.5 * dq.w).normalize();
        }
    }

    fn solve_joint(&mut self, j: usize, _h: f32) {
        let (c, p) = (self.joints[j].child, self.joints[j].parent);
        if self.bodies[c].fixed && self.bodies[p].fixed {
            return;
        }
        // the anchors coincide
        {
            let jt = &self.joints[j];
            let (bc, bp) = (&self.bodies[c], &self.bodies[p]);
            let ac = bc.q * (jt.pos1 - bc.com);
            let ap = bp.q * (jt.pos2 - bp.com);
            let wc = bc.x + ac;
            let wp = bp.x + ap;
            let corr = wp - wc;
            self.apply_positional(c, p, ac, ap, corr, 0.0);
        }
        // swing: the child's twist axis within the elliptical cone around the parent's
        {
            let jt = &self.joints[j];
            let (bc, bp) = (&self.bodies[c], &self.bodies[p]);
            let a1 = bc.q * jt.pri1;
            let a2 = bp.q * jt.pri2;
            let s2 = bp.q * jt.sec2;
            let t2 = a2.cross(s2);
            let cos = a1.dot(a2).clamp(-1.0, 1.0);
            let theta = cos.acos();
            let (sx, ty) = (a1.dot(s2), a1.dot(t2));
            let phi = ty.atan2(sx);
            let (s1l, s2l) = (jt.swing1, jt.swing2);
            let lim = 1.0 / ((phi.cos() / s1l).powi(2) + (phi.sin() / s2l).powi(2)).sqrt();
            if theta > lim {
                let axis = a1.cross(a2);
                if axis.length_squared() > 1e-12 {
                    let axis = axis.normalize();
                    self.apply_angular(c, p, axis, theta - lim, 0.0);
                }
            }
        }
        // twist about the (averaged) twist axis
        {
            let jt = &self.joints[j];
            let (bc, bp) = (&self.bodies[c], &self.bodies[p]);
            let a1 = bc.q * jt.pri1;
            let a2 = bp.q * jt.pri2;
            let n = (a1 + a2).normalize_or_zero();
            if n == Vec3::ZERO {
                return;
            }
            let s1 = bc.q * jt.sec1;
            let s2 = bp.q * jt.sec2;
            let s1p = (s1 - n * s1.dot(n)).normalize_or_zero();
            let s2p = (s2 - n * s2.dot(n)).normalize_or_zero();
            let psi = s2p.cross(s1p).dot(n).atan2(s2p.dot(s1p));
            let lim = jt.twist;
            if psi.abs() > lim {
                let over = psi - psi.clamp(-lim, lim);
                self.apply_angular(c, p, n, -over, 0.0);
            }
        }
    }

    /// The angular drive towards the animated relative orientation (SetNamedMotorsAngularPositionDrive).
    fn solve_motor(&mut self, j: usize, h: f32, anim: &[Xform]) {
        let (c, p) = (self.joints[j].child, self.joints[j].parent);
        if !self.bodies[c].motor || self.bodies[c].fixed {
            return;
        }
        // SetAllMotorsAngularDriveParams(MotorStrength, MotorStrength, 0) and
        // SetAngularDriveScale(s, s): spring = damping, so the PhysX drive is heavily
        // overdamped and the joint closes on the animated orientation at error / (damping /
        // spring), a 1 s time constant whatever the strength (the scale only switches it off).
        // As a damper it also holds the joint against everything else meanwhile.
        if self.death.motor_strength * self.motor_scale <= 0.0 {
            return;
        }
        let (bc, bp) = (&self.bodies[c], &self.bodies[p]);
        let rel_anim = anim[bp.bone].rot.inverse() * anim[bc.bone].rot;
        let rel_now = bp.q.inverse() * bc.q;
        let rel_want = rel_now.slerp(rel_anim, (h / MOTOR_TIME_CONSTANT).min(1.0));
        let target = bp.q * rel_want;
        let mut dq = target * bc.q.inverse();
        if dq.w < 0.0 {
            dq = -dq;
        }
        let axis = Vec3::new(dq.x, dq.y, dq.z);
        let s = axis.length();
        if s < 1e-6 {
            return;
        }
        let angle = 2.0 * s.atan2(dq.w);
        self.apply_angular(c, p, axis / s, angle, 0.0);
    }

    /// The collision spheres against the world: swept from last position, pushed out along the
    /// hit normal, with friction.
    fn solve_contacts(&mut self, world: &tdsim::collision::World) {
        for i in 0..self.bodies.len() {
            if self.bodies[i].fixed {
                continue;
            }
            for k in 0..self.bodies[i].spheres.len() {
                let b = &self.bodies[i];
                let (c, r) = b.spheres[k];
                let p0 = b.x_prev + b.q_prev * (c - b.com);
                let ra = b.q * (c - b.com);
                let p1 = b.x + ra;
                let ext = tdsim::Vec3::new(r * 0.8, r * 0.8, r * 0.8);
                let hit = world.line_check(tv(p1), tv(p0), ext);
                if !hit.hit {
                    continue;
                }
                let n = Vec3::new(hit.normal.x, hit.normal.y, hit.normal.z);
                let stop = p0 + (p1 - p0) * hit.time;
                let pen = (stop - p1).dot(n);
                if pen <= 0.0 {
                    continue;
                }
                // normal push (capped, like PhysX's max depenetration) plus friction against
                // the tangential slide, as a point correction against the static world
                let slide = (p1 - p0) - n * (p1 - p0).dot(n);
                let corr = n * pen.min(MAX_DEPENETRATION) - slide * FRICTION.min(1.0);
                let len = corr.length();
                if len > 1e-6 {
                    let dir = corr / len;
                    let w = self.gen_inv_mass(i, ra, dir);
                    if w > 1e-9 {
                        self.apply_impulse(i, ra, dir * (len / w));
                    }
                }
                self.bodies[i].contact += n;
            }
        }
    }

    /// The bone transforms (Unreal space) of the simulated bodies.
    pub fn body_transforms(&self) -> impl Iterator<Item = (usize, Xform)> + '_ {
        self.bodies.iter().map(|b| (b.bone, Xform { pos: b.x - b.q * b.com, rot: b.q }))
    }
}

/// glTF/Bevy world (meters, Y up) -> Unreal (uu, Z up): swap Y/Z, the rotation conjugated by
/// the swap ((x, z, y, -w)).
pub fn to_ue(m: glam::Mat4) -> Xform {
    let (_, r, t) = m.to_scale_rotation_translation();
    Xform { pos: Vec3::new(t.x, t.z, t.y) * 100.0, rot: Quat::from_xyzw(r.x, r.z, r.y, -r.w).normalize() }
}

pub fn from_ue(x: &Xform) -> glam::Mat4 {
    let r = Quat::from_xyzw(x.rot.x, x.rot.z, x.rot.y, -x.rot.w).normalize();
    glam::Mat4::from_rotation_translation(r, Vec3::new(x.pos.x, x.pos.z, x.pos.y) * 0.01)
}

impl Ragdoll {
    /// One frame for a posed mesh: step against the animated pose (`m` = the mesh-to-world
    /// now), then write the result into `pose` as locals under the root frozen at `m0`
    /// (bodies from the simulation, the rest along their animated locals, the root kept in
    /// its animated relation to the hips).
    pub fn drive(&mut self, pose: &mut crate::pose::PoseEvaluator, m: glam::Mat4, m0: glam::Mat4, dt: f32, world: &tdsim::collision::World, owner_velocity: Vec3) {
        let globals = pose.globals();
        let anim: Vec<Xform> = globals.iter().map(|g| to_ue(m * *g)).collect();
        self.step(dt, &anim, world, owner_velocity);
        let n = globals.len();
        let mut wm: Vec<Option<glam::Mat4>> = vec![None; n];
        for (b, x) in self.body_transforms() {
            wm[b] = Some(from_ue(&x));
        }
        let parents = pose.parents.clone();
        if wm[0].is_none() {
            wm[0] = Some(match pose.bone_index("Hips").filter(|&h| wm[h].is_some()) {
                Some(h) => wm[h].unwrap() * globals[h].inverse() * globals[0],
                None => m * globals[0],
            });
        }
        for i in 1..n {
            if wm[i].is_none() {
                let local = globals[parents[i]].inverse() * globals[i];
                wm[i] = Some(wm[parents[i]].unwrap() * local);
            }
        }
        let inv = m0.inverse();
        for i in 0..n {
            let g = inv * wm[i].unwrap();
            let local = if i == 0 { g } else { (inv * wm[parents[i]].unwrap()).inverse() * g };
            let (_, rot, pos) = local.to_scale_rotation_translation();
            pose.pose[i].pos = pos;
            pose.pose[i].rot = rot;
        }
    }

    /// The lowest and highest body (centre of mass) heights, for checks.
    pub fn height_range(&self) -> (f32, f32) {
        self.bodies.iter().fold((f32::MAX, f32::MIN), |(lo, hi), b| (lo.min(b.x.z), hi.max(b.x.z)))
    }
}

impl Ragdoll {
    /// (bone, centre height, speed, fixed) per body, for checks.
    /// Per joint (child body name): the swing angle of its twist axis off the parent's, and
    /// the twist, in degrees (diagnostics).
    pub fn debug_joints(&self) -> Vec<(String, f32, f32)> {
        self.joints
            .iter()
            .map(|jt| {
                let (bc, bp) = (&self.bodies[jt.child], &self.bodies[jt.parent]);
                let a1 = bc.q * jt.pri1;
                let a2 = bp.q * jt.pri2;
                let theta = a1.dot(a2).clamp(-1.0, 1.0).acos().to_degrees();
                let n = (a1 + a2).normalize_or_zero();
                let s1 = bc.q * jt.sec1;
                let s2 = bp.q * jt.sec2;
                let s1p = (s1 - n * s1.dot(n)).normalize_or_zero();
                let s2p = (s2 - n * s2.dot(n)).normalize_or_zero();
                let psi = s2p.cross(s1p).dot(n).atan2(s2p.dot(s1p)).to_degrees();
                (bc.name.clone(), theta, psi)
            })
            .collect()
    }

    pub fn debug_bodies(&self) -> Vec<(String, f32, f32, bool)> {
        self.bodies.iter().map(|b| (b.name.clone(), b.x.z, b.v.length(), b.fixed)).collect()
    }
}
