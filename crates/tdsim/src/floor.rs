//! Edge natives of ATdPlayerPawn's physics: CheckValidFloor (vt293, slide off a ledge the
//! cylinder only just rests on) with its edge probe 0x12B5EB0, and physFalling's ceiling corner
//! probe 0x12B74E0 (bumping the head on a ledge's underside nudges the pawn out from under it).

use crate::collision::CheckResult;
use crate::math::{UeVec, Vec3};
use crate::sim::Sim;

/// FVector::SafeNormal2D as inlined here.
fn n2d(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, 0.0).safe_normal()
}

/// How far a box of half-width `r` reaches along a flat unit vector (the 0.828 = 2(√2 - 1)
/// blend between the round cylinder and its square-ish corner) plus a 2 uu margin.
fn corner_reach(n: Vec3, r: f32) -> f32 {
    let m = n.x.abs().max(n.y.abs());
    let m2 = (m * m).min(1.0);
    ((1.0 - m2).sqrt() * m * 0.828_427 + 1.0) * r + 2.0
}

impl Sim {
    fn wall_hit(&self, c: &CheckResult) -> bool {
        c.time < 1.0 && c.time > 0.0 && self.pawn.walkable_floor_z > c.normal.z
    }

    /// 0x12B5EB0: two horizontal traces 4 uu under `feet` across `offset` (each way). A wall face
    /// (or a thin, < 20 uu, ridge with faces both ways) means the cylinder hangs over an edge:
    /// returns the push that clears it.
    fn valid_floor_probe(&self, feet: Vec3, offset: Vec3) -> Option<Vec3> {
        let p = Vec3::new(feet.x, feet.y, feet.z - 4.0);
        let ext = offset + offset.safe_normal() * 10.0;
        let a = p + ext;
        let b = p - ext;
        let h1 = self.world.line_check(b, a, Vec3::ZERO);
        let h2 = self.world.line_check(a, b, Vec3::ZERO);
        let w1 = self.wall_hit(&h1);
        let w2 = self.wall_hit(&h2);
        let loc = self.pawn.location;
        let r = self.pawn.collision_radius;
        let single = if w1 {
            if w2 && h1.normal.dot(h2.normal) < 0.0 {
                let d = n2d(h1.normal - h2.normal);
                if (h1.location - h2.location).dot(d) >= 20.0 {
                    return None;
                }
                let d1 = (loc - h1.location).size_2d();
                let d2 = (loc - h2.location).size_2d();
                let near = if d2 <= d1 { &h2 } else { &h1 };
                let nn = n2d(near.normal);
                let dd = n2d(offset);
                let mind = d1.min(d2);
                let side = if (near.location - loc).dot(nn) <= 0.0 { 1.0 } else { -1.0 };
                let along = (dd * mind).dot(nn).abs() * side;
                let s = (corner_reach(nn, r) - along).max(0.0);
                return Some(nn * s);
            }
            h1
        } else if w2 {
            h2
        } else {
            return None;
        };
        let nn = n2d(single.normal);
        let out = -((single.location - loc).x * nn.x + (single.location - loc).y * nn.y);
        if out <= 10.0 {
            return None;
        }
        let s = (corner_reach(nn, r) - out).max(0.0);
        Some(nn * s)
    }

