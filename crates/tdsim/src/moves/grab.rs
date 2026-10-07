//! Ledge grabbing: TdMove_IntoGrab, TdMove_Grab (hang, shimmy, corner, turn, drop), TdMove_GrabPullUp,
//! TdMove_GrabJump, their natives (IntoGrab vt73/vt74, Grab vt70/vt75/vt76, ShimmyMove 0x11F7D60)
//! and TdPhysicsMove's FindFloorOverLedge / CanHeaveOverLedge.

use crate::config::Config;
use crate::math::{Rotator, UeVec, Vec3, norm_axis};
use crate::moves::PreciseMode;
use crate::pawn::{Move, MoveAction, Physics, Slot};
use crate::sim::Sim;

/// `TdMove_Grab.EGrabType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrabType {
    LegsOnWall,
    LegsFree,
}

/// `TdMove_Grab.EShimmyType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shimmy {
    Shimmy,
    ShimmyLong,
    AroundCorner,
    NoShimmy,
}

/// `TdMove_Grab.EGrabFoldedType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Folded {
    None,
    Start,
    End,
}

/// `TdPawn.CurrentGrabTurnType` (EGrabTurnType).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GrabTurn {
    #[default]
    None,
    Start,
    End,
    Idle,
}

pub struct IntoGrab {
    pub max_angle: f32,
    pub align_speed: f32,
    pub min_initial_align_speed: f32,
    pub min_grabable_z_normal: f32,
    pub desired_ledge_offset: Vec3,
    pub min_ledge_adjust_distance: f32,
    pub max_distance: f32,
    pub into_grab_speed: f32,
    pub hang_folded_downward_speed_limit: f32,
    pub hang_folded_into_grab_z_speed_threshold: f32,
    pub hang_folded_into_grab_speed_2d_threshold: f32,
    pub hang_folded_upper_delta_distance: f32,
    pub hang_folded_lower_delta_distance: f32,
    pub hang_folded_max_distance: f32,
    pub hang_impact_min_z_speed: f32,
    pub hang_hard_impact_min_z_speed: f32,
    pub prepare_to_grab: bool,
    pub sloped_ledge: bool,
}

pub struct Grab {
    pub desired_ledge_offset: Vec3,
    pub max_angle: f32,
    pub hang_free_z_distance_check: f32,
    pub relative_extent: f32,
    pub distance_to_wall_from_feet: f32,
    pub start_turning_angle: f32,
    pub is_within_forward_view: bool,
    pub is_turned_right: bool,
    pub sloped_ledge: bool,
    pub climb_up_folded_action_received: bool,
    pub request_drop_down: bool,
    pub hang_free_vertigo_effect: bool,
    pub grab_from_vertical_wallrun: bool,
    pub grab_from_high_z_speed: bool,
    pub pending_shimmy_corner_animation: String,
    pub grab_type: GrabType,
    pub previous_grab_type: GrabType,
    pub shimmy: Shimmy,
    pub folded: Folded,
    pub target_yaw: i32,
    pub target_location: Vec3,
    pub shimmy_velocity: f32,
    pub shimmy_time: f32,
    pub last_shimmy_time_seconds: f32,
    pub disable_shimmy_time: f32,
    pub start_looking_at_ledge_time: f32,
    pub stop_looking_at_ledge_time: f32,
}

pub struct GrabPullUp {
    pub into_crouch: bool,
    pub floor_over_ledge_location: Vec3,
    pub allowed_pull_up_angle: i32,
}

pub struct GrabJump {
    pub off_z_height: f32,
    pub push_away_max_speed: f32,
    pub push_away_min_speed: f32,
    pub allowed_jump_angle: f32,
    pub jump_velocity: Vec3,
    pub delta_jump_yaw: i32,
}

impl IntoGrab {
    pub fn new(cfg: &Config) -> Self {
        let c = &["TdMove_IntoGrab"];
        IntoGrab {
            max_angle: cfg.f32(c, "IntoGrabMaxAngle", 70.0),
            align_speed: cfg.f32(c, "IntoGrabAlignSpeed", 300.0),
            min_initial_align_speed: cfg.f32(c, "IntoGrabMinInitialAlignSpeed", -3000.0),
            min_grabable_z_normal: cfg.f32(c, "GrabMinGrabableZNormal", 0.707),
            desired_ledge_offset: cfg.vec3(c, "GrabDesiredLedgeOffset", Vec3::new(30.0, 0.0, 92.8)),
            min_ledge_adjust_distance: cfg.f32(c, "MinGrabLedgeAdjustDistance", 32.0),
            max_distance: cfg.f32(c, "IntoGrabMaxDistance", 200.0),
            into_grab_speed: 0.0,
            hang_folded_downward_speed_limit: cfg.f32(c, "HangFoldedDownwardSpeedLimit", -300.0),
            hang_folded_into_grab_z_speed_threshold: cfg.f32(c, "HangFoldedIntoGrabZSpeedThreshold", 150.0),
            hang_folded_into_grab_speed_2d_threshold: cfg.f32(c, "HangFoldedIntoGrabSpeed2DThreshold", 50.0),
            hang_folded_upper_delta_distance: cfg.f32(c, "HangFoldedUpperDeltaDistance", 35.0),
            hang_folded_lower_delta_distance: cfg.f32(c, "HangFoldedLowerDeltaDistance", 70.0),
            hang_folded_max_distance: cfg.f32(c, "HangFoldedMaxDistance", 60.0),
            hang_impact_min_z_speed: cfg.f32(c, "HangImpactMinZSpeed", -600.0),
            hang_hard_impact_min_z_speed: cfg.f32(c, "HangHardImpactMinZSpeed", -1000.0),
            prepare_to_grab: false,
            sloped_ledge: false,
        }
    }
}

impl Grab {
    pub fn new(cfg: &Config) -> Self {
        let c = &["TdMove_Grab"];
        Grab {
            desired_ledge_offset: cfg.vec3(c, "GrabDesiredLedgeOffset", Vec3::new(30.0, 0.0, 92.8)),
            max_angle: cfg.f32(c, "GrabMaxAngle", 40.0),
            hang_free_z_distance_check: cfg.f32(c, "HangFreeZDistanceCheck", 128.0),
            relative_extent: 0.0,
            distance_to_wall_from_feet: 0.0,
            start_turning_angle: cfg.f32(c, "StartTurningAngle", 16384.0),
            is_within_forward_view: true,
            is_turned_right: false,
            sloped_ledge: false,
            climb_up_folded_action_received: false,
            request_drop_down: false,
            hang_free_vertigo_effect: false,
            grab_from_vertical_wallrun: false,
            grab_from_high_z_speed: false,
            pending_shimmy_corner_animation: String::new(),
            grab_type: GrabType::LegsOnWall,
            previous_grab_type: GrabType::LegsOnWall,
            shimmy: Shimmy::NoShimmy,
            folded: Folded::None,
            target_yaw: 0,
            target_location: Vec3::ZERO,
            shimmy_velocity: 0.0,
            shimmy_time: 0.0,
            last_shimmy_time_seconds: 0.0,
            disable_shimmy_time: 0.6,
            start_looking_at_ledge_time: 0.0,
            stop_looking_at_ledge_time: 0.0,
        }
    }
}

