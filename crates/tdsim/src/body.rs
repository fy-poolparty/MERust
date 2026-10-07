//! Natives that move Faith's body rather than her collision: the leg yaw that the anim tree
//! twists the hips by (ATdPawn::Tick UpdateLegRotation, TdAnimNodeTurn), the 1p mesh's XY
//! translation (OffsetMeshXY and its decay), TdPlayerPawn's against-wall check and the moves'
//! CheckForCameraCollision.

use crate::math::{norm_axis, Rotator, UeVec, Vec3};
use crate::moves::{class_of, Class};
use crate::pawn::{Move, Physics, WalkingState};
use crate::sim::{Sim, TimerFn};

/// TdPawn.EAgainstWallState.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AgainstWall {
    #[default]
    None,
    AgainstWall,
    Left,
    Right,
}

/// TdAnimNodeTurn (TickAnim 0x1214B80, StartTurn 0x12133D0, OnAnimEnd 0x1213540): when the
/// pawn faces more than ExtendedRegionLimit (65 degrees) away from her legs while idle, plays
/// the 90 degree turn step and swings LegRotation round over its length. Children: Default,
/// Turn Left 45, Turn Left 90, Turn Right 45, Turn Right 90. The node lives here because it
/// drives LegRotation; the pose only reads `active_child` and `position`.
#[derive(Clone, Debug, Default)]
pub struct TurnNode {
    pub relevant: bool,
    pub playing_turn_animation: bool,
    pub leg_turn_per_second: f32,
    pub time_standing_still: f32,
    pub active_child: usize,
    /// Play position of the active turn sequence.
    pub position: f32,
}

/// The two TdAnimNodeTurns of AT_C1P: TdAnimNodeTurn_0 (standing) and _18 (crouched).
pub const STAND_TURNS: [&str; 5] = ["Stand", "StandTurn45Left", "StandTurn90Left", "StandTurn45Right", "StandTurn90Right"];
pub const CROUCH_TURNS: [&str; 5] = ["crouchstill", "CrouchTurn45Left", "CrouchTurn90Left", "CrouchTurn45Right", "CrouchTurn90Right"];

impl TurnNode {
    const IDLE_TIMER: f32 = 0.95;
    const SAFE_REGION_LIMIT: f32 = 25.0;
    const EXTENDED_REGION_LIMIT: f32 = 65.0;
}

impl Sim {
    /// ATdPawn::Tick's leg update (0x12BB730): while moving (or in a move with precise
    /// rotation) LegRotation chases the velocity's yaw, or its reverse when running backwards
    /// (bGoingForward flips with LegAngleLimitFudge hysteresis around GoBackLegAngleLimit), and
    /// it always stays within GoBackLegAngleLimitMin/Max of the pawn's yaw.
    pub(crate) fn update_leg_rotation(&mut self, dt: f32) {
        let ms = self.pawn.movement_state;
        let precise_rotation = self.moves.base(ms).use_precise_rotation;
        let p = &mut self.pawn;
        if p.leg_rotation_slow_timer > 0.0 {
            p.leg_rotation_slow_timer = (p.leg_rotation_slow_timer - dt).max(0.0);
        }
        let speed = p.velocity.size_2d();
        if (!matches!(ms, Move::BotStartWalking | Move::BotStartRunning) && speed > 20.0) || precise_rotation {
            let mut dir = Vec3::new(p.velocity.x, p.velocity.y, 0.0).safe_normal();
            if p.physics == Physics::Falling {
                let f = p.rotation.vector();
                dir = Vec3::new(f.x, f.y, 0.0).safe_normal();
            }
            // 0x12B0850: the yaw of a flat unit vector
            let k = 32768.0 / std::f32::consts::PI;
            let a = dir.x.clamp(-1.0, 1.0).acos();
            let mut yaw = ((if dir.y > 0.0 { a } else { -a }) * k) as i32;
            let rel = norm_axis(yaw - p.rotation.yaw);
            let (min, max, fudge) = (p.go_back_leg_angle_limit_min, p.go_back_leg_angle_limit_max, p.leg_angle_limit_fudge);
            if p.going_forward && (rel > fudge + max || rel < min - fudge) {
                p.going_forward = false;
                p.leg_rotation_slow_timer = 0.4;
            } else if !p.going_forward && rel < max - fudge && rel > fudge + min {
                p.going_forward = true;
                p.leg_rotation_slow_timer = 0.4;
            }
            if !p.going_forward {
                yaw += 0x8000;
            }
            let mut rate = (speed * 0.005).clamp(0.1, 1.0) * dt;
            rate *= if p.leg_rotation_slow_timer > 0.0 { 0.4 } else { 1.0 };
            if ms == Move::Crouch {
                rate *= 0.5;
            }
            if p.physics == Physics::Falling {
                rate *= 0.25;
            }
            let f = (rate * 8.0).min(1.0);
            let d = norm_axis(yaw - p.leg_rotation);
            p.leg_rotation = norm_axis(p.leg_rotation + (d as f32 * f) as i32);
        }
        let rel = norm_axis(p.leg_rotation - p.rotation.yaw).clamp(p.go_back_leg_angle_limit_min, p.go_back_leg_angle_limit_max);
        p.leg_rotation = norm_axis(p.rotation.yaw + rel);
    }

