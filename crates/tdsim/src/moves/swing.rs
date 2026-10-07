//! TdMove_Swing and TdMove_SwingJump (TdSwingVolume): catch a bar from a jump, swing on it
//! as a pendulum (UTdMove_Swing vt70, 0x120B800), shimmy along it, turn around on it and jump
//! off or across to the next bar.

use crate::config::Config;
use crate::math::{Rotator, UeVec, Vec3};
use crate::moves::PreciseMode;
use crate::pawn::{Move, MoveAction, MoveActionHint, Physics, Slot};
use crate::sim::Sim;
use crate::sound::{LoopSlot, LoopSound, SoundEvent};

use std::f32::consts::PI;

pub struct Swing {
    pub volume: Option<usize>,
    pub swing_velocity: f32,
    pub max_swing_velocity: f32,
    pub exit_velocity_modifier: f32,
    pub swing_angle: f32,
    pub swing_direction: Vec3,
    pub bar_direction: Vec3,
    pub swing_location: Vec3,
    pub swing_pendulum_length: f32,
    pub interpolating_into: bool,
    pub shimmying: bool,
    pub turning: bool,
    pub shimmy_velocity: f32,
    pub shimmy_time: f32,
    pub swing_angle_timing_offset: f32,
    pub swing_exit_gravity_modifier: f32,
    pub swing_exit_gravity_modifier_time: f32,
    pub anim_blend_time: f32,
}

pub struct SwingJump {
    pub target_volume: Option<usize>,
    pub gravity_modifier: f32,
    pub gravity_modifier_timer: f32,
    pub target_volume_offset: Vec3,
}

impl Swing {
    pub fn new(cfg: &Config) -> Self {
        let c = &["TdMove_Swing"];
        Swing {
            volume: None,
            swing_velocity: 0.0,
            max_swing_velocity: 4.25,
            exit_velocity_modifier: cfg.f32(c, "ExitVelocityModifier", 600.0),
            swing_angle: 0.0,
            swing_direction: Vec3::X,
            bar_direction: Vec3::Y,
            swing_location: Vec3::ZERO,
            swing_pendulum_length: cfg.f32(c, "SwingPendulumLength", 120.0),
            interpolating_into: false,
            shimmying: false,
            turning: false,
            shimmy_velocity: 0.0,
            shimmy_time: 0.0,
            swing_angle_timing_offset: cfg.f32(c, "SwingAngleTimingOffset", 1.0),
            swing_exit_gravity_modifier: cfg.f32(c, "SwingExitGravityModifier", 0.75),
            swing_exit_gravity_modifier_time: cfg.f32(c, "SwingExitGravityModifierTime", 0.7),
            anim_blend_time: 0.15,
        }
    }
}

impl SwingJump {
    pub fn new(cfg: &Config) -> Self {
        let c = &["TdMove_SwingJump"];
        SwingJump {
            target_volume: None,
            gravity_modifier: cfg.f32(c, "GravityModifier", 0.73),
            gravity_modifier_timer: cfg.f32(c, "GravityModifierTimer", 0.75),
            target_volume_offset: cfg.vec3(c, "TargetVolumeOffset", Vec3::new(-120.0, 0.0, -20.0)),
        }
    }
}

/// TdMove_Swing.SwingSound.
pub const SWING_SOUND: &str = "A_Character_Female_01.Swing.Swing";

/// SetMoveTimer id for TdMove_Swing.StopInterpolating.
const STOP_INTERPOLATING: u8 = 0;

impl Sim {
    /// TdSwingVolume.PawnUpdate.
    pub(crate) fn swing_volume_pawn_update(&mut self, i: usize) {
        if self.pawn.movement_state == Move::Swing {
            return;
        }
        self.moves.swing.volume = Some(i);
        if self.can_do_move(Move::Swing) {
            self.set_move(Move::Swing, false, false);
            self.pawn.active_movement_volume = None;
        } else {
            self.moves.swing.volume = None;
        }
    }

