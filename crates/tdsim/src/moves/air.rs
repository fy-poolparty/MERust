//! Jump-family moves: TdMove_WallrunJump, TdMove_WallrunDodgeJump, TdMove_WallClimbDodgeJump,
//! TdMove_WallClimb180TurnJump, TdMove_DodgeJump, TdMove_180Turn, TdMove_180TurnInAir
//! (vt70 0x120C120), TdMove_Coil (vt70 0x120BF70), TdMove_SoftLanding, TdMove_SkillRoll,
//! TdMove_FallingUncontrolled. TdMove_WallKick's CanDoMove is `return false`, so it is not here.

use crate::config::Config;
use crate::math::{Rotator, UeVec, Vec3, norm_axis};
use crate::pawn::{Move, MoveAction, MoveActionHint, Slot, WalkingState};
use crate::sim::Sim;

pub struct WallrunJump {
    pub push_away_speed_noob: f32,
    pub push_away_speed_pro_add: f32,
    pub push_forward_speed_min: f32,
    pub jump_off_z_height_forward: f32,
    pub jump_off_z_height_max_add_turned: f32,
    pub jump_off_z_speed: f32,
    pub min_constraint_world: i32,
    pub max_constraint_world: i32,
}

/// TdMove_WallrunDodgeJump / TdMove_WallClimbDodgeJump / TdMove_DodgeJump config.
pub struct DodgeParams {
    pub base_jump_z: f32,
    pub jump_add_xy: f32,
    pub inertia_conservation: f32,
    pub blend_in: f32,
    pub blend_out: f32,
}

pub struct WallClimb180TurnJump {
    pub jump_off_z_height: f32,
    pub jump_push_away_speed: f32,
    pub jump_time_window: f32,
    pub jumping_from_wall: bool,
    pub wanted_jump_dir: Vec3,
}

pub struct Coil {
    pub height_boost_duration: f32,
    pub total_height_boost: f32,
    pub min_trigger_speed: f32,
    pub coil_time: f32,
    pub height_boost_left: f32,
}

pub struct AirMoves {
    pub wallrun_jump: WallrunJump,
    pub wallrun_dodge: DodgeParams,
    pub wallclimb_dodge: DodgeParams,
    pub dodge: DodgeParams,
    pub wallclimb_180: WallClimb180TurnJump,
    pub turn180_blend_in: f32,
    pub turn180_blend_out: f32,
    pub coil: Coil,
    pub soft_landing_backwards: bool,
    pub lay_getting_up: bool,
    pub lay_doing_back_roll: bool,
}

impl AirMoves {
    pub fn new(cfg: &Config) -> Self {
        let dodge = |sec: &str, z: f32, xy: f32, inertia: f32, bi: f32, bo: f32| {
            let c = &[sec];
            DodgeParams {
                base_jump_z: cfg.f32(c, "BaseJumpZ", z),
                jump_add_xy: cfg.f32(c, "JumpAddXY", xy),
                inertia_conservation: cfg.f32(c, "DodgeJumpInertiaConservation", inertia),
                blend_in: cfg.f32(c, "JumpBlendInTime", bi),
                blend_out: cfg.f32(c, "JumpBlendOutTime", bo),
            }
        };
        let wj = &["TdMove_WallrunJump"];
        let w180 = &["TdMove_WallClimb180TurnJump"];
        let t180 = &["TdMove_180Turn"];
        let co = &["TdMove_Coil"];
        AirMoves {
            wallrun_jump: WallrunJump {
                push_away_speed_noob: cfg.f32(wj, "WallRunningPushAwaySpeedNoob", 120.0),
                push_away_speed_pro_add: cfg.f32(wj, "WallRunningPushAwaySpeedProAdd", 400.0),
                push_forward_speed_min: cfg.f32(wj, "WallRunningPushForwardSpeedMin", 0.1),
                jump_off_z_height_forward: cfg.f32(wj, "WallRunningJumpOffZHeightForward", 100.0),
                jump_off_z_height_max_add_turned: cfg.f32(wj, "WallRunningJumpOffZHeightMaxAddTurned", 60.0),
                jump_off_z_speed: 0.0,
                min_constraint_world: 0,
                max_constraint_world: 0,
            },
            wallrun_dodge: dodge("TdMove_WallrunDodgeJump", 300.0, 600.0, 0.3, 0.0, 0.0),
            wallclimb_dodge: dodge("TdMove_WallClimbDodgeJump", 700.0, 150.0, 1.0, 0.2, 0.2),
            dodge: dodge("TdMove_DodgeJump", 300.0, 600.0, 0.3, 0.1, 0.2),
            wallclimb_180: WallClimb180TurnJump {
                jump_off_z_height: cfg.f32(w180, "JumpOffZHeight", 250.0),
                jump_push_away_speed: cfg.f32(w180, "JumpPushAwaySpeed", 400.0),
                jump_time_window: cfg.f32(w180, "JumpTimeWindow", 0.6),
                jumping_from_wall: false,
                wanted_jump_dir: Vec3::ZERO,
            },
            turn180_blend_in: cfg.f32(t180, "TurnAnimBlendInTime", 0.2),
            turn180_blend_out: cfg.f32(t180, "TurnAnimBlendOutTime", 0.2),
            coil: Coil {
                height_boost_duration: cfg.f32(co, "HeightBoostDuration", 0.25),
                total_height_boost: cfg.f32(co, "TotalHeightBoost", 60.0),
                min_trigger_speed: cfg.f32(co, "CoilMinTriggerSpeed", 100.0),
                coil_time: cfg.f32(co, "CoilTime", 0.5),
                height_boost_left: 0.0,
            },
            soft_landing_backwards: false,
            lay_getting_up: false,
            lay_doing_back_roll: false,
        }
    }
}

