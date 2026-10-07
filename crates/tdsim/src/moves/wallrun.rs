//! TdMove_WallRun (script) + UTdMove_WallRun natives (FindWallForward 0x11F67B0, FindWallSide
//! 0x11F5A10, vt70 0x12073C0). One instance each for MOVE_WallRunningRight (4),
//! MOVE_WallRunningLeft (5) and the detector MOVE_WallRun (40).

use crate::config::Config;
use crate::controller::CtrlState;
use crate::math::{Rotator, UeVec, Vec3, norm_axis};
use crate::natives::LedgeHitInfo;
use crate::pawn::{Move, MoveAction, MoveActionHint, Physics, Slot};
use crate::sim::Sim;

#[derive(Clone)]
pub struct WallRun {
    pub forward_check_distance: f32,
    pub strafe_check_distance: f32,
    pub min_wall_height: f32,
    pub min_speed: f32,
    pub velocity_start_limit: f32,
    pub velocity_stop_limit: f32,
    pub forward_min_start_angle: f32,
    pub forward_max_start_angle: f32,
    pub strafe_start_angle: f32,
    pub horisontal_friction: f32,
    pub horisontal_initial_z_height: f32,
    pub horisontal_acceleration: f32,
    pub horisontal_deceleration: f32,
    pub default_horisontal_acceleration: f32,
    pub default_horisontal_deceleration: f32,
    pub horisontal_align_speed: f32,
    pub into_wallrun_blend_in_time: f32,
    pub into_wallrun_blend_out_time: f32,
    pub play_camera_hit_wall_effect: bool,
    pub delay_pawn_rotation_time: f32,
    pub rotate_pawn_along_wall_time: f32,
    pub move_to_into_position_degree_threshold: f32,
    pub start_upper_body_anim_play_rate: f32,
    pub has_reached_wall: bool,
    pub start_moving_into_wall: bool,
    pub turned_90_from_wall: bool,
    pub changed_constraints: bool,
    pub begin_speed: f32,
    pub next_move: Move,
    pub wall_normal: Vec3,
    pub predicted_wall_hit_location: Vec3,
    pub consequtive_wallruns: i32,
    pub min_constraint_world: i32,
    pub max_constraint_world: i32,
}

impl WallRun {
    pub fn new(cfg: &Config) -> Self {
        let c = &["TdMove_WallRun"];
        let acc = cfg.f32(c, "WallRunningHorisontalAcceleration", 820.0);
        let dec = cfg.f32(c, "WallRunningHorisontalDeceleration", 500.0);
        WallRun {
            forward_check_distance: cfg.f32(c, "WallRunningForwardCheckDistance", 50.0),
            strafe_check_distance: cfg.f32(c, "WallRunningStrafeCheckDistance", 50.0),
            min_wall_height: cfg.f32(c, "WallRunningMinWallHeight", 192.0),
            min_speed: cfg.f32(c, "WallRunningMinSpeed", 200.0),
            velocity_start_limit: cfg.f32(c, "WallRunningVelocityStartLimit", 300.0),
            velocity_stop_limit: cfg.f32(c, "WallRunningVelocityStopLimit", -500.0),
            forward_min_start_angle: cfg.f32(c, "WallRunningForwardMinStartAngle", 0.0),
            forward_max_start_angle: cfg.f32(c, "WallRunningForwardMaxStartAngle", 57.0),
            strafe_start_angle: cfg.f32(c, "WallRunningStrafeStartAngle", 60.0),
            horisontal_friction: cfg.f32(c, "WallRunningHorisontalFriction", 0.05),
            horisontal_initial_z_height: cfg.f32(c, "WallRunningHorisontalInitialZHeight", 170.0),
            horisontal_acceleration: acc,
            horisontal_deceleration: dec,
            default_horisontal_acceleration: acc,
            default_horisontal_deceleration: dec,
            horisontal_align_speed: cfg.f32(c, "WallRunningHorisontalAlignSpeed", 700.0),
            into_wallrun_blend_in_time: cfg.f32(c, "WallRunningIntoWallrunBlendInTime", 0.2),
            into_wallrun_blend_out_time: cfg.f32(c, "WallRunningIntoWallrunBlendOutTime", 0.2),
            play_camera_hit_wall_effect: cfg.bool(c, "PlayCameraHitWallEffect", true),
            delay_pawn_rotation_time: cfg.f32(c, "WallRunningDelayPawnRotationTime", 0.01),
            rotate_pawn_along_wall_time: cfg.f32(c, "WallRunningRotatePawnAlongWallTime", 0.4),
            move_to_into_position_degree_threshold: cfg.f32(c, "WallRunningMoveToIntoPositionDegreeThreshold", 70.0),
            start_upper_body_anim_play_rate: cfg.f32(c, "WallrunStartUpperBodyAnimPlayRate", 0.6),
            has_reached_wall: false,
            start_moving_into_wall: false,
            turned_90_from_wall: false,
            changed_constraints: false,
            begin_speed: 0.0,
            next_move: Move::None,
            wall_normal: Vec3::ZERO,
            predicted_wall_hit_location: Vec3::ZERO,
            consequtive_wallruns: 0,
            min_constraint_world: 0,
            max_constraint_world: 0,
        }
    }
}