    /// TdMove_Swing.CanDoMove: coming at the grip from below, facing and moving towards it.
    pub fn swing_can_do_move(&mut self, m: Move) -> bool {
        let Some(vi) = self.moves.swing.volume else { return false };
        let grip_to_pawn = (self.pawn.location - self.swings[vi].vol.location).safe_normal();
        if grip_to_pawn.dot(self.pawn.rotation.vector()) >= -0.3 {
            return false;
        }
        if grip_to_pawn.dot(self.pawn.velocity.safe_normal()) >= 0.0 {
            return false;
        }
        if grip_to_pawn.dot(Vec3::new(0.0, 0.0, -1.0)) < -0.5 {
            return false;
        }
        self.tdmove_can_do_move(m)
    }

    /// TdMove_Swing.StartMove.
    pub fn swing_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        let bt = self.moves.swing.anim_blend_time;
        self.set_root_offset_space(Vec3::new(0.0, -50.0, -32.0), bt, crate::pawn::BoneControlSpace::Bone);
        let v = self.swings[self.moves.swing.volume.expect("swing without a volume")].clone();
        let (vx, vy, _) = v.vol.rotation.axes();
        // PointDistToLine: the closest point of the bar's line
        let p = self.pawn.location;
        let o = v.vol.location;
        self.moves.swing.swing_location = o + vy * (p - o).dot(vy);
        let to_grip = (o - p).safe_normal();
        let s = &mut self.moves.swing;
        if to_grip.dot(v.vol.rotation.vector()) >= 0.0 {
            s.bar_direction = vy;
            s.swing_direction = vx;
        } else {
            s.bar_direction = -vy;
            s.swing_direction = -vx;
        }
        let angle = self.swing_get_pawn_angle(p);
        // Clamp(int(SwingAngle), int(-1.2), int(-0.7)): integer truncation leaves -1 or 0
        self.moves.swing.swing_angle = (angle as i32).clamp(-1, 0) as f32;
        let s = &mut self.moves.swing;
        s.swing_velocity = s.max_swing_velocity * (self.pawn.velocity.size_2d() / 500.0).min(1.0);
        let dir = s.swing_direction;
        self.set_precise_rotation(m, Rotator::from_vector(dir), bt);
        self.moves.swing.interpolating_into = true;
        let a = self.moves.swing.swing_angle;
        self.swing_set_pawn_rotation(a);
        self.swing_enable_control(bt);
        self.reset_camera_look(m, bt);
        self.pawn.velocity = Vec3::ZERO;
        self.pawn.acceleration = Vec3::ZERO;
        self.anim.stop(Slot::FullBody, bt);
        self.anim.stop(Slot::FullBodyDir, bt);
        // InitSwingSound: FadeIn(0.2)
        self.sound(SoundEvent::LoopStart { slot: LoopSlot::Swing, sound: LoopSound::Cue(SWING_SOUND.into()), fade_in: 0.2 });
        let anim = if v.thick_grip { "SwingHardStartWide" } else { "SwingHardStart" };
        self.play_move_anim(m, Slot::FullBody, anim, 1.0, bt, 0.2, false, false);
        self.set_move_timer(m, bt, false, STOP_INTERPOLATING);
    }

    /// TdMove_Swing.StopMove.
    pub fn swing_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.swing_disable_control();
        self.sound(SoundEvent::LoopStop { slot: LoopSlot::Swing, fade_out: 0.7 });
        self.set_root_offset_space(Vec3::ZERO, 0.1, crate::pawn::BoneControlSpace::Bone);
        self.moves.swing.shimmying = false;
        self.moves.swing.turning = false;
    }

    /// EnableSwingControl / DisableSwingControl: SwingControl1p's strength.
    fn swing_enable_control(&mut self, time: f32) {
        self.pawn.swing_control.set_strength(1.0, time);
    }

    fn swing_disable_control(&mut self) {
        self.pawn.swing_control.set_strength(0.0, 0.25);
    }

    /// TdMove_Swing.SetPawnRotation (event): the swing sound's input and SwingControl's roll
    /// and translation (the mesh pivots round the hands, 94 above the root).
    pub fn swing_set_pawn_rotation(&mut self, rad: f32) {
        self.pawn.custom_sound_input = 500.0 * self.moves.swing.swing_velocity.abs();
        let un_angle = (2.0 * (rad / PI) * 16384.0) as i32;
        self.pawn.swing_control.roll = -un_angle;
        self.pawn.swing_control.translation = Vec3::new(rad.sin() * 94.0, 0.0, (1.0 - rad.cos()) * 94.0);
    }

    /// UTdMove_Swing::GetPawnLocation (vt75, 0x11F3330).
    fn swing_get_pawn_location(&self, angle: f32) -> Vec3 {
        let s = &self.moves.swing;
        let v = Vec3::new(0.0, 0.0, 1.0) * angle.cos() - s.swing_direction * angle.sin();
        s.swing_location - v * s.swing_pendulum_length
    }

    /// UTdMove_Swing::GetPawnAngle (vt76, 0x11F8FB0): the angle of the pawn round the bar in
    /// the swing plane (0 hanging straight down, positive forward).
    fn swing_get_pawn_angle(&self, p: Vec3) -> f32 {
        let s = &self.moves.swing;
        let (x, _y, z) = Rotator::from_vector(s.swing_direction).axes();
        let d = (p - s.swing_location).safe_normal();
        let (lx, lz) = (d.dot(x), d.dot(z));
        let a = if lz <= 0.0 { lx.clamp(-1.0, 1.0).asin() } else { PI - lx.clamp(-1.0, 1.0).asin() };
        if a > PI { a - 2.0 * PI } else { a }
    }

    /// UTdMove_Swing::CanShimmy (vt78): the grip stays inside the volume.
    fn swing_can_shimmy(&self, delta: f32) -> bool {
        let Some(vi) = self.moves.swing.volume else { return false };
        let s = &self.moves.swing;
        self.swings[vi].vol.encompasses(s.swing_location + s.bar_direction * delta)
    }

    /// UTdMove_Swing::UpdateShimmy (vt77): along the bar at ShimmyVelocity times the strafe
    /// anim's weight.
    fn swing_update_shimmy(&mut self, dt: f32) {
        if !self.moves.swing.shimmying {
            return;
        }
        let weight = self.anim.current(Slot::FullBody).map(|a| a.weight).unwrap_or(0.0);
        let delta = weight * self.moves.swing.shimmy_velocity * dt;
        if self.swing_can_shimmy(delta) {
            let s = &mut self.moves.swing;
            s.swing_location = s.swing_location + s.bar_direction * delta;
            s.shimmy_time -= dt;
            if s.shimmy_time < 0.0 {
                self.swing_abort_shimmy();
            }
        } else {
            self.swing_abort_shimmy();
        }
    }

    /// TdMove_Swing.AbortShimmy (event).
    fn swing_abort_shimmy(&mut self) {
        self.moves.swing.shimmying = false;
        self.anim.stop(Slot::FullBody, 0.25);
    }

    /// UTdMove_Swing::CheckForTargetVolume (vt79, 0x1206770): another bar ahead, within reach
    /// of the current swing speed.
    fn swing_check_for_target_volume(&self) -> Option<usize> {
        let vi = self.moves.swing.volume?;
        let s = &self.moves.swing;
        let reach = (500.0 / s.max_swing_velocity * s.swing_velocity).max(0.0) + 100.0;
        let o = self.swings[vi].vol.location;
        let (start, end) = (o + s.swing_direction * 100.0, o + s.swing_direction * reach);
        self.swings
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != vi)
            .filter_map(|(i, v)| v.vol.line_entry(start, end).map(|t| (i, t)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// UTdMove_Swing vt70 (0x120B800): the pendulum, then the pawn placed on it.
    pub fn swing_tick(&mut self, m: Move, dt: f32) {
        let hint = self.pawn.move_action_hint;
        let s = &mut self.moves.swing;
        if !s.interpolating_into {
            s.swing_velocity -= s.swing_angle.sin() * dt * 10.0;
            s.swing_velocity -= s.swing_velocity * dt * 0.8;
            if !(s.shimmying || s.turning) {
                if hint == MoveActionHint::Up && s.swing_velocity >= 0.0 {
                    s.swing_velocity += dt * 3.0;
                } else if hint == MoveActionHint::Down && s.swing_velocity <= 0.0 {
                    s.swing_velocity -= dt * 3.0;
                }
            }
        }
        if s.swing_angle > 0.0 {
            s.swing_velocity = s.swing_velocity.min(s.swing_angle.cos() * s.max_swing_velocity);
        } else if s.swing_angle < 0.0 {
            s.swing_velocity = s.swing_velocity.max(-(s.swing_angle.cos() * s.max_swing_velocity));
        }
        let old = s.swing_angle;
        if !s.interpolating_into {
            s.swing_angle = (s.swing_angle + s.swing_velocity * dt).clamp(-PI, PI);
        }
        if s.turning {
            let sign = |a: f32| if a == 0.0 { 0.0 } else { a.signum() };
            if sign(old) != sign(s.swing_angle) {
                self.swing_on_timer(m);
            }
        }
        self.swing_update_shimmy(dt);
        if self.pawn.movement_state != m {
            return;
        }
        let mut loc = self.swing_get_pawn_location(self.moves.swing.swing_angle);
        if self.moves.swing.interpolating_into {
            let d = loc - self.pawn.location;
            if d.length_squared() >= 4.0 {
                let k = self.moves.base(m).move_active_time / self.moves.swing.anim_blend_time;
                loc = self.pawn.location + d * k;
            } else {
                self.moves.swing.interpolating_into = false;
            }
        }
        if let Some(vi) = self.moves.swing.volume {
            if self.swings[vi].snap_to_center {
                let o = self.swings[vi].vol.location;
                let s = &mut self.moves.swing;
                if (s.swing_location - o).size_2d() > 1e-8 {
                    s.swing_location = s.swing_location - (s.swing_location - o) * (dt * 2.0);
                }
            }
        }
        // FarMoveActor
        self.pawn.location = loc;
    }

    /// UTdMove_Swing vt71 (0x11F8E20): SetPawnRotation every tick.
    pub fn swing_tick_move(&mut self) {
        let a = self.moves.swing.swing_angle;
        self.swing_set_pawn_rotation(a);
    }

    /// TdMove_Swing.HandleMoveAction.
    pub fn swing_handle_move_action(&mut self, m: Move, a: MoveAction) {
        let left = matches!(a, MoveAction::ShimmyLeft | MoveAction::ShimmyLeftLong);
        let right = matches!(a, MoveAction::ShimmyRight | MoveAction::ShimmyRightLong);
        let s = &self.moves.swing;
        if a == MoveAction::Jump {
            let jump_angle = (PI / 2.0).min(s.swing_angle - s.swing_angle_timing_offset);
            if jump_angle > -(PI / 4.0) {
                self.swing_jump_off(m, jump_angle);
            }
        } else if !s.interpolating_into && a == MoveAction::Crouch {
            self.swing_let_go(m);
        } else if !s.interpolating_into && !s.turning && !s.shimmying && a == MoveAction::Turn {
            self.moves.swing.turning = true;
        } else if a == MoveAction::Melee {
            // SetMove(MeleeAir): melee isn't ported
        } else if !s.turning && !s.shimmying && (left || right) && !self.moves.swing.volume.is_some_and(|v| self.swings[v].snap_to_center) {
            if s.swing_velocity.abs() < 1.0 && s.swing_angle.abs() < PI / 8.0 {
                let dir = if right { 1.0 } else { -1.0 };
                if self.swing_can_shimmy(dir) {
                    self.moves.swing.shimmying = true;
                    self.moves.swing.shimmy_velocity = 35.0 * dir;
                    self.play_move_anim(m, Slot::FullBody, "SwingStrafe", dir, 0.2, 0.2, false, false);
                    let len = self.anim.current(Slot::FullBody).map(|c| c.length / c.rate.abs().max(1e-3)).unwrap_or(0.0);
                    self.moves.swing.shimmy_time = len.abs();
                }
            }
        }
    }

    /// TdMove_Swing.OnMoveTimer('StopInterpolating').
    pub fn swing_on_move_timer(&mut self, _m: Move, id: u8) {
        if id == STOP_INTERPOLATING {
            self.moves.swing.interpolating_into = false;
        }
    }

    /// TdMove_Swing.OnTimer: the turn round, played when the swing passes the bottom.
    fn swing_on_timer(&mut self, m: Move) {
        self.use_root_rotation(true);
        self.play_move_anim(m, Slot::FullBody, "Swing180", 1.0, 0.2, 0.3, false, true);
        self.pawn.rotation = Rotator::from_vector(self.moves.swing.swing_direction);
        self.moves.swing.swing_velocity = 0.0;
        self.moves.swing.swing_angle = 0.0;
        self.swing_set_pawn_rotation(0.0);
    }

    /// TdMove_Swing.OnCeaseRelevantRootMotion: the turn's done, face the other way.
    pub fn swing_on_cease_relevant_root_motion(&mut self, _m: Move) {
        if self.moves.swing.turning {
            self.moves.swing.turning = false;
            self.use_root_rotation(false);
            let s = &mut self.moves.swing;
            s.swing_direction = -s.swing_direction;
            s.bar_direction = -s.bar_direction;
            let r = Rotator::from_vector(s.swing_direction);
            self.pawn.rotation = r;
            self.pc.rotation = r;
            self.moves.swing.swing_velocity = 0.25;
        }
    }

    /// TdMove_Swing.JumpOff.
    fn swing_jump_off(&mut self, m: Move, jump_angle: f32) {
        let target = self.swing_check_for_target_volume();
        self.moves.swing_jump.target_volume = target;
        if target.is_none() {
            let s = &self.moves.swing;
            let dir = s.bar_direction.cross((s.swing_location - self.pawn.location).safe_normal());
            self.pawn.velocity = dir * jump_angle.cos() * s.exit_velocity_modifier;
            self.pawn.acceleration = self.pawn.velocity;
            self.pawn.gravity_modifier = s.swing_exit_gravity_modifier;
            self.pawn.gravity_modifier_timer = s.swing_exit_gravity_modifier_time;
            self.play_move_anim(m, Slot::FullBody, "SwingJumpOff", 1.0, 0.2, 0.2, false, false);
        }
        self.set_move(Move::SwingJump, false, false);
    }

    /// TdMove_Swing.LetGo.
    fn swing_let_go(&mut self, m: Move) {
        self.play_move_anim(m, Slot::FullBody, "SwingEnd", 1.0, 0.2, 0.2, false, false);
        self.set_move(Move::Falling, false, false);
    }

    // ------------------------------------------------------------------ SwingJump

    /// TdMove_SwingJump.StartMove.
    pub fn swing_jump_start_move(&mut self, m: Move) {
        self.set_animation_movement_state(Move::Swing, 0.0);
        self.set_animation_movement_state(Move::None, 0.3);
        self.physics_move_start_move(m);
        let Some(ti) = self.moves.swing_jump.target_volume else { return };
        let sj = &self.moves.swing_jump;
        self.pawn.gravity_modifier = sj.gravity_modifier;
        self.pawn.gravity_modifier_timer = sj.gravity_modifier_timer;
        let off = sj.target_volume_offset;
        self.set_physics(Physics::Flying);
        let t = self.swings[ti].vol.location;
        let to = t - self.pawn.location;
        let dist = to.size_2d();
        let target = t + to.safe_normal() * off.x + Vec3::new(0.0, 0.0, 1.0) * off.z;
        self.set_precise_location(m, target, PreciseMode::SimJump, dist / 0.9);
        self.play_move_anim(m, Slot::FullBody, "SwingOff", 1.0, 0.2, 0.2, false, false);
        self.set_move_countdown(m, 0.9);
    }

    /// TdMove_SwingJump.OnTimer.
    pub fn swing_jump_on_timer(&mut self, _m: Move) {
        self.set_physics(Physics::Falling);
    }
}