/// Normal(vect(0,0,1) cross Vector(Rotation)): the pawn's right.
fn right_of(rot: Rotator) -> Vec3 {
    Vec3::new(0.0, 0.0, 1.0).cross(rot.vector()).safe_normal()
}

impl Sim {
    /// Jump height to vertical speed against the gravity, clipped by a ceiling trace (shared by
    /// the wall jumps): Speed = |g| * 2 * sqrt(h / |g|).
    fn jump_speed_for_height(&self, start: Vec3, mut height: f32) -> f32 {
        let mut end = start;
        end.z += height;
        if let Some(h) = self.movement_trace(end, start, self.pawn.extent()) {
            height = h.location.z - self.pawn.location.z;
        }
        let g = self.pawn.gravity_z().abs();
        g * 2.0 * (height / g).sqrt()
    }

    // ---------------------------------------------------------------- WallrunJump

    /// TdMove_WallrunJump.StartMove.
    pub fn wallrun_jump_start_move(&mut self, m: Move) {
        let saved_floor = self.pawn.floor;
        let wall_normal = Rotator::from_vector(saved_floor);
        self.pawn.illegal_ledge_timer = 2.0;
        self.pawn.illegal_ledge_normal = saved_floor;
        let (lo, hi) = if self.pawn.old_movement_state == Move::WallRunningLeft { (16000, 10000) } else { (10000, 16000) };
        self.moves.air.wallrun_jump.min_constraint_world = norm_axis(wall_normal.yaw - lo);
        self.moves.air.wallrun_jump.max_constraint_world = norm_axis(wall_normal.yaw + hi);
        self.physics_move_start_move(m);
        self.pawn.last_jump_location = self.pawn.location;
        let v = self.pawn.velocity;
        let mut wall_proj = v - saved_floor * v.dot(saved_floor);
        wall_proj.z = 0.0;
        let mut wall_dir = saved_floor;
        wall_dir.z = 0.0;
        let wall_dir = wall_dir.safe_normal();
        let mut cam = if self.moves.wallrun.turned_90_from_wall {
            self.pawn.face_rotation_time_left = 0.1;
            self.pawn.leg_rotation = wall_normal.yaw;
            self.set_look_at_target_angle(m, wall_normal, 0.1, -1.0);
            self.set_precise_rotation(m, wall_normal, 0.1);
            self.moves.base_mut(m).disable_controller_facing_pawn_yaw_rotation = true;
            saved_floor
        } else {
            self.pc.rotation.vector()
        };
        cam.z = 0.0;
        let cam = cam.safe_normal();
        // the script's FMax(0, PushSpeed) discards its result
        let push = wall_dir.dot(cam);
        let wj = &self.moves.air.wallrun_jump;
        let height = wj.jump_off_z_height_forward + push * wj.jump_off_z_height_max_add_turned;
        let loc = self.pawn.location;
        let mut speed = self.jump_speed_for_height(loc, height);
        speed *= 1.0 / (1 + self.moves.wallrun.consequtive_wallruns) as f32;
        let wj = &mut self.moves.air.wallrun_jump;
        wj.jump_off_z_speed = speed.max(10.0);
        let away = wj.push_away_speed_noob + wj.push_away_speed_pro_add * push;
        let fwd = wj.push_forward_speed_min + (1.0 - wj.push_forward_speed_min) * (1.0 - push);
        self.pawn.velocity.z = speed;
        self.pawn.velocity.x = saved_floor.x * away;
        self.pawn.velocity.y = saved_floor.y * away;
        self.pawn.velocity += wall_proj * fwd;
        self.anim.stop(Slot::FullBody, 0.2);
        let anim = if push > 0.6 {
            if self.pawn.old_movement_state == Move::WallRunningLeft { "WallrunJumpLeft" } else { "WallrunJumpRight" }
        } else {
            "JumpSlow"
        };
        self.play_move_anim(m, Slot::FullBodyDir, anim, 1.0, 0.2, 0.2, false, false);
    }