impl Sim {
    /// Moves[4], [5] and [40] are one TdMove_WallRun object (ATdPawn::InitMoveObjects).
    fn wr(&mut self, _m: Move) -> &mut WallRun {
        &mut self.moves.wallrun
    }

    /// The box sweep TdMove natives use with the default cylinder: (R, R, 2).
    fn wall_probe_extent(&self) -> Vec3 {
        let r = self.pawn.default_collision_radius;
        Vec3::new(r, r, 2.0)
    }

    /// Shared tail of FindWallForward / FindWallSide: from the confirming hit, sweep the body
    /// (lifted by half a step) along `dir` into the wall and build the predicted contact.
    fn wall_contact(&self, check_dist: f32, dir: Vec3, n: Vec3, prev_hit_loc: Vec3, same_plane_tol: f32) -> Option<LedgeHitInfo> {
        let inv = 1.0 / (-n).dot(dir);
        let mut start = self.pawn.location;
        start.z += self.pawn.max_step_height * 0.5;
        let end = start + dir * (check_dist * inv);
        let mut ext = self.pawn.extent();
        ext.z -= self.pawn.max_step_height * 0.5;
        let h3 = self.world.line_check(end, start, ext);
        if !h3.hit {
            return None;
        }
        let d2 = (prev_hit_loc - h3.location).safe_normal_2d();
        if d2.dot(h3.normal).abs() > same_plane_tol {
            return None;
        }
        let n3 = h3.normal.safe_normal_2d();
        let m = n3.x.abs().max(n3.y.abs());
        let mut off = (m * (1.0 - (m * m).min(1.0)).sqrt() * 0.828_427 + 1.0) * ext.x;
        let dir2 = dir.safe_normal_2d();
        let c = dir2.dot(n3).abs();
        if c > 0.0 {
            off /= c;
        }
        Some(LedgeHitInfo {
            ledge_location: h3.location + dir2 * off,
            ledge_normal: h3.normal,
            move_normal: h3.normal,
            feet_excluded: h3.surface.exclude_foot_moves,
            hands_excluded: h3.surface.exclude_hand_moves,
        })
    }

