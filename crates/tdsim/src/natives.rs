//! UTdMove natives that look at the world: MovementTrace, FindLedge (0x11F4160), FindLedgeEx
//! (0x11F47A0), CalculateRelativeExtent (0x11F3C40), FindLedgeInFrontOfPlayer (0x12050F0) and
//! UTdMove_WallClimb::DetectPossibleHandPlant (0x12053A0).

use crate::collision::{CheckResult, Surface};
use crate::math::{UeVec, Vec3};
use crate::sim::Sim;

/// `TdPawn.LedgeHitInfo`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LedgeHitInfo {
    pub ledge_location: Vec3,
    /// Normal of the ledge's top surface.
    pub ledge_normal: Vec3,
    /// Normal of the wall below the ledge.
    pub move_normal: Vec3,
    pub feet_excluded: bool,
    pub hands_excluded: bool,
}

impl LedgeHitInfo {
    fn new(loc: Vec3, ledge_normal: Vec3, move_normal: Vec3, s: Surface) -> Self {
        LedgeHitInfo { ledge_location: loc, ledge_normal, move_normal, feet_excluded: s.exclude_foot_moves, hands_excluded: s.exclude_hand_moves }
    }
}

/// `max(|x|, |y|)` of a 2D direction, and the factor TdMove uses to widen an axis-aligned
/// extent along a diagonal: sqrt(1 - m^2) * m * 2(sqrt2 - 1).
fn diag_factor(d2: Vec3) -> f32 {
    let m = d2.x.abs().max(d2.y.abs());
    (1.0 - (m * m).min(1.0)).sqrt() * m * 0.828_427
}

impl Sim {
    /// UTdMove::MovementTrace: first blocking hit between start and end.
    pub fn movement_trace(&self, end: Vec3, start: Vec3, extent: Vec3) -> Option<CheckResult> {
        let h = self.world.line_check(end, start, extent);
        if h.hit { Some(h) } else { None }
    }

    /// UTdMove::CalculateRelativeExtent.
    pub fn calculate_relative_extent(&self, base: f32) -> f32 {
        let mut d = self.pawn.move_normal.safe_normal_2d();
        if d.length() <= 1e-4 {
            d = -self.pawn.rotation.vector().safe_normal_2d();
        }
        diag_factor(d) * base
    }

    /// Shared tail of FindLedge / FindLedgeEx: from the wall hit, step to the wall surface and
    /// LedgeFindDepth into it, then trace down for the top. Returns (start, end, wall face point);
    /// the ledge location reported is the face point's XY with the top hit's Z.
    fn ledge_top_probe(&self, wall: &CheckResult, dir2d: Vec3, extent: Vec3) -> (Vec3, Vec3, Vec3) {
        let depth = self.pawn.ledge_find_depth;
        let wn2 = wall.normal.safe_normal_2d();
        let mut off = (diag_factor(wn2) + 1.0) * extent.x;
        let c = wn2.dot(dir2d).abs();
        if c > 0.0 {
            off /= c;
        }
        let p = wall.location + dir2d * off;
        let start = Vec3::new(p.x - wn2.x * depth, p.y - wn2.y * depth, p.z + extent.z);
        let end = Vec3::new(start.x, start.y, start.z - extent.z * 2.0);
        (start, end, p)
    }

    /// UTdMove::FindLedge.
    pub fn find_ledge(&self, start: Vec3, end: Vec3, extent: Vec3) -> Option<LedgeHitInfo> {
        let dir2d = (end - start).safe_normal().safe_normal_2d();
        let wall = self.world.line_check(end, start, extent);
        if !wall.hit || wall.time <= 0.0 {
            return None;
        }
        let (s2, e2, face) = self.ledge_top_probe(&wall, dir2d, extent);
        let top = self.world.line_check(e2, s2, Vec3::ZERO);
        if top.hit && top.time > 0.0 {
            Some(LedgeHitInfo::new(Vec3::new(face.x, face.y, top.location.z), top.normal, wall.normal, top.surface))
        } else {
            None
        }
    }

    /// UTdMove::FindLedgeEx: 0 = nothing, 1 = a wall without a reachable top, 2 = a ledge.
    pub fn find_ledge_ex(&self, start: Vec3, end: Vec3, extent: Vec3) -> (i32, LedgeHitInfo) {
        let dir2d = (end - start).safe_normal().safe_normal_2d();
        // is the start column itself blocked?
        let col_top = Vec3::new(start.x, start.y, start.z + (extent.z - 1.0));
        let col_bot = Vec3::new(start.x, start.y, start.z - (extent.z - 1.0));
        let col = self.world.line_check(col_top, col_bot, Vec3::new(extent.x, extent.y, 1.0));
        if col.hit && col.time > 0.0 {
            return (0, LedgeHitInfo::default());
        }
        let wall = self.world.line_check(end, start, extent);
        if !wall.hit || wall.time <= 0.0 {
            return (0, LedgeHitInfo::default());
        }
        let (mut s2, e2, face) = self.ledge_top_probe(&wall, dir2d, extent);
        let loc_wall_z = |z: f32| Vec3::new(face.x, face.y, z);
        let top = self.world.line_check(e2, s2, Vec3::ZERO);
        if !top.hit {
            return (1, LedgeHitInfo::new(loc_wall_z(wall.location.z), wall.normal, wall.normal, wall.surface));
        }
        if top.time > 0.0 {
            return (2, LedgeHitInfo::new(loc_wall_z(top.location.z), top.normal, wall.normal, top.surface));
        }
        // started inside: retry from halfway down
        s2.z -= (s2.z - e2.z) * 0.5;
        let top = self.world.line_check(e2, s2, Vec3::ZERO);
        if !top.hit {
            return (0, LedgeHitInfo::default());
        }
        if top.time > 0.0 {
            (2, LedgeHitInfo::new(loc_wall_z(top.location.z), top.normal, wall.normal, top.surface))
        } else {
            (1, LedgeHitInfo::new(loc_wall_z(top.location.z), wall.normal, wall.normal, wall.surface))
        }
    }