    /// Mesh translation decay at the top of ATdPawn::Tick (0x12BA2E0): OffsetMeshXY's X/Y
    /// return to zero at 100 uu/s (scaled down by speed while walking or crouching, so a
    /// camera-collision push holds while standing still), and Translation.Z steps to
    /// TargetMeshZ at ten times the gap per second, at least 1 uu/s.
    pub(crate) fn update_mesh_translation(&mut self, dt: f32) {
        let p = &mut self.pawn;
        let mut rate = dt * 100.0;
        if matches!(p.movement_state, Move::Walking | Move::Crouch) {
            rate *= (p.velocity.size_2d() * 0.01).min(1.0);
        }
        for v in [&mut p.mesh_offset_xy.x, &mut p.mesh_offset_xy.y] {
            if v.abs() > 0.0001 {
                *v -= v.signum() * v.abs().min(rate);
            }
        }
        let d = p.target_mesh_translation_z - p.mesh_translation_z;
        if d.abs() > 0.0001 {
            let step = (d.abs() * dt * 10.0).max(1.0).min(d.abs());
            p.mesh_translation_z += d.signum() * step;
        }
    }

    /// TdPawn.OffsetMeshXY (0x12BA050): adds to the 1p/3p meshes' actor-space translation,
    /// each axis clamped to 32. A world-space offset is turned into actor space first.
    pub fn offset_mesh_xy(&mut self, offset: Vec3, world_space: bool) {
        let p = &mut self.pawn;
        let local = if world_space {
            let (x, y, z) = p.rotation.axes();
            Vec3::new(offset.dot(x), offset.dot(y), offset.dot(z))
        } else {
            offset
        };
        p.mesh_offset_xy.x = (p.mesh_offset_xy.x + local.x).clamp(-32.0, 32.0);
        p.mesh_offset_xy.y = (p.mesh_offset_xy.y + local.y).clamp(-32.0, 32.0);
    }

    /// The mesh XY translation in world space (Mesh.Translation is relative to the actor).
    pub fn mesh_offset_xy_world(&self) -> Vec3 {
        let (x, y, _) = self.pawn.rotation.axes();
        x * self.pawn.mesh_offset_xy.x + y * self.pawn.mesh_offset_xy.y
    }

    /// TdPlayerPawn.Tick: the against-wall state is only kept up while the move allows it.
    pub(crate) fn tick_against_wall(&mut self) {
        let ms = self.pawn.movement_state;
        let mut allow = self.moves.base(ms).enable_against_wall && !self.pawn.uncontrolled_fall && !self.pawn.dying;
        if self.pawn.physics == Physics::Falling && self.pawn.velocity.size_2d() > 400.0 {
            allow = false;
        }
        if allow {
            self.update_against_wall();
        } else {
            self.pawn.against_wall_state = AgainstWall::None;
        }
    }

    /// ATdPlayerPawn::UpdateAgainstWall (0x12BD860).
    fn update_against_wall(&mut self) {
        let s = self.check_against_wall();
        if s != AgainstWall::None {
            self.pawn.against_wall_state = s;
            self.set_timer(TimerFn::StopAgainstWall, 0.15, false);
            self.pawn.constrain_look = true;
            self.pawn.min_look_constraint.pitch = -5000;
            self.pawn.max_look_constraint.pitch = 32768;
        } else if self.pawn.movement_state == Move::Walking {
            // Moves[Walking] gets its class-default look constraints back
            let d = crate::moves::MoveBase::new(Class::Walking, &self.cfg);
            let b = self.moves.base_mut(Move::Walking);
            b.min_look_constraint = d.min_look_constraint;
            b.max_look_constraint = d.max_look_constraint;
        }
    }