impl GrabPullUp {
    pub fn new(cfg: &Config) -> Self {
        GrabPullUp { into_crouch: false, floor_over_ledge_location: Vec3::ZERO, allowed_pull_up_angle: cfg.i32(&["TdMove_GrabPullUp"], "GrabAllowedPullUpAngle", 45) }
    }
}

impl GrabJump {
    pub fn new(cfg: &Config) -> Self {
        let c = &["TdMove_GrabJump"];
        GrabJump {
            off_z_height: cfg.f32(c, "GrabJumpOffZHeight", 160.0),
            push_away_max_speed: cfg.f32(c, "GrabJumpPushAwayMaxSpeed", 400.0),
            push_away_min_speed: cfg.f32(c, "GrabJumpPushAwayMinSpeed", 200.0),
            allowed_jump_angle: cfg.f32(c, "GrabAllowedJumpAngle", 45.0),
            jump_velocity: Vec3::ZERO,
            delta_jump_yaw: 0,
        }
    }
}

// SetMoveTimer function ids.
const T_SET_DPG: u8 = 1;
const T_CHANGE_DPG: u8 = 2;
const T_START_ROOT_ROTATION: u8 = 3;
const T_RELEASE_CAMERA: u8 = 4;
const T_ENABLE_COLLISION: u8 = 5;

// TdMove_Grab look constraint defaults.
const HANG_FREE_MIN: Rotator = Rotator { pitch: 8300, yaw: -16384, roll: 0 };
const HANG_FREE_MAX: Rotator = Rotator { pitch: 16000, yaw: 16384, roll: 0 };
const SLOPE_MIN: Rotator = Rotator { pitch: -3200, yaw: -8192, roll: 0 };
const SLOPE_MAX: Rotator = Rotator { pitch: 8300, yaw: 8192, roll: 0 };
const CORNER_MIN: Rotator = Rotator { pitch: 3200, yaw: -32768, roll: 0 };
const CORNER_MAX: Rotator = Rotator { pitch: 16000, yaw: 32768, roll: 0 };
const CORNER_FREE_MIN: Rotator = Rotator { pitch: 11000, yaw: -32768, roll: 0 };
const CORNER_FREE_MAX: Rotator = Rotator { pitch: 16000, yaw: 32768, roll: 0 };
pub const GRAB_DEFAULT_MIN: Rotator = Rotator { pitch: -3200, yaw: -32768, roll: 0 };
pub const GRAB_DEFAULT_MAX: Rotator = Rotator { pitch: 16000, yaw: 32768, roll: 0 };

fn is_left(a: MoveAction) -> bool {
    matches!(a, MoveAction::ShimmyLeft | MoveAction::ShimmyLeftLong)
}

fn strafe_anim(free: bool, left: bool) -> &'static str {
    match (free, left) {
        (true, true) => "HangFreeStrafeLeft",
        (true, false) => "HangFreeStrafe",
        (false, true) => "HangStrafeLeft",
        (false, false) => "HangStrafeRight",
    }
}

impl Sim {
    // ---------------------------------------------------------------- TdPhysicsMove helpers

    /// TdPhysicsMove.FindFloorOverLedge.
    pub fn find_floor_over_ledge(&self, depth: f32, floor: &mut Vec3, height: f32) -> bool {
        let p = &self.pawn;
        let r = p.default_collision_radius;
        let mut start = p.location - p.move_normal * depth;
        start.z = p.move_ledge_location.z + r * 1.414 + 4.0;
        let mut end = p.location - p.move_normal * depth;
        end.z = p.move_ledge_location.z - r * 1.414 - height * 2.0;
        let mut ext = p.extent();
        ext.x -= 5.0;
        ext.y -= 5.0;
        ext.z = 0.0;
        match self.movement_trace(end, start, ext) {
            Some(h) => {
                *floor = h.location;
                true
            }
            None => false,
        }
    }

    /// TdPhysicsMove.CanHeaveOverLedge.
    fn can_heave_over_ledge(&self, depth: f32, dest_floor: Vec3, height: f32, highest: f32) -> bool {
        let p = &self.pawn;
        let start = p.location;
        let mut end = start;
        end.z = p.move_ledge_location.z + height;
        let mut ext = p.extent();
        ext.z = height;
        if self.movement_trace_for_blocking(end, start, ext) {
            return false;
        }
        let mut start = p.location;
        start.z = (dest_floor.z as i32).max(highest as i32) as f32 + height + 2.0;
        let end = start - p.move_normal * depth;
        let mut ext = p.extent();
        ext.z = height - 2.0;
        !self.movement_trace_for_blocking(end, start, ext)
    }

    pub fn can_heave_over_ledge_fully_extended(&self, depth: f32, dest: Vec3, highest: f32) -> bool {
        self.can_heave_over_ledge(depth, dest, self.pawn.collision_height, highest)
    }

    pub fn can_heave_over_ledge_crouched(&self, depth: f32, dest: Vec3, highest: f32) -> bool {
        self.can_heave_over_ledge(depth, dest, 122.0 * 0.5, highest)
    }

    // ---------------------------------------------------------------- IntoGrab

    /// TdMove_IntoGrab.CanDoMove (script).
    pub fn into_grab_can_do_move(&mut self, m: Move) -> bool {
        self.tdmove_can_do_move(m) && self.pawn.movement_state != Move::IntoClimb
    }

    /// UTdMove_IntoGrab vt74 (0x11F9C30): the native ledge test used by auto-move detection.
    pub fn into_grab_native_can_do(&self) -> bool {
        let p = &self.pawn;
        let g = &self.moves.into_grab;
        if p.evade_timer > 0.0 {
            return false;
        }
        let dx = p.move_ledge_location.x - p.location.x;
        let dy = p.move_ledge_location.y - p.location.y;
        let grab_z = p.move_ledge_location.z - g.desired_ledge_offset.z;
        let dz = grab_z - p.location.z;
        let dir2 = Vec3::new(dx, dy, 0.0).safe_normal();
        let f = ((p.velocity.x * dir2.x + p.velocity.y * dir2.y) * 0.005).clamp(0.4, 1.0);
        let maxd = g.max_distance * f;
        if dz * dz + dx * dx + dy * dy > maxd * maxd {
            return false;
        }
        if p.velocity.z >= 0.0 && (p.move_ledge_location.z > p.collision_height + p.location.z || p.movement_state != Move::WallClimbing) {
            return false;
        }
        if p.move_ledge_location.z - p.location.z < 0.0 {
            return false;
        }
        if g.min_grabable_z_normal > p.move_ledge_normal.z {
            return false;
        }
        let fwd = p.rotation.vector();
        if (g.max_angle * 0.017453292).cos() > (-p.move_normal).dot(fwd) {
            return false;
        }
        if g.min_initial_align_speed > p.velocity.dot(-p.move_normal) {
            return false;
        }
        if p.velocity.z < 0.0 && grab_z > p.location.z {
            return false;
        }
        // 0x11F9BA0: ledge inside a no-grab volume. The port has no volumes.
        true
    }