    /// UTdMove_WallRun::FindWallForward.
    pub fn find_wall_forward(&self, m: Move) -> (Move, LedgeHitInfo) {
        let w = &self.moves.wallrun;
        let b = self.moves.base(m);
        let p = &self.pawn;
        let fwd = p.rotation.vector();
        let mut loc = p.location;
        let speed = p.velocity.size_2d();
        let extra = (speed - p.speed_max_base_velocity).max(0.0);
        let dist = ((b.context_move_distance_multiplier - 1.0) * extra / (p.ground_speed - p.speed_max_base_velocity) + 1.0) * w.forward_check_distance;
        let ext = self.wall_probe_extent();
        let none = (Move::None, LedgeHitInfo::default());
        let h1 = self.world.line_check(loc + fwd * dist, loc, ext);
        if !h1.hit {
            return none;
        }
        let d = (-h1.normal).dot(fwd);
        if ((90.0 - w.forward_min_start_angle).to_radians()).cos() > d || d > ((90.0 - w.forward_max_start_angle).to_radians()).cos() {
            return none;
        }
        if p.illegal_ledge_timer > 0.0 && p.illegal_ledge_normal.dot(h1.normal) > 0.98 {
            return none;
        }
        // is the wall tall enough?
        loc.z += (w.min_wall_height - 2.0) - (1.0 - d) * 50.0 - p.collision_height;
        let h2 = self.world.line_check(loc + fwd * dist, loc, ext);
        if !h2.hit {
            return none;
        }
        let n = h2.normal;
        let t = Vec3::new(n.y, -n.x, 0.0);
        let s = if t.dot(fwd) >= 0.0 { 1.0 } else { -1.0 };
        if p.velocity.dot(t * s) < 0.0 {
            return none;
        }
        let rel = h2.location - loc;
        let k = -(rel.x * n.x + rel.y * n.y).abs();
        let mut along = rel - n * k;
        if along.dot(fwd) <= 0.0 {
            along = -along;
        }
        let along = along.safe_normal();
        let vdir = p.velocity.safe_normal();
        let thr = (w.move_to_into_position_degree_threshold * 0.011_111_111).clamp(0.0, 1.0);
        let a = thr.max((-n).dot(vdir));
        let bb = thr.max(vdir.dot(along));
        let dir = ((-n) * speed * a + along * speed * bb).safe_normal_2d();
        let Some(hit) = self.wall_contact(w.forward_check_distance, dir, n, h2.location, 0.05) else { return none };
        let right = Vec3::new(-fwd.y, fwd.x, 0.0);
        let side = if (-hit.move_normal).dot(right) <= 0.0 { Move::WallRunningLeft } else { Move::WallRunningRight };
        (side, hit)
    }

    /// UTdMove_WallRun::FindWallSide.
    pub fn find_wall_side(&self, m: Move, side: Move) -> Option<LedgeHitInfo> {
        let w = &self.moves.wallrun;
        let p = &self.pawn;
        let fwd = p.rotation.vector();
        let s = if side == Move::WallRunningRight { -1.0 } else { 1.0 };
        let sd = Vec3::new(fwd.y, -fwd.x, 0.0) * s;
        let ext = self.wall_probe_extent();
        let mut loc = p.location;
        loc.z += (w.min_wall_height - 2.0) - p.collision_height;
        let h1 = self.world.line_check(loc + sd * w.strafe_check_distance, loc, ext);
        if !h1.hit {
            return None;
        }
        let n = h1.normal;
        if w.strafe_start_angle.to_radians().cos() > (-n).dot(sd).abs() {
            return None;
        }
        if p.illegal_ledge_timer > 0.0 && p.illegal_ledge_normal.dot(n) < -0.98 {
            return None;
        }
        if p.velocity.dot(Vec3::new(n.y, -n.x, 0.0) * s) < 0.0 {
            return None;
        }
        let mut loc2 = p.location;
        loc2.z += p.max_step_height + 2.0 - p.collision_height;
        let h2 = self.world.line_check(loc2 + sd * w.strafe_check_distance, loc2, ext);
        if !h2.hit {
            return None;
        }
        let vert = (h1.location - h2.location).safe_normal();
        if (h1.location - h2.location).length_squared() < 1e-8 || vert.z < 0.96 {
            return None;
        }
        let n2 = h2.normal;
        let rel = h2.location - loc2;
        let k = -(rel.x * n2.x + rel.y * n2.y).abs();
        let mut along = rel - n2 * k;
        if along.dot(fwd) <= 0.0 {
            along = -along;
        }
        let along = along.safe_normal();
        let dir = ((-n2) * w.horisontal_align_speed + along * w.velocity_start_limit).safe_normal_2d();
        self.wall_contact(w.strafe_check_distance, dir, n2, h1.location, 0.1)
    }