    /// TdPlayerPawn.StopAgainstWall (timer).
    pub(crate) fn stop_against_wall(&mut self) {
        if self.check_against_wall() == AgainstWall::None {
            self.pawn.against_wall_state = AgainstWall::None;
            // ReleaseCameraConstraintsAgainstWall
            let d = crate::pawn::Pawn::new(&self.cfg);
            self.pawn.min_look_constraint = d.min_look_constraint;
            self.pawn.max_look_constraint = d.max_look_constraint;
            self.pawn.constrain_look = false;
        } else {
            self.set_timer(TimerFn::StopAgainstWall, 0.15, false);
        }
    }

    /// ATdPlayerPawn::CheckAgainstWall (0x12B4F10): two short traces straight ahead from the
    /// shoulders (14 uu either side of the pawn, at 68 over the location or 26 crouched) that
    /// must meet a wall facing back at the pawn. Both: against the wall; one: that side.
    pub fn check_against_wall(&mut self) -> AgainstWall {
        if self.health <= 0 {
            return AgainstWall::None;
        }
        let p = &self.pawn;
        let dir = p.rotation.vector();
        let right = Vec3::new(-dir.y, dir.x, 0.0);
        let along = (p.velocity.dot(dir) * 0.7 * 0.4).max(40.0);
        let up = if p.movement_state == Move::Crouch { 26.0 } else { 68.0 };
        let extent = Vec3::new(2.0, 2.0, 5.0);
        let view_down = self.pc.rotation.vector().z.min(0.0);
        let start_r = p.location + Vec3::new(0.0, 0.0, up) + right * 14.0;
        let mut right_hit = false;
        let mut left_hit = false;
        let h = self.world.line_check(start_r + dir * along, start_r, extent);
        let mut normal = Vec3::ZERO;
        if h.hit && h.normal.dot(dir) < -0.5 {
            right_hit = true;
            self.pawn.against_wall_right_hand = h.location - right * (view_down * 15.0);
            normal = h.normal;
        }
        let start_l = start_r - right * 28.0;
        let h = self.world.line_check(start_l + dir * along, start_l, extent);
        if h.hit && h.normal.dot(dir) < -0.5 {
            left_hit = true;
            self.pawn.against_wall_left_hand = h.location + right * (view_down * 15.0);
            normal = h.normal;
        }
        if !left_hit && !right_hit {
            return AgainstWall::None;
        }
        self.pawn.against_wall_normal = (normal + Vec3::new(0.0, 0.0, 0.1)).safe_normal();
        match (left_hit, right_hit) {
            (true, true) => AgainstWall::AgainstWall,
            (true, false) => AgainstWall::Left,
            _ => AgainstWall::Right,
        }
    }

    /// TdPlayerPawn.CalcCamera -> Moves[MovementState].CheckForCameraCollision, for moves with
    /// bUseCameraCollision: a short trace ahead of the eye pushes the mesh (and so the eye)
    /// back out of whatever it would see through. `camera` is the eye after the swan neck.
    pub fn check_for_camera_collision(&mut self, camera: Vec3, camera_rotation: Rotator) {
        let m = self.pawn.movement_state;
        if !self.moves.base(m).use_camera_collision {
            return;
        }
        let ext = Vec3::new(2.0, 2.0, 2.0);
        let pawn_dir = self.pawn.rotation.vector();
        let trace = |s: &Sim, end: Vec3, start: Vec3| {
            let h = s.world.line_check(end, start, ext);
            h.hit.then_some(h.location)
        };
        // TdMove.CheckForCameraCollision
        let base = |s: &mut Sim| {
            let start = camera - pawn_dir * 5.0;
            let end = camera + pawn_dir * 11.0;
            if let Some(hit) = trace(s, end, start) {
                s.offset_mesh_xy(Vec3::new(-(end - hit).size_2d(), 0.0, 0.0), false);
            }
        };
        match class_of(m) {
            Class::Walking => {
                base(self);
                if self.pawn.against_wall_state == AgainstWall::None {
                    let view = self.pc.rotation.vector();
                    let start = camera - view * 5.0;
                    let end = camera + view * 15.0;
                    let h = self.world.line_check(end, start, ext);
                    let b = self.moves.base_mut(m);
                    if h.hit {
                        let hit_time = (h.location - start).length() / (end - start).length();
                        let pitch = camera_rotation.normalize().pitch;
                        b.min_look_constraint.pitch = if b.max_look_constraint.pitch < 14000 || hit_time < 0.8 {
                            (pitch as f32 * hit_time) as i32
                        } else {
                            pitch
                        };
                        b.max_look_constraint.pitch = 32768;
                        b.constrain_look = true;
                    } else {
                        b.constrain_look = false;
                    }
                }
            }
            // TdMove_Crouch / TdMove_Slide: for the first 0.2 s a longer, slightly lowered trace
            Class::Crouch | Class::Slide => {
                if self.moves.base(m).move_active_time < 0.2 {
                    let start = camera - pawn_dir * 5.0;
                    let mut end = camera + pawn_dir * 15.0;
                    end.z -= 5.0;
                    if let Some(hit) = trace(self, end, start) {
                        self.offset_mesh_xy(Vec3::new(-(end - hit).size_2d() - 1.0, 0.0, 0.0), false);
                    }
                } else if class_of(m) == Class::Crouch {
                    base(self);
                }
            }
            Class::GrabPullUp => {
                let view = self.pc.rotation.vector();
                let start = camera - view * 5.0;
                let end = camera + view * 20.0;
                if let Some(hit) = trace(self, end, start) {
                    self.offset_mesh_xy(hit - end, true);
                }
            }
            Class::SpeedVault | Class::VaultOver => {
                let d = self.pawn.move_normal.cross(Vec3::new(0.0, 0.0, -1.0));
                let start = camera - d * 5.0;
                let end = camera + d * 15.0;
                if let Some(hit) = trace(self, end, start) {
                    self.offset_mesh_xy(hit - end, true);
                }
            }
            _ => base(self),
        }
    }