    /// TdMove_WallrunJump.StopMove.
    pub fn wallrun_jump_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        if self.pawn.pending_movement_state != Move::Falling && self.pawn.pending_movement_state != Move::VaultOver {
            self.anim.stop(Slot::FullBodyDir, 0.2);
        }
    }

    // ---------------------------------------------------------------- dodge jumps

    /// TdMove_WallrunDodgeJump.CanDoMove.
    pub fn wallrun_dodge_can_do_move(&mut self, m: Move) -> bool {
        if !self.tdmove_can_do_move(m) {
            return false;
        }
        let (ms, h) = (self.pawn.movement_state, self.pawn.move_action_hint);
        (ms == Move::WallRunningLeft && h == MoveActionHint::Right) || (ms == Move::WallRunningRight && h == MoveActionHint::Left)
    }

    /// TdMove_WallrunDodgeJump.StartMove.
    pub fn wallrun_dodge_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        self.pawn.last_jump_location = self.pawn.location;
        let p = &self.moves.air.wallrun_dodge;
        let left = self.pawn.move_action_hint == MoveActionHint::Left;
        let dir = right_of(self.pawn.rotation) * ((if left { -1.0 } else { 1.0 }) * p.jump_add_xy);
        self.pawn.velocity = dir + self.pawn.velocity * p.inertia_conservation;
        self.pawn.velocity.z = p.base_jump_z;
        self.play_move_anim(m, Slot::FullBody, if left { "dodgejumpleft" } else { "dodgejumpright" }, 1.0, 0.2, 0.2, false, false);
    }

    /// TdMove_WallClimbDodgeJump.CanDoMove.
    pub fn wallclimb_dodge_can_do_move(&mut self, m: Move) -> bool {
        if !self.tdmove_can_do_move(m) {
            return false;
        }
        self.pawn.movement_state == Move::WallClimbing && matches!(self.pawn.move_action_hint, MoveActionHint::Right | MoveActionHint::Left)
    }

    /// TdMove_WallClimbDodgeJump.StartMove.
    pub fn wallclimb_dodge_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        let p = &self.moves.air.wallclimb_dodge;
        let left = self.pawn.move_action_hint == MoveActionHint::Left;
        let dir = right_of(self.pawn.rotation) * (if left { -p.jump_add_xy } else { p.jump_add_xy });
        let (bi, bo, inertia, z) = (p.blend_in, p.blend_out, p.inertia_conservation, p.base_jump_z);
        self.play_move_anim(m, Slot::FullBody, "JumpSlow", 1.0, bi, bo, false, false);
        self.set_look_at_target_angle(m, Rotator::from_vector(dir), 0.1, 1.0);
        self.pawn.last_jump_location = self.pawn.location;
        self.pawn.velocity = dir + self.pawn.velocity * inertia;
        self.pawn.velocity.z = z;
        let b = self.moves.base_mut(m);
        b.check_for_grab = false;
        b.check_for_vault_over = false;
        b.check_for_wall_climb = false;
        self.set_move_countdown(m, 0.5);
    }

    /// TdMove_WallClimbDodgeJump.OnTimer / TdMove_WallClimb180TurnJump: back to the defaults.
    fn restore_ledge_checks(&mut self, m: Move) {
        let d = super::MoveBase::new(super::class_of(m), &self.cfg);
        let b = self.moves.base_mut(m);
        b.check_for_grab = d.check_for_grab;
        b.check_for_vault_over = d.check_for_vault_over;
        b.check_for_wall_climb = d.check_for_wall_climb;
    }

    pub fn wallclimb_dodge_on_timer(&mut self, m: Move) {
        self.restore_ledge_checks(m);
    }

    /// TdMove_DodgeJump.CanDoMove.
    pub fn dodge_can_do_move(&mut self, m: Move) -> bool {
        let p = &self.pawn;
        if !p.move_action_max || p.uncontrolled_slide {
            return false;
        }
        if !matches!(p.move_action_hint, MoveActionHint::Left | MoveActionHint::Right) {
            return false;
        }
        self.tdmove_can_do_move(m)
    }

    /// TdMove_DodgeJump.StartMove (aim assist look-at omitted: no AI).
    pub fn dodge_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        let p = &self.moves.air.dodge;
        let left = self.pawn.move_action_hint == MoveActionHint::Left;
        let dir = right_of(self.pawn.rotation) * (if left { -p.jump_add_xy } else { p.jump_add_xy });
        let (bi, bo, inertia, z) = (p.blend_in, p.blend_out, p.inertia_conservation, p.base_jump_z);
        self.play_move_anim(m, Slot::FullBody, if left { "dodgejumpleft" } else { "dodgejumpright" }, 1.0, bi, bo, false, false);
        self.pawn.last_jump_location = self.pawn.location;
        self.pawn.velocity = dir + self.pawn.velocity * inertia;
        self.pawn.velocity.z = z;
        self.pawn.acceleration = self.pawn.velocity.safe_normal();
    }

    // ---------------------------------------------------------------- WallClimb180TurnJump

    pub fn wallclimb_180_can_do_move(&mut self, m: Move) -> bool {
        self.tdmove_can_do_move(m) && self.pawn.movement_state == Move::WallClimbing
    }

    /// TdMove_WallClimb180TurnJump.StartMove.
    pub fn wallclimb_180_start_move(&mut self, m: Move) {
        let mut d = self.pawn.floor;
        d.z = 0.0;
        self.moves.air.wallclimb_180.wanted_jump_dir = d.safe_normal();
        self.physics_move_start_move(m);
        self.moves.air.wallclimb_180.jumping_from_wall = false;
        self.play_move_anim(m, Slot::FullBody, "wallrunvertical180turn", 1.0, 0.2, 0.1, false, true);
        self.use_root_rotation(true);
        self.reset_camera_look(m, 0.2);
        let b = self.moves.base_mut(m);
        b.check_for_grab = false;
        b.check_for_vault_over = false;
        b.check_for_wall_climb = false;
        let w = self.moves.air.wallclimb_180.jump_time_window;
        self.set_move_countdown(m, w);
    }

    pub fn wallclimb_180_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.use_root_rotation(false);
    }

    /// TdMove_WallClimb180TurnJump.HandleMoveAction.
    pub fn wallclimb_180_handle_move_action(&mut self, m: Move, a: MoveAction) {
        let w = &self.moves.air.wallclimb_180;
        if a == MoveAction::Jump && !w.jumping_from_wall && self.moves.base(m).move_active_time < w.jump_time_window {
            self.wallclimb_180_jump_from_wall(m);
        }
    }

    /// TdMove_WallClimb180TurnJump.JumpFromWall.
    fn wallclimb_180_jump_from_wall(&mut self, m: Move) {
        self.moves.air.wallclimb_180.jumping_from_wall = true;
        self.reset_camera_look(m, 0.2);
        self.pawn.last_jump_location = self.pawn.location;
        let w = &self.moves.air.wallclimb_180;
        let (dir, height, push) = (w.wanted_jump_dir, w.jump_off_z_height, w.jump_push_away_speed);
        let start = self.pawn.location + dir * 2.0;
        let speed = self.jump_speed_for_height(start, height);
        self.pawn.velocity = dir * push;
        self.pawn.velocity.z = speed;
    }

    /// TdMove_WallClimb180TurnJump.OnTimer.
    pub fn wallclimb_180_on_timer(&mut self, m: Move) {
        if self.moves.air.wallclimb_180.jumping_from_wall {
            self.play_move_anim(m, Slot::FullBody, "WallrunJumpLeft", 1.0, 0.1, 0.2, false, false);
        }
        self.use_root_rotation(false);
        let r = Rotator::new(self.pc.rotation.pitch, Rotator::from_vector(self.moves.air.wallclimb_180.wanted_jump_dir).yaw, 0);
        self.set_look_at_target_angle(m, r, 0.2, -1.0);
        self.set_precise_rotation(m, r, 0.1);
    }

    /// TdMove_WallClimb180TurnJump.ReachedPreciseRotation.
    pub fn wallclimb_180_reached_precise_rotation(&mut self, m: Move) {
        if self.moves.air.wallclimb_180.jumping_from_wall {
            self.restore_ledge_checks(m);
        }
        self.stop_ignore_move_input();
        self.set_move(Move::Falling, false, false);
    }

    // ---------------------------------------------------------------- 180Turn

    /// TdMove_180Turn.CanDoMove.
    pub fn turn180_can_do_move(&mut self, m: Move) -> bool {
        self.pawn.movement_state == Move::Walking && self.tdmove_can_do_move(m)
    }

    /// TdMove_180Turn.StartMove (unarmed).
    pub fn turn180_start_move(&mut self, m: Move) {
        self.tdmove_start_move(m);
        let (bi, bo) = (self.moves.air.turn180_blend_in, self.moves.air.turn180_blend_out);
        let anim = if self.pawn.current_walking_state != WalkingState::Idle { "RunTurn180" } else { "StandTurn180Right" };
        self.play_move_anim(m, Slot::FullBody, anim, 1.0, bi, bo, false, true);
        self.use_root_rotation(true);
        let t = self.moves.base(m).disable_movement_time;
        self.set_move_countdown(m, t);
    }

    /// TdMove_180Turn.StopMove.
    pub fn turn180_stop_move(&mut self, m: Move) {
        self.anim.stop(Slot::FullBody, 0.2);
        self.use_root_rotation(false);
        self.tdmove_stop_move(m);
    }

    /// TdMove_180Turn.OnTimer.
    pub fn turn180_on_timer(&mut self, m: Move) {
        self.use_root_rotation(false);
        self.moves.base_mut(m).disable_face_rotation = false;
        self.pawn.face_rotation_time_left = 0.25;
        self.set_move_countdown(m, 0.2);
    }

    // ---------------------------------------------------------------- 180TurnInAir

    /// TdMove_180TurnInAir.CanDoMove.
    pub fn turn180_air_can_do_move(&mut self, m: Move) -> bool {
        if self.pawn.movement_state == Move::SoftLanding {
            return false;
        }
        if self.pawn.rotation.vector().safe_normal().dot(self.pawn.velocity.safe_normal()) < 0.2 {
            return false;
        }
        self.tdmove_can_do_move(m)
    }

    /// TdMove_180TurnInAir.StartMove.
    pub fn turn180_air_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        self.play_move_anim(m, Slot::FullBody, "JumpTurnFly", 1.0, 0.1, 0.1, false, true);
        self.use_root_rotation(true);
        let mut look = self.pawn.last_jump_location;
        look.z += 90.0;
        self.set_look_at_target_location(m, look, 0.3, 2.0);
    }

    pub fn turn180_air_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.use_root_rotation(false);
        self.anim.stop(Slot::Weapon, 0.2);
    }

    /// UTdMove_180TurnInAir vt70 tail: stuck in the air (no velocity) for a while -> Landing.
    pub fn turn180_air_tick(&mut self, m: Move) {
        if self.moves.base(m).move_active_time > 0.3 && self.pawn.velocity_magnitude < 0.1 {
            self.set_move(Move::Landing, false, false);
        }
    }

    // ---------------------------------------------------------------- Coil

    /// TdMove_Coil.CanDoMove.
    pub fn coil_can_do_move(&mut self, m: Move) -> bool {
        let p = &self.pawn;
        if p.is_using_root_rotation || p.movement_state == Move::Falling {
            return false;
        }
        if p.movement_state == Move::IntoGrab && p.velocity.z < 0.0 {
            return false;
        }
        if p.physics != crate::pawn::Physics::Falling {
            return false;
        }
        if p.rotation.vector().dot(p.velocity) < self.moves.air.coil.min_trigger_speed {
            return false;
        }
        self.tdmove_can_do_move(m)
    }

    pub fn coil_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        self.moves.air.coil.height_boost_left = self.moves.air.coil.total_height_boost;
        self.set_animation_movement_state(Move::Crouch, -1.0);
        self.play_move_anim(m, Slot::FullBody, "JumpCoil", 1.0, 0.15, 0.15, false, false);
    }

    pub fn coil_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.play_move_anim(m, Slot::FullBody, "JumpCoilEnd", 1.0, 0.25, 0.35, false, false);
        self.set_animation_movement_state(Move::None, -1.0);
    }

    /// UTdMove_Coil vt70 tail: lift the pawn by TotalHeightBoost over HeightBoostDuration.
    pub fn coil_tick(&mut self, dt: f32) {
        let c = &mut self.moves.air.coil;
        if c.height_boost_left > 0.0 {
            let step = c.total_height_boost / c.height_boost_duration * dt;
            c.height_boost_left -= step;
            self.move_actor(Vec3::new(0.0, 0.0, step));
        }
    }

    // ---------------------------------------------------------------- SoftLanding / SkillRoll / FallingUncontrolled

    /// TdMove_SoftLanding.StartMove.
    pub fn soft_landing_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        if self.pawn.old_movement_state == Move::Turn180InAir {
            self.set_animation_movement_state(Move::Turn180InAir, -1.0);
            self.moves.air.soft_landing_backwards = true;
        } else {
            // PlayCustomAnim, not PlayMoveAnim: the move gets no OnCustomAnimEnd
            self.anim.play(Slot::FullBody, "fallinglandintosoftlanding", 1.0, 0.6, 0.2, true, false, false);
            self.moves.air.soft_landing_backwards = false;
        }
    }

    pub fn soft_landing_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.anim.stop(Slot::FullBody, 0.2);
    }

    /// TdMove_SkillRoll.CanDoMove.
    pub fn skill_roll_can_do_move(&mut self, m: Move) -> bool {
        self.tdmove_can_do_move(m) && self.pawn.movement_state == Move::Landing
    }

    /// TdMove_SkillRoll.StartMove.
    pub fn skill_roll_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        self.use_root_motion(true);
        self.play_move_anim(m, Slot::FullBody, "fallinglandroll", 1.0, 0.2, 0.2, true, false);
        self.set_ignore_move_input(-1.0);
        self.set_ignore_look_input(-1.0);
        self.reset_camera_look(m, 0.2);
        self.pawn.velocity = Vec3::ZERO;
        self.pawn.acceleration = Vec3::ZERO;
    }

    pub fn skill_roll_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.use_root_motion(false);
    }

    // ---------------------------------------------------------------- LayOnGround

    /// TdMove_LayOnGround.StartMove.
    pub fn lay_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        self.moves.air.lay_getting_up = false;
        self.moves.air.lay_doing_back_roll = false;
        if self.pawn.move_action_hint == MoveActionHint::Down {
            self.lay_get_up_back(m);
        }
    }

    pub fn lay_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.use_root_motion(false);
        self.moves.air.lay_getting_up = false;
        self.moves.air.lay_doing_back_roll = false;
    }

    /// TdMove_LayOnGround.HandleMoveAction.
    pub fn lay_handle_move_action(&mut self, m: Move, a: MoveAction) {
        if a == MoveAction::Jump || self.pawn.move_action_hint == MoveActionHint::Up {
            let up = self.pawn.location + Vec3::new(0.0, 0.0, 30.0);
            if self.can_stand(up, false) {
                self.lay_get_up(m);
            } else {
                self.set_move(Move::Crouch, false, false);
            }
        } else if self.pawn.move_action_hint == MoveActionHint::Down {
            self.lay_get_up_back(m);
        }
    }

    /// TdMove_LayOnGround.GetUp.
    fn lay_get_up(&mut self, m: Move) {
        if !self.moves.air.lay_getting_up {
            self.play_move_anim(m, Slot::FullBody, "JumpTurnLandingStand", 1.0, 0.2, 0.2, false, false);
            self.set_animation_movement_state(Move::Walking, 0.2);
            self.moves.air.lay_getting_up = true;
            self.reset_camera_look(m, 1.0);
        }
    }

    /// TdMove_LayOnGround.GetUpBack: backwards roll onto the feet.
    fn lay_get_up_back(&mut self, m: Move) {
        if !self.moves.air.lay_getting_up {
            self.play_move_anim(m, Slot::FullBody, "EvadeRoll", 1.0, 0.2, 0.2, true, false);
            self.use_root_motion(true);
            self.moves.air.lay_getting_up = true;
            self.moves.air.lay_doing_back_roll = true;
            self.reset_camera_look(m, 1.0);
            let mut look = self.pawn.location + self.pawn.rotation.vector() * 1000.0;
            look.z -= 300.0;
            self.set_look_at_target_location(m, look, 0.4, -1.0);
        }
    }

    /// TdMove_LayOnGround.OnCustomAnimEnd.
    pub fn lay_on_custom_anim_end(&mut self) {
        if self.moves.air.lay_getting_up {
            self.moves.air.lay_getting_up = false;
            let up = self.pawn.location + Vec3::new(0.0, 0.0, 30.0);
            let next = if self.can_stand(up, false) { Move::Walking } else { Move::Crouch };
            self.set_move(next, false, false);
        }
    }
}