    /// TdMove_WallRun.CanDoMove.
    pub fn wallrun_can_do_move(&mut self, m: Move) -> bool {
        if !self.tdmove_can_do_move(m) {
            return false;
        }
        let min_speed = self.wr(m).min_speed;
        if self.pawn.velocity.size_sq_2d() < min_speed * min_speed {
            return false;
        }
        if self.pawn.velocity.dot(self.pawn.rotation.vector()) < 0.0 {
            return false;
        }
        let jump_delta = (self.pawn.location - self.pawn.last_jump_location).length();
        let mut wanted = Move::None;
        if jump_delta < 30.0 {
            match self.pawn.move_action_hint {
                MoveActionHint::Right => wanted = Move::WallRunningRight,
                MoveActionHint::Left => wanted = Move::WallRunningLeft,
                _ => {}
            }
        }
        let side_hit = if wanted != Move::None { self.find_wall_side(m, wanted) } else { None };
        let (next, hit) = match side_hit {
            Some(h) => (wanted, h),
            None => self.find_wall_forward(m),
        };
        self.wr(m).next_move = next;
        if next == Move::None || hit.feet_excluded {
            return false;
        }
        if self.pawn.velocity.dot(hit.move_normal) >= 0.0 {
            return false;
        }
        if self.pawn.velocity.z < 0.0 {
            // falling: make sure there is wall to run on where we would end up
            let mut start = hit.ledge_location + hit.move_normal * self.pawn.collision_radius;
            start.z -= self.pawn.collision_height * 2.0;
            let half = self.pawn.velocity.size_2d() * 0.5;
            let up = if next == Move::WallRunningRight { Vec3::new(0.0, 0.0, -1.0) } else { Vec3::new(0.0, 0.0, 1.0) };
            let mut end = hit.move_normal.cross(up) * half;
            end.z -= 45.0;
            end += start;
            if self.movement_trace_for_blocking(end, start, Vec3::new(1.0, 1.0, 0.0)) {
                return false;
            }
        }
        self.pawn.found_ledge_excludes_foot_moves = hit.feet_excluded;
        self.pawn.found_ledge_excludes_hand_moves = hit.hands_excluded;
        self.pawn.move_normal = hit.move_normal;
        let w = self.wr(m);
        w.wall_normal = hit.move_normal;
        w.predicted_wall_hit_location = hit.ledge_location;
        true
    }