    /// The TdAnimNodeTurn the anim tree is showing, if any: TdAnimNodeTurn_0 under the
    /// standing idle (MovementState Walking, WalkingState idle) or _18 under the crouched one.
    pub fn turn_node_set(&self) -> Option<&'static [&'static str; 5]> {
        let p = &self.pawn;
        let s = if p.animation_movement_state != Move::None { p.animation_movement_state } else { p.movement_state };
        let idle = matches!(p.current_walking_state, WalkingState::Idle | WalkingState::None);
        match s {
            Move::Walking if idle && p.physics != Physics::Falling => Some(&STAND_TURNS),
            Move::Crouch if idle => Some(&CROUCH_TURNS),
            _ => None,
        }
    }

    /// TdAnimNodeTurn TickAnim for whichever turn node is relevant (only the player's path:
    /// the 25-65 degree idle realign is for TdAIControllers).
    pub(crate) fn tick_turn_node(&mut self, dt: f32) {
        let set = self.turn_node_set();
        let n = &mut self.turn_node;
        match set {
            None => {
                if n.relevant {
                    // OnCeaseRelevant
                    n.relevant = false;
                    n.playing_turn_animation = false;
                    n.active_child = 0;
                }
                return;
            }
            Some(_) if !n.relevant => {
                // OnBecomeRelevant
                *n = TurnNode { relevant: true, ..Default::default() };
            }
            _ => {}
        }
        let set = set.unwrap();
        if self.turn_node.playing_turn_animation {
            let lt = self.turn_node.leg_turn_per_second;
            self.pawn.leg_rotation = norm_axis(self.pawn.leg_rotation + (lt * dt) as i32);
            let len = self.anim.length(set[self.turn_node.active_child]);
            self.turn_node.position += dt;
            if self.turn_node.position >= len {
                // OnAnimEnd of the active child
                self.turn_node.playing_turn_animation = false;
                self.turn_node.active_child = 0;
                self.turn_node.position = 0.0;
            }
            self.turn_node.time_standing_still = 0.0;
            return;
        }
        let angle = norm_axis(self.pawn.rotation.yaw - self.pawn.leg_rotation) as f32 * (360.0 / 65536.0);
        if angle.abs() < TurnNode::SAFE_REGION_LIMIT {
            self.turn_node.time_standing_still = 0.0;
        } else if angle.abs() > TurnNode::EXTENDED_REGION_LIMIT {
            self.start_turn(angle, set);
        }
        let _ = TurnNode::IDLE_TIMER;
    }

    /// TdAnimNodeTurn StartTurn (0x12133D0).
    fn start_turn(&mut self, angle: f32, set: &[&str; 5]) {
        let big = angle.abs() > TurnNode::EXTENDED_REGION_LIMIT;
        let child = match (big, angle >= 0.0) {
            (false, false) => 1,
            (true, false) => 2,
            (false, true) => 3,
            (true, true) => 4,
        };
        let n = &mut self.turn_node;
        n.playing_turn_animation = true;
        n.active_child = child;
        n.position = 0.0;
        let len = self.anim.length(set[child]).max(1e-3);
        n.leg_turn_per_second = angle.signum() / len * if big { 16384.0 } else { 8192.0 };
    }
}