    /// ATdPlayerPawn::CheckValidFloor (vt293): probe ahead along the floor (and across it) for
    /// an edge under the cylinder's rim; if the pushed-off spot has no floor within
    /// MaxStepHeight, the floor isn't valid - and with `slide_off` the pawn is moved off it
    /// (the mesh keeps its place via OffsetMeshXY, EvadeTimer 0.2).
    pub fn check_valid_floor(&mut self, delta: Vec3, floor: Vec3, slide_off: bool) -> bool {
        let p = &self.pawn;
        let h = p.collision_height;
        let r = p.collision_radius;
        let mut feet = Vec3::new(p.location.x, p.location.y, p.location.z - h);
        let dir = if delta.x * delta.x + delta.y * delta.y <= 0.0 { p.rotation.vector() } else { n2d(delta) };
        let maxc = dir.x.abs().max(dir.y.abs());
        let along = (dir - floor * floor.dot(dir)).safe_normal() / maxc;
        let side_dir = Vec3::new(dir.y, -dir.x, dir.z);
        let across = (side_dir - floor * floor.dot(side_dir)).safe_normal() / maxc;
        let f2 = Vec3::new(floor.x, floor.y, 0.0);
        let fn2 = f2.safe_normal();
        let maxf = if fn2.x != 0.0 || fn2.y != 0.0 { fn2.x.abs().max(fn2.y.abs()) } else { 1.0 };
        feet.z -= f2.length() * (1.0 / maxf / floor.z * r);
        let push = match self.valid_floor_probe(feet, along * r) {
            Some(v) => Some(v),
            None => self.valid_floor_probe(feet, across * r),
        };
        let Some(push) = push else { return true };
        let start = self.pawn.location + push;
        let end = Vec3::new(start.x, start.y, start.z - self.pawn.max_step_height - 2.0);
        let c = self.world.line_check(end, start, self.pawn.extent());
        if c.time >= 1.0 || (c.time > 0.0 && self.pawn.walkable_floor_z > c.normal.z) {
            if !slide_off {
                return false;
            }
            let m = self.move_actor(push);
            if m.time == 1.0 {
                self.offset_mesh_xy(-push, true);
                self.pawn.evade_timer = 0.2;
                return false;
            }
            self.move_actor(-push * m.time);
        }
        true
    }

    /// 0x12B74E0: a horizontal trace from `start` across `off`; a wall there adds the overlap
    /// to `acc` (per axis, the larger push of a consistent sign).
    fn probe_corner(&self, start: Vec3, off: Vec3, acc: &mut Vec3) -> bool {
        let end = start + off;
        let c = self.world.line_check(end, start, Vec3::ZERO);
        if c.time >= 1.0 || c.time <= 0.0 || self.pawn.walkable_floor_z <= c.normal.z.abs() {
            return false;
        }
        let d = c.location - end;
        let n = n2d(c.normal);
        let t = n.x * d.x + n.y * d.y;
        let (px, py) = (n.x * t, n.y * t);
        let sign = |v: f32| if v == 0.0 { 0.0 } else { v.signum() };
        if acc.x == 0.0 || acc.x.signum() == sign(px) {
            if px.abs() > acc.x.abs() {
                acc.x = px;
            }
        }
        if acc.y != 0.0 && acc.y.signum() != sign(py) {
            return true;
        }
        if py.abs() > acc.y.abs() {
            acc.y = py;
        }
        true
    }

    /// ATdPlayerPawn::physFalling, rising into a ceiling: four corner probes just over the
    /// cylinder's top (a second ring 4 uu higher if the first finds nothing) push the pawn
    /// out from under a ledge it clips with its head.
    pub(crate) fn ceiling_corner_push(&mut self) {
        let r1 = self.pawn.collision_radius + 1.0;
        let mut start = self.pawn.location;
        start.z += self.pawn.collision_height + 1.0;
        let corners = [-r1, r1];
        let mut push = Vec3::ZERO;
        let mut hits = 0;
        for _ in 0..2 {
            for k in 0..4usize {
                let off = Vec3::new(corners[k & 1], corners[k >> 1], 0.0);
                hits += self.probe_corner(start, off, &mut push) as i32;
            }
            if hits != 0 {
                break;
            }
            start.z += 4.0;
        }
        if hits == 0 {
            return;
        }
        let len2 = push.dot(push);
        if r1 * r1 < len2 || len2 <= 0.0 {
            return;
        }
        let len = len2.sqrt();
        let step = (r1 - len).min(5.0);
        push += push * (step / len);
        let m = self.move_actor(push);
        if m.time >= 1.0 || m.time < 0.0 {
            self.pawn.just_teleported = true;
        } else {
            self.move_actor(-push * m.time);
        }
    }
}