    /// UTdMove::FindLedgeInFrontOfPlayer -> (ledge location, ledge normal, move normal).
    pub fn find_ledge_in_front(&mut self) -> Option<LedgeHitInfo> {
        let p = &self.pawn;
        let fwd = p.rotation.vector().safe_normal_2d();
        let end = p.location + fwd * p.ledge_find_distance;
        let h = self.find_ledge(p.location, end, p.ledge_find_extent)?;
        self.pawn.found_ledge_excludes_hand_moves = h.hands_excluded;
        self.pawn.found_ledge_excludes_foot_moves = h.feet_excluded;
        Some(h)
    }

    /// UTdMove_WallClimb::DetectPossibleHandPlant: two hand-width probes (left and right of the
    /// body) for a ledge or wall straight ahead. Returns 0 / 1 (wall) / 2 (ledge) and fills
    /// MoveLedgeLocation / MoveLedgeNormal / MoveNormal.
    pub fn detect_possible_hand_plant(
        &mut self,
        m: crate::pawn::Move,
        location: Vec3,
        rotation: crate::math::Rotator,
        check_distance: f32,
        two_sided: bool,
    ) -> i32 {
        let (ledge, found) = self.detect_possible_hand_plant_out(m, location, rotation, check_distance, two_sided);
        let Some((hl, hr)) = found else { return 0 };
        self.pawn.move_ledge_location = (hl.ledge_location + hr.ledge_location) * 0.5;
        self.pawn.move_ledge_normal = hl.ledge_normal;
        self.pawn.move_normal = hl.move_normal;
        self.pawn.found_ledge_excludes_hand_moves = hl.hands_excluded;
        self.pawn.found_ledge_excludes_foot_moves = hl.feet_excluded;
        ledge
    }

    /// DetectPossibleHandPlant_body with its results as out values (left and right probe) rather
    /// than the pawn's Move* fields, as TdMove_GrabTransfer calls it.
    pub fn detect_possible_hand_plant_out(
        &self,
        m: crate::pawn::Move,
        location: Vec3,
        rotation: crate::math::Rotator,
        check_distance: f32,
        two_sided: bool,
    ) -> (i32, Option<(LedgeHitInfo, LedgeHitInfo)>) {
        let b = self.moves.base(m).clone();
        let mut fwd = rotation.vector().safe_normal_2d();
        if b.check_for_edge_in_vel_dir {
            fwd.z += self.pawn.velocity.safe_normal().z;
        }
        let ext = Vec3::new(b.hand_plant_extent_check_width * 0.5, b.hand_plant_extent_check_width * 0.5, b.hand_plant_extent_check_height);
        let f2 = rotation.vector().safe_normal_2d();
        let side = Vec3::new(f2.y, -f2.x, 0.0);
        let dr = self.pawn.default_collision_radius;
        let dh = self.pawn.default_collision_height;
        let side_off = dr - b.hand_plant_extent_check_width * 0.5;
        let iterations = if two_sided { 3 } else { 1 };
        let mut result: Option<(i32, LedgeHitInfo, LedgeHitInfo)> = None;
        for i in 0..iterations {
            let base = match i {
                0 => location,
                1 => location - side * 30.0,
                _ => location + side * 30.0,
            };
            let mut s1 = base - side * side_off;
            s1.z += b.hand_plant_check_height - dh;
            let e1 = s1 + fwd * check_distance;
            let (ledge, hl) = self.find_ledge_ex(s1, e1, ext);
            if ledge == 0 || (i > 0 && hl.move_normal.z < 0.98) || hl.move_normal.z > 0.2 {
                continue;
            }
            if self.pawn.illegal_ledge_timer > 0.0 && self.pawn.illegal_ledge_normal.dot(hl.move_normal) > 0.98 {
                continue;
            }
            if (ledge == 2 && hl.hands_excluded) || (ledge == 1 && hl.feet_excluded) {
                continue;
            }
            let mut s2 = base + side * side_off;
            s2.z += b.hand_plant_check_height - dh;
            let e2 = s2 + fwd * check_distance;
            let (ledge2, hr) = self.find_ledge_ex(s2, e2, ext);
            if ledge2 != ledge {
                continue;
            }
            let lr = (hr.ledge_location - hl.ledge_location).safe_normal();
            if lr.dot(hl.ledge_normal).abs() > 0.25 {
                return (0, None);
            }
            if hr.move_normal.z <= 0.2
                && hl.ledge_normal.dot(hr.ledge_normal).abs() >= 0.99
                && hr.move_normal.dot(hl.move_normal).abs() >= 0.95
            {
                let lr2 = (hr.ledge_location - hl.ledge_location).safe_normal_2d();
                let mn2 = hl.move_normal.safe_normal_2d();
                if lr2.dot(mn2).abs() <= 0.25 {
                    result = Some((ledge, hl, hr));
                    break;
                }
            }
        }
        match result {
            Some((ledge, hl, hr)) => (ledge, Some((hl, hr))),
            None => (0, None),
        }
    }
}