    /// TdMove_WallRun.StartMove (only ever entered as WallRunningRight / Left).
    pub fn wallrun_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        let old = self.pawn.old_movement_state;
        let speed2d = self.pawn.velocity.size_2d();
        let vz = self.pawn.velocity.z;
        {
            let w = self.wr(m);
            w.has_reached_wall = false;
            w.start_moving_into_wall = true;
            w.changed_constraints = false;
            w.turned_90_from_wall = false;
            w.begin_speed = speed2d;
            if vz > 0.0 {
                w.begin_speed = w.begin_speed.max(w.velocity_start_limit);
            }
            if old == Move::WallRunJump {
                w.consequtive_wallruns += 1;
                let k = (1 + w.consequtive_wallruns) as f32;
                w.horisontal_deceleration = w.default_horisontal_deceleration * k;
                w.horisontal_acceleration = w.default_horisontal_acceleration * k;
            } else {
                w.consequtive_wallruns = 0;
                w.horisontal_deceleration = w.default_horisontal_deceleration;
                w.horisontal_acceleration = w.default_horisontal_acceleration;
            }
        }
        self.set_physics(Physics::WallRunning);
        self.wr(m).has_reached_wall = false;
        self.wr(m).start_moving_into_wall = false;
        // initial upward speed from the head room above
        let h0 = self.wr(m).horisontal_initial_z_height;
        let start = self.pawn.location;
        let end = start + Vec3::new(0.0, 0.0, h0);
        let mut height = match self.movement_trace(end, start, self.pawn.extent()) {
            Some(h) => h.location.z - self.pawn.location.z,
            None => h0,
        };
        height -= self.pawn.location.z - self.pawn.last_jump_location.z;
        let height = (height as i32).max(0) as f32;
        let speed = (2.0 * height * self.wr(m).horisontal_acceleration).sqrt();
        if self.pawn.velocity.z > 0.0 && old != Move::WallRunJump {
            self.pawn.velocity.z = speed;
        }
        let wn = self.wr(m).wall_normal;
        self.pawn.floor = wn; // SetBase(MovementActor, WallNormal)
        let delay = self.wr(m).delay_pawn_rotation_time;
        self.set_move_countdown(m, delay);
        // upper-body start anim plays slower if there is wall above to look at
        let r = self.pawn.collision_radius;
        let ext = Vec3::new(r, r, 10.0);
        let mut s = self.wr(m).predicted_wall_hit_location;
        s.z += self.pawn.collision_height + 30.0 + ext.z;
        let e = s + (-wn) * self.wr(m).strafe_check_distance;
        let rate = if self.movement_trace_for_blocking(e, s, ext) { self.wr(m).start_upper_body_anim_play_rate } else { 1.0 };
        let (bi, bo) = (self.wr(m).into_wallrun_blend_in_time, self.wr(m).into_wallrun_blend_out_time);
        let wyaw = Rotator::from_vector(wn).yaw;
        if m == Move::WallRunningRight {
            self.play_move_anim(m, Slot::FullBody, "wallrunrightstart", rate, bi, bo, false, false);
            self.wr(m).min_constraint_world = norm_axis(wyaw);
            self.wr(m).max_constraint_world = norm_axis(wyaw + 16384);
        } else {
            self.play_move_anim(m, Slot::FullBody, "wallrunleftstart", rate, bi, bo, false, false);
            self.wr(m).min_constraint_world = norm_axis(wyaw - 16384);
            self.wr(m).max_constraint_world = norm_axis(wyaw);
        }
        let (mn, mx) = (self.wr(m).min_constraint_world, self.wr(m).max_constraint_world);
        let b = self.moves.base_mut(m);
        b.min_look_constraint.yaw = mn;
        b.max_look_constraint.yaw = mx;
    }

    /// TdMove_WallRun.ReachedWall.
    pub fn wallrun_reached_wall(&mut self, m: Move) {
        let begin = self.wr(m).begin_speed;
        let mut v2 = self.pawn.velocity;
        v2.z = 0.0;
        let v2 = v2.safe_normal() * begin;
        self.pawn.velocity.x = v2.x;
        self.pawn.velocity.y = v2.y;
        if self.wr(m).play_camera_hit_wall_effect {
            let name = if m == Move::WallRunningRight { "wallrunimpactright" } else { "wallrunimpactleft" };
            self.play_move_anim(m, Slot::Camera, name, 1.0, 0.15, 0.15, false, false);
        }
        self.wr(m).has_reached_wall = true;
        self.moves.base_mut(m).disable_face_rotation = true;
    }

    /// TdMove_WallRun.OnTimer -> FacePawnAlongWall.
    pub fn wallrun_on_timer(&mut self, m: Move) {
        let wn = self.wr(m).wall_normal;
        let mut r = Rotator::from_vector(wn);
        r.yaw += if m == Move::WallRunningRight { 16384 } else { -16384 };
        self.pawn.move_normal = wn;
        let t = self.wr(m).rotate_pawn_along_wall_time;
        self.set_precise_rotation(m, r, t);
    }

    /// TdMove_WallRun.HandleMoveAction (Turn: look away from the wall).
    pub fn wallrun_handle_move_action(&mut self, m: Move, a: MoveAction) {
        if a == MoveAction::Turn {
            let mut want = Rotator::from_vector(self.pawn.floor);
            want.pitch = self.pc.rotation.pitch;
            let b = self.moves.base_mut(m);
            b.disable_controller_facing_pawn_yaw_rotation = true;
            b.look_at_target_angle = true;
            b.look_at_target_interpolation_time = 0.15;
            b.look_at_target_duration = b.move_active_time + 1.5;
            b.look_at_target_angle_v = want;
            self.wr(m).turned_90_from_wall = true;
        }
    }

    pub fn wallrun_stop_move(&mut self, m: Move) {
        self.wr(m).next_move = Move::None;
        if !self.wr(m).turned_90_from_wall {
            self.pawn.face_rotation_time_left = 0.3;
            self.pawn.leg_rotation = self.pc.rotation.yaw;
        }
        self.physics_move_stop_move(m);
        self.moves.base_mut(m).disable_face_rotation = false;
        self.moves.base_mut(m).disable_controller_facing_pawn_yaw_rotation = true;
    }

    /// UTdMove_WallRun vt70 (after the TdPhysicsMove part).
    pub fn wallrun_tick(&mut self, m: Move) {
        self.moves.base_mut(m).friction_modifier = 0.0;
        if self.pawn.physics == Physics::Falling {
            self.set_move(Move::Falling, false, false);
            return;
        }
        if self.pawn.velocity_magnitude < 0.1 {
            self.falling_off_wall();
            if self.pawn.physics == Physics::WallRunning {
                self.set_physics(Physics::Falling);
            }
        }
        if !self.wr(m).changed_constraints && self.moves.base(m).move_active_time > 0.3 {
            let y = Rotator::from_vector(self.wr(m).wall_normal).yaw;
            let y = norm_axis(y);
            if m == Move::WallRunningRight {
                self.wr(m).max_constraint_world = y + 20000;
            } else {
                self.wr(m).min_constraint_world = y - 20000;
            }
            self.wr(m).changed_constraints = true;
            let (mn, mx) = (self.wr(m).min_constraint_world, self.wr(m).max_constraint_world);
            let b = self.moves.base_mut(m);
            b.min_look_constraint.yaw = mn;
            b.max_look_constraint.yaw = mx;
        }
        if self.wr(m).has_reached_wall {
            let n = self.pawn.floor;
            let t = Vec3::new(n.y, -n.x, 0.0).safe_normal();
            let speed = self.pawn.velocity.size_2d();
            let s = if m == Move::WallRunningLeft { 1.0 } else { -1.0 };
            self.pawn.velocity.x = s * t.x * speed;
            self.pawn.velocity.y = s * t.y * speed;
            let f = self.wr(m).horisontal_friction;
            self.moves.base_mut(m).friction_modifier = f;
        }
        if !self.wr(m).start_moving_into_wall {
            if self.wr(m).velocity_stop_limit <= self.pawn.velocity.z {
                self.pawn.acceleration = Vec3::ZERO;
                let a = if self.pawn.velocity.z <= 0.0 { self.wr(m).horisontal_deceleration } else { self.wr(m).horisontal_acceleration };
                self.pawn.acceleration.z = -a;
            } else {
                self.falling_off_wall();
                if self.pawn.physics == Physics::WallRunning {
                    self.set_physics(Physics::Falling);
                }
            }
        }
    }

    /// The controller state for wall moves.
    pub fn wallrun_controller_state() -> CtrlState {
        CtrlState::PlayerWallWalking
    }
}
