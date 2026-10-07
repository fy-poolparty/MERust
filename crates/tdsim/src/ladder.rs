//! TdLadderVolume (TdMovementVolume): the climbing positions of a ladder or pipe, built the way
//! the natives do from the volume's box and rotation.

use crate::math::{Rotator, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LadderType {
    #[default]
    Ladder = 0,
    Pipe = 1,
}

#[derive(Clone, Debug)]
pub struct LadderVolume {
    /// Actor location (the box centre) and rotation (X points into the wall).
    pub location: Vec3,
    pub rotation: Rotator,
    /// Box half extents in the volume's local frame.
    pub extent: Vec3,
    pub ladder_type: LadderType,
    pub can_exit_at_top: bool,
    pub step_height: f32,
    pub z_offset_ladder: f32,
    pub z_offset_pipe: f32,
    pub xy_offset_ladder: f32,
    pub xy_offset_pipe: f32,
    pub floor_normal: Vec3,
    pub wall_normal: Vec3,
    pub move_direction: Vec3,
    pub center: Vec3,
    pub pawn_ladder_locations: Vec<Vec3>,
    pub ladder_steps: Vec<Vec3>,
}

impl LadderVolume {
    /// A box volume `extent` (half size) around `location`, facing `yaw` into the wall.
    pub fn new(location: Vec3, extent: Vec3, yaw: i32, ladder_type: LadderType) -> Self {
        let rotation = Rotator::new(0, yaw, 0);
        let (x, _y, z) = rotation.axes();
        let mut v = LadderVolume {
            location,
            rotation,
            extent,
            ladder_type,
            can_exit_at_top: true,
            step_height: 32.0,
            z_offset_ladder: 0.0,
            z_offset_pipe: -5.0,
            xy_offset_ladder: -50.0,
            xy_offset_pipe: -62.0,
            // ATdMovementVolume (0x12A8FD0): Z axis is the floor normal and the climb direction,
            // the box centre is Center; ATdLadderVolume (0x12A9F60) flips the X axis into WallNormal.
            floor_normal: z,
            wall_normal: -x,
            move_direction: z,
            center: location,
            pawn_ladder_locations: Vec::new(),
            ladder_steps: Vec::new(),
        };
        v.build_steps();
        v
    }

    pub fn to_local(&self, p: Vec3) -> Vec3 {
        let (x, y, z) = self.rotation.axes();
        let d = p - self.location;
        Vec3::new(d.dot(x), d.dot(y), d.dot(z))
    }

    /// AVolume::Encompasses for the box.
    /// Does a box sweep (half size `ext`, world axes) from `start` to `end` touch the volume?
    /// Stands in for the MultiLineCheck against volume brushes; the sweep's box is taken as
    /// round in XY (the pawn's extent is square, so the rotated volume only grows it slightly).
    pub fn sweep_hits(&self, start: Vec3, end: Vec3, ext: Vec3) -> bool {
        let a = self.to_local(start);
        let b = self.to_local(end);
        let half = [self.extent.x + ext.x.max(ext.y), self.extent.y + ext.x.max(ext.y), self.extent.z + ext.z];
        let (pa, pb) = ([a.x, a.y, a.z], [b.x, b.y, b.z]);
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for i in 0..3 {
            let d = pb[i] - pa[i];
            if d.abs() < 1e-6 {
                if pa[i].abs() > half[i] {
                    return false;
                }
                continue;
            }
            let (mut lo, mut hi) = ((-half[i] - pa[i]) / d, (half[i] - pa[i]) / d);
            if lo > hi {
                std::mem::swap(&mut lo, &mut hi);
            }
            t0 = t0.max(lo);
            t1 = t1.min(hi);
            if t0 > t1 {
                return false;
            }
        }
        true
    }

    pub fn encompasses(&self, p: Vec3) -> bool {
        let l = self.to_local(p);
        l.x.abs() <= self.extent.x && l.y.abs() <= self.extent.y && l.z.abs() <= self.extent.z
    }

    /// sub_12A9170: the volume's boundary on the `dir` side of the line through `origin` (the
    /// origin is first pushed out along `dir`, then a line check comes back along -dir).
    fn boundary(&self, dir: Vec3, origin: Vec3) -> Vec3 {
        let mut o = origin;
        for _ in 0..64 {
            if !self.encompasses(o) {
                break;
            }
            o += dir * 500.0;
        }
        let p0 = self.to_local(o);
        let (x, y, z) = self.rotation.axes();
        let d = -dir;
        let dl = Vec3::new(d.dot(x), d.dot(y), d.dot(z));
        let (mut t0, mut t1) = (0.0f32, 10000.0f32);
        for (p, v, e) in [(p0.x, dl.x, self.extent.x), (p0.y, dl.y, self.extent.y), (p0.z, dl.z, self.extent.z)] {
            if v.abs() < 1e-8 {
                if p.abs() > e {
                    return Vec3::ZERO;
                }
            } else {
                let (a, b) = ((-e - p) / v, (e - p) / v);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
        }
        if t0 > t1 {
            return Vec3::ZERO;
        }
        o + d * t0
    }

    /// ATdLadderVolume (0x12AB620): one location per StepHeight up the wall-side face.
    fn build_steps(&mut self) {
        self.pawn_ladder_locations.clear();
        self.ladder_steps.clear();
        if self.step_height <= 0.0 {
            return;
        }
        let bottom = self.boundary(-self.move_direction, self.center);
        let above = bottom + self.move_direction * self.step_height;
        let base = self.boundary(self.wall_normal, above) - self.wall_normal * 64.0;
        let top = self.boundary(self.move_direction, base).z - self.move_direction.z * 96.0;
        let span = (top - base.z).min(10000.0);
        let steps = (span / self.step_height).trunc() * self.step_height;
        let mut p = Vec3::new(base.x, base.y, top - steps);
        while top >= p.z {
            self.ladder_steps.push(p);
            self.pawn_ladder_locations.push(p);
            p += self.move_direction * self.step_height;
        }
    }

    /// GetLadderLocation (0x12AB3B0).
    pub fn ladder_location(&self, index: i32) -> Vec3 {
        let n = self.pawn_ladder_locations.len() as i32;
        if n == 0 {
            return self.location;
        }
        let i = index.clamp(0, n - 1) as usize;
        let (xy, z) = match self.ladder_type {
            LadderType::Pipe => (self.xy_offset_pipe, self.z_offset_pipe),
            LadderType::Ladder => (self.xy_offset_ladder, self.z_offset_ladder),
        };
        self.pawn_ladder_locations[i] + self.move_direction * z - self.wall_normal * xy
    }

    /// GetClosestStep (0x12A9FE0).
    pub fn closest_step(&self, z: f32) -> i32 {
        let mut best = 0;
        let mut d = (self.ladder_location(0).z - z).abs();
        for i in 1..self.pawn_ladder_locations.len() as i32 {
            let di = (self.ladder_location(i).z - z).abs();
            if d > di {
                best = i;
                d = di;
            }
        }
        best
    }

    /// GetClosestStepUp (0x12AA080).
    pub fn closest_step_up(&self, z: f32) -> i32 {
        let n = self.pawn_ladder_locations.len() as i32;
        for i in 0..n {
            if self.ladder_location(i).z > z {
                return i;
            }
        }
        n - 1
    }

    /// GetClosestStepDown (0x12A88D0).
    pub fn closest_step_down(&self, z: f32) -> i32 {
        let mut i = self.last_step();
        while i >= 0 {
            if z > self.ladder_location(i).z {
                return i;
            }
            i -= 1;
        }
        0
    }

    /// GetLastStep (0x12AA0E0): pipes stop four steps short of the top.
    pub fn last_step(&self) -> i32 {
        let n = self.pawn_ladder_locations.len() as i32;
        match self.ladder_type {
            LadderType::Pipe => (n - 4).max(0),
            LadderType::Ladder => (n - 1).max(0),
        }
    }
}