    /// TdMove_IntoGrab.StartMove.
    pub fn into_grab_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        let sloped = self.pawn.move_ledge_normal.z < 0.999;
        self.moves.into_grab.sloped_ledge = sloped;
        self.moves.grab.sloped_ledge = sloped;
        let mut mn2 = self.pawn.move_normal;
        mn2.z = 0.0;
        let mn2 = mn2.safe_normal();
        let mut wanted = self.pawn.move_ledge_location;
        wanted.z -= self.moves.into_grab.desired_ledge_offset.z;
        let r = self.pawn.collision_radius;
        let rel = self.calculate_relative_extent(r);
        wanted += mn2 * (r + rel);
        self.moves.base_mut(m).root_offset.x += rel;
        if self.pawn.old_movement_state == Move::WallClimbing {
            let g = self.pawn.gravity_z().abs();
            let need = self.pawn.move_ledge_location.z - (self.pawn.location.z + self.pawn.collision_height);
            let speed = (0i32.max((4.0 * (need + 4.0) * g) as i32) as f32).sqrt();
            self.pawn.velocity.z = (speed as i32).min(self.pawn.velocity.z as i32) as f32;
        }
        self.pawn.move_location = wanted;
        if self.pawn.old_movement_state == Move::GrabTransfer {
            self.into_grab_reached_precise_location(m);
        } else {
            let s = self.moves.into_grab.align_speed.max(self.pawn.velocity.size_2d());
            self.set_precise_location(m, wanted, PreciseMode::Fall, s);
            let rot = Rotator::from_vector(-self.pawn.move_normal);
            self.set_precise_rotation(m, rot, 0.2);
        }
        self.moves.into_grab.into_grab_speed = self.pawn.velocity.z;
        self.moves.grab.folded = Folded::None;
        self.into_grab_check_for_folded_hang();
        self.moves.into_grab.prepare_to_grab = false;
    }

    /// TdMove_IntoGrab.CheckForFoldedHang.
    fn into_grab_check_for_folded_hang(&mut self) {
        let g = &self.moves.into_grab;
        let p = &self.pawn;
        let vel2d = p.velocity.size_2d();
        let dist2d = (p.move_ledge_location - p.location).size_2d();
        if g.sloped_ledge
            || g.into_grab_speed > g.hang_folded_into_grab_z_speed_threshold
            || g.into_grab_speed < -1200.0
            || vel2d < g.hang_folded_into_grab_speed_2d_threshold
            || dist2d > g.hang_folded_max_distance
            || p.move_ledge_normal.z < 0.98
            || p.move_ledge_location.z > p.location.z + g.hang_folded_upper_delta_distance
            || p.move_ledge_location.z < p.location.z - g.hang_folded_lower_delta_distance
            || self.moves.grab.folded == Folded::Start
            || !self.grab_is_facing_ledge()
        {
            return;
        }
        let start = p.move_location;
        let mut end = start;
        end.z -= p.collision_height + 1.0;
        let mut ext = p.extent();
        ext.z = 0.0;
        if self.movement_trace_for_blocking(end, start, ext) {
            return;
        }
        let mut floor = p.location;
        floor.z -= p.collision_height;
        let h = p.max_step_height + 4.0;
        self.find_floor_over_ledge(98.0, &mut floor, h);
        if floor.z < self.pawn.move_ledge_location.z - 4.0 {
            return;
        }
        floor.z = self.pawn.move_ledge_location.z + 4.0;
        if !self.can_heave_over_ledge_crouched(98.0, floor, floor.z) {
            return;
        }
        self.grab_enable_folded_hang();
        self.set_animation_movement_state(Move::Grabbing, -1.0);
        let g = &self.moves.into_grab;
        let (lo, hi) = (g.hang_folded_downward_speed_limit as i32, g.hang_folded_into_grab_z_speed_threshold as i32);
        self.pawn.velocity.z = (self.pawn.velocity.z as i32).clamp(lo, hi) as f32;
    }

    /// UTdMove_IntoGrab vt73 (0x12063F0, replaces the TdPhysicsMove detector): re-detect the
    /// ledge while flying in and retarget when it moved further than MinGrabLedgeAdjustDistance.
    pub fn into_grab_pre_physics(&mut self, m: Move) {
        if self.pawn.velocity.is_nearly_zero() || self.pawn.old_movement_state == Move::GrabTransfer {
            return;
        }
        let saved = (self.pawn.move_ledge_location, self.pawn.move_ledge_normal, self.pawn.move_normal);
        let (loc, rot) = (self.pawn.location, self.pawn.rotation);
        let dist = self.moves.base(m).hand_plant_check_distance;
        let r = self.detect_possible_hand_plant(m, loc, rot, dist, true);
        let found = (self.pawn.move_ledge_location, self.pawn.move_ledge_normal, self.pawn.move_normal);
        (self.pawn.move_ledge_location, self.pawn.move_ledge_normal, self.pawn.move_normal) = saved;
        self.pawn.move_ledge_result = r;
        if r != 2 || !self.into_grab_native_can_do() {
            return;
        }
        let p = &self.pawn;
        if (p.location - p.move_location).size_2d() <= self.moves.into_grab.min_ledge_adjust_distance {
            return;
        }
        (self.pawn.move_ledge_location, self.pawn.move_ledge_normal, self.pawn.move_normal) = found;
        let r = self.pawn.collision_radius;
        let rel = self.calculate_relative_extent(r);
        let mn2 = self.pawn.move_normal.safe_normal_2d();
        let mut t = self.pawn.move_ledge_location + mn2 * (rel + r);
        t.z -= self.moves.into_grab.desired_ledge_offset.z;
        self.moves.base_mut(m).precise_location = t;
        self.pawn.move_location = t;
    }

    /// TdMove_IntoGrab.ReachedPreciseLocation.
    pub fn into_grab_reached_precise_location(&mut self, _m: Move) {
        if !self.can_do_move(Move::Grabbing) {
            self.set_move(Move::Falling, false, false);
            return;
        }
        self.moves.into_grab.prepare_to_grab = true;
        let r = Rotator::from_vector(-self.pawn.move_normal);
        self.set_rotation(r);
        let ml = self.pawn.move_location;
        self.set_location(ml);
        self.pawn.velocity = Vec3::ZERO;
        self.pawn.acceleration = Vec3::ZERO;
        let t = self.grab_check_wall_leg_placement();
        self.grab_set_type(t);
        self.moves.grab.previous_grab_type = self.moves.grab.grab_type;
        let sloped = self.moves.into_grab.sloped_ledge;
        let speed = self.moves.into_grab.into_grab_speed;
        let gm = Move::Grabbing;
        let slot = if sloped { Slot::Camera } else { Slot::FullBody };
        let bo = if sloped { 0.8 } else { 0.2 };
        if self.moves.grab.folded == Folded::Start {
        } else if self.pawn.old_movement_state == Move::WallClimbing && self.moves.wallclimb.performed_double_jump {
            self.moves.grab.grab_from_vertical_wallrun = true;
            self.play_move_anim(gm, Slot::FullBody, "hanghardstartvertical", 1.0, 0.2, bo, false, false);
        } else if self.grab_is_hanging_free() {
            self.play_move_anim(gm, slot, "HangFreeHardStart", 1.0, 0.1, 0.2, false, false);
            self.moves.grab.start_looking_at_ledge_time = 0.5;
            self.moves.grab.stop_looking_at_ledge_time = 1.0;
        } else if !sloped && speed < self.moves.into_grab.hang_impact_min_z_speed {
            if speed < self.moves.into_grab.hang_hard_impact_min_z_speed {
                self.play_move_anim(gm, Slot::FullBody, "HangHardStart3", 1.0, 0.1, bo, false, false);
                self.play_move_anim(gm, Slot::Camera, "gethitfront", 1.0, 0.05, bo, false, false);
                self.moves.grab.start_looking_at_ledge_time = 1.6;
                self.moves.grab.stop_looking_at_ledge_time = 2.0;
                self.moves.grab.grab_from_high_z_speed = true;
            } else {
                self.play_move_anim(gm, slot, "HangHardStart2", 1.0, 0.1, 0.2, false, false);
                self.moves.grab.start_looking_at_ledge_time = 0.8;
                self.moves.grab.stop_looking_at_ledge_time = 1.2;
            }
        } else {
            self.play_move_anim(gm, slot, "HangHardStart", 1.0, 0.1, 0.2, false, false);
            self.moves.grab.start_looking_at_ledge_time = 0.5;
            self.moves.grab.stop_looking_at_ledge_time = 1.0;
        }
        self.reset_camera_look(gm, 0.15);
        self.set_move(Move::Grabbing, false, false);
    }

    /// TdMove_IntoGrab.FailedToReachPreciseLocation.
    pub fn into_grab_failed_precise_location(&mut self, _m: Move) {
        self.set_move(Move::Falling, false, false);
    }

    /// TdMove_IntoGrab.StopMove.
    pub fn into_grab_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.moves.base_mut(m).root_offset = Vec3::ZERO;
        if self.moves.grab.folded != Folded::Start {
            self.moves.grab.folded = Folded::None;
        }
        if self.pawn.pending_movement_state != Move::Grabbing && self.moves.grab.folded == Folded::Start {
            self.anim.stop(Slot::FullBody, 0.2);
        }
        self.moves.into_grab.prepare_to_grab = false;
    }

    // ---------------------------------------------------------------- Grab natives

    /// UTdMove_Grab::CheckWallLegPlacement (vt75, 0x11F7F40): sweep from both sides of the
    /// legs towards the wall; no hit on either side means the legs hang free.
    pub fn grab_check_wall_leg_placement(&mut self) -> GrabType {
        let p = &self.pawn;
        let fwd = p.rotation.vector().safe_normal_2d();
        let half = self.moves.base(Move::Grabbing).hand_plant_extent_check_width * 0.5;
        let ext = Vec3::new(half, half, half);
        let side = Vec3::new(fwd.y, -fwd.x, 0.0);
        let r = p.collision_radius;
        let mut base = p.location;
        base.z += p.collision_height - self.moves.grab.hang_free_z_distance_check;
        let mn2 = p.move_normal.safe_normal_2d();
        let m = mn2.x.abs().max(mn2.y.abs());
        let off = ((1.0 - (m * m).min(1.0)).sqrt() * m * 0.828_427 + 1.0) * r;
        let reach = off + 32.0;
        for s in [side, -side] {
            let start = base + s * (r - half);
            let end = start + fwd * reach;
            let h = self.world.line_check(end, start, ext);
            if h.hit {
                let d = (start - h.location).size_2d() - off;
                self.moves.grab.distance_to_wall_from_feet = if d < 0.0 { 0.0 } else { d };
                return GrabType::LegsOnWall;
            }
        }
        GrabType::LegsFree
    }

    /// UTdMove_Grab::IsHangingFree (vt76).
    pub fn grab_is_hanging_free(&self) -> bool {
        self.moves.grab.grab_type == GrabType::LegsFree && !self.moves.grab.sloped_ledge
    }

    fn grab_set_type(&mut self, t: GrabType) {
        self.moves.grab.previous_grab_type = self.moves.grab.grab_type;
        self.moves.grab.grab_type = t;
    }

    /// TdMove_Grab.IsFacingLedge.
    fn grab_is_facing_ledge(&self) -> bool {
        let d = (-self.pawn.move_normal).dot(self.pawn.rotation.vector());
        d >= (self.moves.grab.max_angle * 3.1415927 / 180.0).cos()
    }

    /// TdMove_Grab.EnableFoldedHang.
    fn grab_enable_folded_hang(&mut self) {
        self.moves.grab.folded = Folded::Start;
        self.play_move_anim(Move::Grabbing, Slot::FullBody, "HangFoldedStart", 1.0, 0.2, 0.0, false, false);
        self.moves.base_mut(Move::Grabbing).constrain_look = false;
    }

    // ---------------------------------------------------------------- Grab script

    /// TdMove_Grab.CanDoMove.
    pub fn grab_can_do_move(&mut self, m: Move) -> bool {
        if self.pawn.movement_state != Move::IntoGrab || !self.grab_is_facing_ledge() {
            return false;
        }
        self.tdmove_can_do_move(m)
    }

    /// TdMove_Grab.StartMove.
    pub fn grab_start_move(&mut self, m: Move) {
        self.moves.grab.sloped_ledge = self.pawn.move_ledge_normal.z < 0.999;
        let r = self.pawn.collision_radius;
        let rel = self.calculate_relative_extent(r);
        self.moves.grab.relative_extent = rel;
        let free = self.grab_is_hanging_free();
        self.moves.base_mut(m).root_offset.x += rel + if free { 0.0 } else { 1.0 };
        if self.moves.grab.sloped_ledge && free {
            self.moves.base_mut(m).root_offset.z += 3.0;
        }
        self.physics_move_start_move(m);
        self.set_move_timer(m, 0.2, false, T_SET_DPG);
        self.pawn.velocity = Vec3::ZERO;
        self.moves.grab.target_yaw = -1;
        self.moves.grab.shimmy = Shimmy::NoShimmy;
        self.pawn.current_grab_turn_type = GrabTurn::None;
        self.grab_update_view_constraints(m);
        let t = self.time;
        let g = &mut self.moves.grab;
        g.hang_free_vertigo_effect = false;
        g.climb_up_folded_action_received = false;
        g.request_drop_down = false;
        g.last_shimmy_time_seconds = t;
        // bSlopedLedge: EnableGrabIK (hand IK, visual only)
        if self.cfg_take_fall_damage {
            self.take_falling_damage();
            self.pawn.enter_falling_height = self.pawn.location.z;
        }
    }

    /// TdMove_Grab.StopMove.
    pub fn grab_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.use_root_motion(false);
        self.use_root_rotation(false);
        self.pawn.face_rotation_time_left = 0.5;
        self.pawn.leg_rotation = self.pc.rotation.yaw;
        if self.moves.grab.folded != Folded::Start {
            self.anim.stop(Slot::FullBody, 0.2);
        }
        let b = self.moves.base_mut(m);
        b.root_offset = Vec3::ZERO;
        b.constrain_look = true;
        let g = &mut self.moves.grab;
        g.hang_free_vertigo_effect = false;
        g.grab_from_vertical_wallrun = false;
        g.request_drop_down = false;
        g.grab_from_high_z_speed = false;
    }

    /// TdMove_Grab.UpdateViewConstraints.
    fn grab_update_view_constraints(&mut self, m: Move) {
        let free = self.grab_is_hanging_free();
        let g = &self.moves.grab;
        let (min, max) = if g.sloped_ledge {
            let (mut mn, mut mx) = (SLOPE_MIN, SLOPE_MAX);
            if free {
                mn.pitch = HANG_FREE_MIN.pitch;
                mx.pitch = HANG_FREE_MAX.pitch;
            }
            (mn, mx)
        } else if g.shimmy == Shimmy::AroundCorner {
            if free { (CORNER_FREE_MIN, CORNER_FREE_MAX) } else { (CORNER_MIN, CORNER_MAX) }
        } else if free {
            (HANG_FREE_MIN, HANG_FREE_MAX)
        } else {
            (GRAB_DEFAULT_MIN, GRAB_DEFAULT_MAX)
        };
        let b = self.moves.base_mut(m);
        b.min_look_constraint = min;
        b.max_look_constraint = max;
    }

    /// TdMove_Grab.UpdateGrabType (event from vt70 when the leg placement changed).
    fn grab_update_grab_type(&mut self, m: Move, moving_left: bool) {
        self.grab_update_view_constraints(m);
        let prev_pos = self.anim.normalized_position(Slot::FullBody);
        let free = self.grab_is_hanging_free();
        self.play_move_anim(m, Slot::FullBody, strafe_anim(free, moving_left), 1.0, 0.2, 0.0, false, false);
        let len = self.anim.slots.get(&Slot::FullBody).map(|s| s.length).unwrap_or(0.0);
        self.anim.set_position(Slot::FullBody, prev_pos * len);
        self.moves.base_mut(m).root_offset.x += if free { -1.0 } else { 1.0 };
        let ro = self.moves.base(m).root_offset;
        self.set_root_offset(ro, 0.3);
    }

    /// TdMove_Grab.CanPullUp.
    pub fn grab_can_pull_up(&self) -> bool {
        let g = &self.moves.grab;
        let t = self.moves.base(Move::Grabbing).move_active_time;
        !(g.hang_free_vertigo_effect
            || (g.grab_from_vertical_wallrun && t < 1.0)
            || g.folded == Folded::End
            || g.request_drop_down
            || (g.grab_from_high_z_speed && t < 1.0))
    }

    /// TdMove_Grab.RequestDropDown.
    pub fn grab_request_drop_down(&mut self) {
        let g = &self.moves.grab;
        if g.hang_free_vertigo_effect || g.request_drop_down || g.folded != Folded::None {
            return;
        }
        let t = self.moves.base(Move::Grabbing).move_active_time;
        if g.shimmy != Shimmy::AroundCorner
            && self.pawn.current_grab_turn_type == GrabTurn::None
            && (!g.grab_from_vertical_wallrun || t > 1.0)
        {
            let name = if self.grab_is_hanging_free() { "HangFreeEnd" } else { "HangEnd" };
            self.play_move_anim(Move::Grabbing, Slot::FullBody, name, 1.0, 0.1, 0.2, false, false);
            self.set_animation_movement_state(Move::Falling, 0.2);
            self.set_move_timer(Move::Grabbing, 0.2, false, T_CHANGE_DPG);
        } else {
            self.set_move(Move::Falling, false, false);
        }
        self.moves.grab.request_drop_down = true;
    }

    /// TdMove_Grab.HandleMoveAction.
    pub fn grab_handle_move_action(&mut self, m: Move, a: MoveAction) {
        self.moves.grab.climb_up_folded_action_received = a == MoveAction::ClimbUp;
        if self.moves.grab.shimmy == Shimmy::Shimmy && a == MoveAction::None {
            self.grab_abort_shimmy(false);
            return;
        }
        let g = &self.moves.grab;
        if g.shimmy == Shimmy::AroundCorner || g.hang_free_vertigo_effect || g.folded == Folded::Start {
            return;
        }
        let t = self.moves.base(m).move_active_time;
        let left = matches!(a, MoveAction::ShimmyLeft | MoveAction::ShimmyLeftLong) && t > g.disable_shimmy_time;
        let right = matches!(a, MoveAction::ShimmyRight | MoveAction::ShimmyRightLong) && t > g.disable_shimmy_time;
        if g.sloped_ledge || g.request_drop_down {
            return;
        }
        if (left || right) && !g.is_within_forward_view && !self.grab_is_hanging_free() {
            return;
        }
        if a == MoveAction::Turn && self.moves.grab.folded == Folded::None {
            self.moves.grab.stop_looking_at_ledge_time = 0.0;
            let b = self.moves.base(m);
            let mut la = self.pawn.rotation;
            la.yaw += if self.moves.grab.is_turned_right { b.max_look_constraint.yaw } else { b.min_look_constraint.yaw };
            self.abort_look_at_target(m);
            self.set_look_at_target_angle(m, la.normalize(), 0.2, -1.0);
        }
        if (left || right) && self.time - self.moves.grab.last_shimmy_time_seconds > 0.5 {
            if self.grab_can_shimmy(a) {
                self.grab_start_shimmy(m, a);
            } else {
                self.grab_abort_shimmy(false);
            }
        }
    }

    /// TdMove_Grab.CanShimmy.
    fn grab_can_shimmy(&self, a: MoveAction) -> bool {
        if self.pawn.current_grab_turn_type != GrabTurn::None {
            return false;
        }
        let (_, y, _) = self.pawn.rotation.axes();
        let mut dir = (if is_left(a) { -y } else { y }).safe_normal();
        dir.z = 0.0;
        let g = &self.moves.grab;
        let mut start = self.pawn.move_ledge_location + self.pawn.move_normal * (g.desired_ledge_offset.x + g.relative_extent);
        start.z -= g.desired_ledge_offset.z;
        let end = start + dir * 35.0;
        !self.movement_trace_for_blocking(end, start, self.pawn.extent())
    }

    /// TdMove_Grab.CanShimmyAroundCorner: sets TargetLocation / TargetYaw on success.
    fn grab_can_shimmy_around_corner(&mut self, a: MoveAction) -> bool {
        let (_, y, _) = self.pawn.rotation.axes();
        let left = is_left(a);
        let mut dir = (if left { -y } else { y }).safe_normal();
        dir.z = 0.0;
        let g = &self.moves.grab;
        let dist = g.desired_ledge_offset.x * 2.0 + g.relative_extent + 4.0;
        let mut start = self.pawn.move_ledge_location + self.pawn.move_normal * (g.desired_ledge_offset.x + g.relative_extent);
        start.z -= g.desired_ledge_offset.z;
        let end = start + dir * dist;
        let ext = self.pawn.extent();
        if self.movement_trace_for_blocking(end, start, ext) {
            return false;
        }
        let start = end;
        let end = start - self.pawn.move_normal * dist;
        if self.movement_trace_for_blocking(end, start, ext) {
            return false;
        }
        self.moves.grab.target_location = end;
        self.moves.grab.target_yaw = self.pawn.rotation.yaw + if left { 16384 } else { -16384 };
        true
    }

    /// TdMove_Grab.StartShimmy.
    fn grab_start_shimmy(&mut self, m: Move, a: MoveAction) {
        let left = is_left(a);
        if self.moves.grab.shimmy == Shimmy::Shimmy {
            return;
        }
        self.abort_look_at_target(m);
        let t = self.time;
        let g = &mut self.moves.grab;
        g.shimmy = Shimmy::Shimmy;
        g.shimmy_velocity = 60.0 * if left { -1.0 } else { 1.0 };
        g.shimmy_time = 1.0;
        g.last_shimmy_time_seconds = t;
        self.set_physics(Physics::Flying);
        let free = self.grab_is_hanging_free();
        self.play_move_anim(m, Slot::FullBody, strafe_anim(free, left), 1.0, 0.2, 0.0, false, false);
    }

    /// TdMove_Grab.StartShimmyAroundCorner.
    fn grab_start_shimmy_around_corner(&mut self, m: Move, a: MoveAction) {
        let left = is_left(a);
        let (x, y, _) = self.pawn.rotation.axes();
        self.use_root_motion(true);
        self.set_move_timer(m, 0.1, false, T_START_ROOT_ROTATION);
        let tl = self.moves.grab.target_location;
        let l = if left { tl - x * 63.858 + y * 60.753 } else { tl - x * 63.858 - y * 60.753 };
        self.set_location(l);
        self.abort_look_at_target(m);
        self.reset_camera_look(m, 0.1);
        self.anim.stop(Slot::FullBody, 0.2);
        let free = self.grab_is_hanging_free();
        self.moves.grab.pending_shimmy_corner_animation = match (left, free) {
            (true, true) => "HangFreeCornerOutSideLeft",
            (true, false) => "HangCornerOutSideLeft",
            (false, true) => "HangFreeCornerOutSideRight",
            (false, false) => "HangCornerOutSideRight",
        }
        .to_string();
        self.set_physics(Physics::Flying);
        self.moves.grab.shimmy = Shimmy::AroundCorner;
        self.grab_update_view_constraints(m);
    }

    /// TdMove_Grab.AbortShimmy.
    fn grab_abort_shimmy(&mut self, force_vel_stop: bool) {
        if self.moves.grab.shimmy == Shimmy::AroundCorner {
            return;
        }
        if force_vel_stop {
            self.pawn.velocity = Vec3::ZERO;
            self.pawn.acceleration = Vec3::ZERO;
            self.anim.stop(Slot::FullBody, 0.2);
            self.moves.grab.shimmy = Shimmy::NoShimmy;
            self.moves.grab.shimmy_time = 0.0;
        } else {
            let pos = self.anim.normalized_position(Slot::FullBody);
            if (pos > 0.1 && pos < 0.25) || pos > 0.8 {
                self.anim.stop(Slot::FullBody, 0.4);
                self.moves.grab.shimmy = Shimmy::NoShimmy;
                self.moves.grab.shimmy_time = 0.0;
            }
        }
    }

    /// UTdMove_Grab vt70 (0x1207A70, after the TdMove precise-location update): follow the ledge
    /// sideways, start corner shimmies, and drive ShimmyMove.
    pub fn grab_tick(&mut self, m: Move, dt: f32) {
        if self.moves.grab.shimmy == Shimmy::AroundCorner {
            return;
        }
        let action = if self.moves.grab.shimmy_velocity < 0.0 { MoveAction::ShimmyLeft } else { MoveAction::ShimmyRight };
        let left = action == MoveAction::ShimmyLeft;
        let t = self.grab_check_wall_leg_placement();
        if t != self.moves.grab.grab_type {
            let prev = self.moves.grab.grab_type;
            self.moves.grab.grab_type = t;
            self.moves.grab.previous_grab_type = prev;
            self.grab_update_grab_type(m, left);
        }
        let (_, y, _) = self.pawn.rotation.axes();
        let dir = (if left { -y } else { y }).safe_normal_2d();
        let probe = self.pawn.location + dir * 5.0;
        let rot = self.pawn.rotation;
        let saved = (self.pawn.move_ledge_location, self.pawn.move_ledge_normal, self.pawn.move_normal);
        let r = self.detect_possible_hand_plant(m, probe, rot, 30.0, false);
        let found = (self.pawn.move_ledge_location, self.pawn.move_ledge_normal, self.pawn.move_normal);
        (self.pawn.move_ledge_location, self.pawn.move_ledge_normal, self.pawn.move_normal) = saved;
        if r == 2 {
            if saved.2.dot(found.2) >= 0.98 {
                (self.pawn.move_ledge_location, self.pawn.move_ledge_normal, self.pawn.move_normal) = found;
                if self.moves.grab.shimmy_velocity.abs() > 0.0 {
                    self.grab_shimmy_move(dt);
                }
                return;
            }
        } else if self.grab_can_shimmy_around_corner(action) {
            let target = self.moves.grab.target_location;
            let trot = Rotator::new(0, self.moves.grab.target_yaw, 0);
            let dist = self.moves.base(m).hand_plant_check_distance;
            if self.detect_possible_hand_plant(m, target, trot, dist, false) == 2 {
                let g = &self.moves.grab;
                let mut tl = self.pawn.move_ledge_location + self.pawn.move_normal * (g.relative_extent + g.desired_ledge_offset.x);
                tl.z -= g.desired_ledge_offset.z;
                self.moves.grab.target_location = tl;
                self.grab_abort_shimmy(false);
                self.grab_start_shimmy_around_corner(m, action);
                return;
            }
            (self.pawn.move_ledge_location, self.pawn.move_ledge_normal, self.pawn.move_normal) = saved;
        }
        self.moves.grab.shimmy_velocity = 0.0;
        self.grab_abort_shimmy(true);
    }

    /// UTdMove_Grab::ShimmyMove (0x11F7D60).
    fn grab_shimmy_move(&mut self, dt: f32) {
        let (_, y, _) = self.pawn.rotation.axes();
        let rate = self.anim.slots.get(&Slot::FullBody).map(|s| s.play_rate).unwrap_or(1.0);
        let s = rate * self.moves.grab.shimmy_velocity;
        self.pawn.velocity = y.safe_normal_2d() * s;
        self.moves.grab.shimmy_time -= dt;
        if self.moves.grab.shimmy_time < 0.0 && self.moves.grab.shimmy_velocity < 0.0001 {
            self.grab_abort_shimmy(false);
        }
    }

    /// TdMove_Grab move timers.
    pub fn grab_on_move_timer(&mut self, m: Move, id: u8) {
        // TdMove_Grab.SetDPG: the arms go depth-tested so the ledge hides the fingertips;
        // OnChangeDPGTimer (dropping off) puts them back in front.
        if id == T_SET_DPG {
            self.pawn.first_person_dpg = crate::pawn::Dpg::Intermediate;
        }
        if id == T_CHANGE_DPG {
            self.pawn.first_person_dpg = crate::pawn::Dpg::Foreground;
        }
        if id == T_START_ROOT_ROTATION {
            // TdMove_Grab.StartRootRotation
            self.use_root_rotation(true);
            let name = self.moves.grab.pending_shimmy_corner_animation.clone();
            self.play_move_anim(m, Slot::FullBody, &name, 1.0, 0.15, 0.15, true, true);
        }
    }

    /// TdMove_Grab.OnCustomAnimEnd.
    pub fn grab_on_custom_anim_end(&mut self, m: Move, seq_name: &str) {
        if self.moves.grab.shimmy == Shimmy::AroundCorner {
            if seq_name.eq_ignore_ascii_case(&self.moves.grab.pending_shimmy_corner_animation) && self.moves.grab.target_yaw != -1 {
                let mut r = self.pawn.rotation;
                r.yaw = self.moves.grab.target_yaw;
                self.set_rotation(r);
                let tl = self.moves.grab.target_location;
                self.set_location(tl);
                self.moves.grab.target_yaw = -1;
                self.moves.grab.shimmy_time = 0.0;
                let rad = self.pawn.collision_radius;
                let rel = self.calculate_relative_extent(rad);
                self.moves.grab.relative_extent = rel;
                let free = self.grab_is_hanging_free();
                self.moves.base_mut(m).root_offset.x = rel + if free { 0.0 } else { 1.0 };
            } else {
                return;
            }
        }
        self.use_root_motion(false);
        self.use_root_rotation(false);
        self.moves.grab.shimmy = Shimmy::NoShimmy;
        self.pawn.velocity = Vec3::ZERO;
        self.set_physics(Physics::None);
        self.grab_update_view_constraints(m);
        match self.moves.grab.folded {
            Folded::Start => {
                if self.moves.grab.climb_up_folded_action_received && self.can_do_move(Move::GrabPullUp) {
                    self.moves.grab.climb_up_folded_action_received = false;
                    self.set_move(Move::GrabPullUp, false, false);
                } else {
                    if self.grab_is_hanging_free() {
                        self.play_move_anim(m, Slot::FullBody, "HangFreeFoldedEndHangFree", 1.0, 0.0, 0.2, false, false);
                        self.set_move_countdown(m, 1.5);
                    } else {
                        self.play_move_anim(m, Slot::FullBody, "HangFoldedEndHang", 1.0, 0.0, 0.2, false, false);
                    }
                    self.moves.grab.folded = Folded::End;
                    self.moves.base_mut(m).constrain_look = false;
                }
            }
            Folded::End => {
                self.moves.grab.folded = Folded::None;
                self.moves.base_mut(m).constrain_look = true;
            }
            Folded::None => {}
        }
        if self.moves.grab.hang_free_vertigo_effect {
            self.moves.grab.hang_free_vertigo_effect = false;
            self.moves.base_mut(m).constrain_look = true;
        }
        self.pawn.current_grab_turn_type = match self.pawn.current_grab_turn_type {
            GrabTurn::Start => GrabTurn::Idle,
            GrabTurn::End => GrabTurn::None,
            t => t,
        };
        if self.moves.grab.request_drop_down {
            self.set_move(Move::Falling, false, false);
        }
    }

    /// TdMove_Grab.OnTimer.
    pub fn grab_on_timer(&mut self, m: Move) {
        match self.pawn.current_grab_turn_type {
            GrabTurn::End => self.pawn.current_grab_turn_type = GrabTurn::None,
            GrabTurn::Start => self.pawn.current_grab_turn_type = GrabTurn::Idle,
            _ => {
                self.abort_look_at_target(m);
                let mut la = self.pawn.rotation;
                la.pitch += 9000;
                self.set_look_at_target_angle(m, la, 0.2, 0.5);
            }
        }
    }

    /// TdMove_Grab.UpdateViewRotation.
    pub fn grab_update_view_rotation(&mut self, m: Move, view: &mut Rotator, dt: f32, delta: &mut Rotator) {
        if self.moves.grab.hang_free_vertigo_effect || self.moves.grab.folded != Folded::None {
            delta.pitch = 0;
            delta.yaw = 0;
        }
        if delta.yaw != 0 || delta.pitch != 0 {
            self.abort_look_at_target(m);
        }
        self.tdmove_update_view_rotation(m, view, dt, delta);
        let cd = (view.normalize() - self.pawn.rotation.normalize()).normalize();
        let sta = self.moves.grab.start_turning_angle;
        self.moves.grab.is_within_forward_view = (cd.yaw as f32) < sta && (cd.yaw as f32) > -sta;
        self.moves.grab.is_turned_right = cd.yaw >= 0;
        let free = self.grab_is_hanging_free();
        let should_trigger_free_turn = free
            && (cd.yaw >= HANG_FREE_MAX.yaw - 750 || cd.yaw <= HANG_FREE_MIN.yaw + 750)
            && !self.moves.grab.hang_free_vertigo_effect;
        if !free {
            if self.pawn.current_grab_turn_type == GrabTurn::None {
                if cd.yaw as f32 > sta {
                    self.play_move_anim(m, Slot::FullBody, "HangTurnRightStart", 1.0, 0.2, 0.2, false, false);
                    self.pawn.current_grab_turn_type = GrabTurn::Start;
                    self.set_move_countdown(m, 0.2);
                } else if (cd.yaw as f32) < -sta {
                    self.play_move_anim(m, Slot::FullBody, "HangTurnLeftStart", 1.0, 0.2, 0.2, false, false);
                    self.pawn.current_grab_turn_type = GrabTurn::Start;
                    self.set_move_countdown(m, 0.2);
                } else if self.moves.base(m).move_active_time > 1.0 {
                    // ClearTimer
                    self.moves.base_mut(m).timer = 0.0;
                }
            } else if self.moves.grab.is_within_forward_view {
                if self.pawn.current_grab_turn_type == GrabTurn::Idle {
                    let n = if self.moves.grab.is_turned_right { "HangTurnRightEnd" } else { "HangTurnLeftEnd" };
                    self.play_move_anim(m, Slot::FullBody, n, 1.0, 0.2, 0.2, false, false);
                    self.pawn.current_grab_turn_type = GrabTurn::End;
                    self.set_move_countdown(m, 0.6);
                } else if self.pawn.current_grab_turn_type == GrabTurn::Start {
                    self.anim.stop(Slot::FullBody, 0.2);
                    self.pawn.current_grab_turn_type = GrabTurn::None;
                }
            }
        } else {
            self.pawn.current_grab_turn_type = GrabTurn::None;
        }
        let t = self.moves.base(m).move_active_time;
        let g = &self.moves.grab;
        if t > g.start_looking_at_ledge_time && t < g.stop_looking_at_ledge_time && g.folded == Folded::None && !g.grab_from_vertical_wallrun {
            let ledge = self.pawn.move_ledge_location;
            self.set_look_at_target_location(m, ledge, 0.3, 0.25);
        }
        if should_trigger_free_turn {
            self.moves.grab.hang_free_vertigo_effect = true;
            let n = if self.moves.grab.is_turned_right { "HangFreeTurnRight" } else { "HangFreeTurnLeft" };
            self.play_move_anim(m, Slot::FullBody, n, 1.0, 0.2, 0.2, false, false);
            let mut la = self.pawn.rotation;
            la.pitch = 0;
            self.set_look_at_target_angle(m, la, 0.2, 0.2);
            self.moves.base_mut(m).constrain_look = false;
            self.set_move_countdown(m, 1.75);
        }
    }

    // ---------------------------------------------------------------- GrabPullUp

    /// TdMove_GrabPullUp.CanDoMove.
    pub fn grab_pull_up_can_do_move(&mut self, m: Move) -> bool {
        if !self.tdmove_can_do_move(m) || self.pawn.movement_state != Move::Grabbing || !self.grab_can_pull_up() {
            return false;
        }
        let cr = (self.pawn.rotation - self.pc.rotation).normalize();
        if (cr.yaw as f32).abs() > (32768 * self.moves.grab_pull_up.allowed_pull_up_angle) as f32 / 180.0 {
            return false;
        }
        let mut floor = self.pawn.location;
        floor.z -= self.pawn.collision_height;
        let max_ledge_thickness = 5.0;
        let h = self.pawn.max_step_height + 4.0;
        self.find_floor_over_ledge(64.0 + max_ledge_thickness, &mut floor, h);
        self.moves.grab_pull_up.floor_over_ledge_location = floor;
        let delta = floor.z - self.pawn.move_ledge_location.z;
        if delta > -self.pawn.max_step_height {
            self.grab_pull_up_can_pull_up(63.0)
        } else if !self.grab_pull_up_can_pull_up(90.0 + max_ledge_thickness) {
            self.moves.grab_pull_up.floor_over_ledge_location.z = self.pawn.move_ledge_location.z;
            self.grab_pull_up_can_pull_up(63.0)
        } else {
            true
        }
    }

    /// TdMove_GrabPullUp.CanPullUp.
    fn grab_pull_up_can_pull_up(&mut self, depth: f32) -> bool {
        let highest = self.pawn.move_ledge_location.z + self.pawn.move_ledge_normal.z.acos().tan() * self.pawn.default_collision_radius;
        let floor = self.moves.grab_pull_up.floor_over_ledge_location;
        if self.can_heave_over_ledge_fully_extended(depth, floor, highest) {
            self.moves.grab_pull_up.into_crouch = false;
            return true;
        }
        if self.moves.grab.folded != Folded::Start && self.can_heave_over_ledge_crouched(depth, floor, highest) {
            self.moves.grab_pull_up.into_crouch = true;
            return floor.z >= self.pawn.move_ledge_location.z - self.pawn.max_step_height;
        }
        false
    }

    /// TdMove_GrabPullUp.StartMove.
    pub fn grab_pull_up_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        let heave_over = self.moves.grab_pull_up.floor_over_ledge_location.z < self.pawn.move_ledge_location.z - self.pawn.max_step_height;
        let free = self.grab_is_hanging_free();
        let crouch = self.moves.grab_pull_up.into_crouch;
        let (release, enable) = if self.moves.grab.folded == Folded::Start {
            self.play_move_anim(m, Slot::FullBody, "HangFoldedHeaveUp", 1.0, 0.1, 0.2, true, false);
            self.moves.grab.folded = Folded::None;
            (0.8, 0.8)
        } else if !heave_over {
            let a = match (free, crouch) {
                (true, true) => "hangfreeheaveuptocrouch",
                (true, false) => "HangFreeHeaveUp",
                (false, true) => "HangHeaveUpToCrouch",
                (false, false) => "HangHeaveUp",
            };
            self.play_move_anim(m, Slot::FullBody, a, 1.0, 0.1, 0.2, true, false);
            (0.8, 1.4)
        } else {
            let a = if free { "HangFreeHeaveOver" } else { "HangHeaveOver" };
            self.play_move_anim(m, Slot::FullBody, a, 1.0, 0.2, 0.2, true, false);
            (if free { 0.7 } else { 0.6 }, 0.6)
        };
        self.set_animation_movement_state(Move::Grabbing, -1.0);
        let after = if crouch && !heave_over { Move::Crouch } else { Move::None };
        self.set_animation_movement_state(after, 0.2);
        self.moves.base_mut(m).disable_face_rotation = true;
        self.use_root_motion(true);
        let dlt = self.moves.base(m).disable_look_time;
        self.reset_camera_look(m, dlt);
        self.set_move_timer(m, release, false, T_RELEASE_CAMERA);
        self.set_move_timer(m, enable, false, T_ENABLE_COLLISION);
    }

    pub fn grab_pull_up_on_move_timer(&mut self, m: Move, id: u8) {
        match id {
            T_RELEASE_CAMERA => {
                self.moves.base_mut(m).disable_face_rotation = false;
                self.pawn.face_rotation_time_left = 0.25;
                self.abort_look_at_target(m);
            }
            T_ENABLE_COLLISION => self.grab_pull_up_enable_collision(),
            _ => {}
        }
    }

    /// TdMove_GrabPullUp.EnableCollision.
    fn grab_pull_up_enable_collision(&mut self) {
        self.pawn.collide_world = true;
        self.stop_ignore_move_input();
    }

    /// TdMove_GrabPullUp.StopMove.
    pub fn grab_pull_up_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.grab_pull_up_enable_collision();
    }

    /// TdMove_GrabPullUp.OnCustomAnimEnd.
    pub fn grab_pull_up_on_custom_anim_end(&mut self, _m: Move) {
        self.use_root_motion(false);
        self.pawn.acceleration = self.pawn.velocity.safe_normal();
        self.set_physics(Physics::Walking);
        let next = if self.moves.grab_pull_up.into_crouch { Move::Crouch } else { Move::Walking };
        self.set_move(next, false, false);
    }

    // ---------------------------------------------------------------- GrabJump

    /// TdMove_GrabJump.CanDoMove.
    pub fn grab_jump_can_do_move(&mut self, m: Move) -> bool {
        if !self.tdmove_can_do_move(m) {
            return false;
        }
        let cr = (self.pawn.rotation - self.pc.rotation).normalize();
        if (cr.yaw as f32).abs() < 32768.0 * self.moves.grab_jump.allowed_jump_angle / 180.0 {
            return false;
        }
        if self.pawn.movement_state == Move::Grabbing && self.grab_is_hanging_free() {
            return false;
        }
        matches!(self.pawn.movement_state, Move::Grabbing | Move::Climb)
    }

    /// TdMove_GrabJump.StartMove.
    pub fn grab_jump_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        self.pawn.last_jump_location = self.pawn.location;
        let mut cam = self.pc.rotation.vector();
        cam.z = 0.0;
        let cam = cam.safe_normal();
        let gj = &self.moves.grab_jump;
        let i = cam.dot(self.pawn.move_normal).max(0.0);
        let push = i * gj.push_away_max_speed + (1.0 - i) * gj.push_away_min_speed;
        let mut h = gj.off_z_height;
        let start = self.pawn.location;
        let mut end = start;
        end.z += h;
        if let Some(hit) = self.movement_trace(end, start, self.pawn.extent()) {
            h = hit.location.z - self.pawn.location.z;
        }
        let g = self.pawn.gravity_z().abs();
        let speed = g * 2.0 * (h / g).sqrt();
        let mut v = cam * push;
        v.z = speed;
        self.moves.grab_jump.jump_velocity = v;
        self.play_move_anim(m, Slot::FullBody, "HangTurnJump", 1.0, 0.2, 0.2, false, false);
        self.set_animation_movement_state(Move::Grabbing, -1.0);
        self.moves.base_mut(m).disable_face_rotation = true;
        self.set_move_countdown(m, 0.1);
        let d = (self.pc.rotation.normalize() - self.pawn.rotation.normalize()).yaw;
        self.moves.grab_jump.delta_jump_yaw = norm_axis(d);
    }

    /// TdMove_GrabJump.OnTimer.
    pub fn grab_jump_on_timer(&mut self, m: Move) {
        self.pawn.velocity = self.moves.grab_jump.jump_velocity;
        self.pawn.face_rotation_time_left = 0.3;
        self.pawn.leg_rotation = self.pc.rotation.yaw;
        self.moves.base_mut(m).disable_face_rotation = false;
    }

    /// TdMove_GrabJump.UpdateViewRotation (before super).
    pub fn grab_jump_view_rotation(&mut self, m: Move, delta: &mut Rotator) {
        let d = self.moves.grab_jump.delta_jump_yaw;
        if self.moves.base(m).move_active_time < 0.05 && ((delta.yaw > 300 && d >= 27000) || (delta.yaw < -300 && d < -27000)) {
            delta.yaw = 0;
        }
    }

    /// TdMove_GrabJump.StopMove.
    pub fn grab_jump_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.set_animation_movement_state(Move::None, -1.0);
        self.anim.stop(Slot::FullBody, 0.2);
    }
}
