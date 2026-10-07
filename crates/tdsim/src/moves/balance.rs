//! TdMove_Balance (TdBalanceWalkVolume): walking a narrow beam or pipe. The lean
//! (BalanceFactor, -1 left .. 1 right) runs away by itself, faster the faster you walk; strafing
//! and turning the view push it back (UTdMove_Balance vt70, 0x120A280). Past full lean you are
//! in danger: get it back under 0.9 within TimeToCounter or fall off. Each tick the pawn is
//! pulled back onto the volume's spline and faced along it (vt71, 0x11FA570).

use crate::config::Config;
use crate::math::{Rotator, UeVec, Vec3};
use crate::pawn::{Move, Physics, Slot};
use crate::sim::Sim;

pub struct Balance {
    pub volume: Option<usize>,
    pub current_param_on_curve: f32,
    pub facing_forward: bool,
    pub danger: bool,
    pub has_fallen: bool,
    pub balance_factor: f32,
    pub counter_timer: f32,
    pub external_force: f32,
    pub time_to_counter: f32,
    pub gravity_influence: f32,
    pub control_influence: f32,
    pub speed_influence: f32,
    pub camera_influence: f32,
}

impl Balance {
    pub fn new(cfg: &Config) -> Self {
        let c = &["TdMove_Balance"];
        Balance {
            volume: None,
            current_param_on_curve: 0.0,
            facing_forward: true,
            danger: false,
            has_fallen: false,
            balance_factor: 0.0,
            counter_timer: 0.0,
            external_force: 0.0,
            time_to_counter: cfg.f32(c, "TimeToCounter", 0.8),
            gravity_influence: cfg.f32(c, "GravityInfluence", 0.3),
            control_influence: cfg.f32(c, "ControlInfluence", 1.5),
            speed_influence: cfg.f32(c, "SpeedInfluence", 2.5),
            camera_influence: cfg.f32(c, "CameraInfluence", 0.3),
        }
    }
}

/// SetMoveTimer id for TdMove_Balance.GoIntoFalling.
const GO_INTO_FALLING: u8 = 0;

impl Sim {
    /// TdBalanceWalkVolume.PawnUpdate.
    pub(crate) fn balance_volume_pawn_update(&mut self, i: usize) {
        self.moves.balance.volume = Some(i);
        if self.can_do_move(Move::Balance) {
            self.set_move(Move::Balance, false, false);
        }
    }

    /// TdBalanceWalkVolume.PawnLeavingVolume: walking off either end.
    pub(crate) fn balance_volume_pawn_leaving(&mut self) {
        if self.pawn.movement_state == Move::Balance && !self.moves.balance.has_fallen {
            self.set_move(Move::Walking, false, false);
        }
    }

    /// TdMove_Balance.CanDoMove: from walking, crouching or sliding.
    pub fn balance_can_do_move(&mut self, m: Move) -> bool {
        if !matches!(self.pawn.movement_state, Move::Walking | Move::Crouch | Move::Slide) {
            return false;
        }
        self.tdmove_can_do_move(m)
    }

    /// TdMove_Balance.StartMove.
    pub fn balance_start_move(&mut self, m: Move) {
        // TdMove_Balance.StartMove: a heavy gun is dropped
        if self.heavy_weapon() {
            self.reset_camera_look(m, 0.5);
            self.drop_weapon();
        }
        self.physics_move_start_move(m);
        let v = self.balances[self.moves.balance.volume.expect("balance without a volume")].clone();
        let (_, param) = v.find_closest_point_on_dspline(self.pawn.location, 0);
        self.moves.balance.current_param_on_curve = param;
        let dir = v.slope_on_spline(param / v.num_spline_segments as f32);
        let facing = self.pawn.rotation.vector();
        let forward = facing.dot(dir.safe_normal()) > 0.0;
        self.moves.balance.facing_forward = forward;
        let sign = if forward { 1.0 } else { -1.0 };
        self.set_precise_rotation(m, Rotator::from_vector(dir * sign), 0.2);
        self.moves.balance.balance_factor = (facing * if forward { -0.2 } else { 0.2 }).dot(dir.cross(Vec3::new(0.0, 0.0, 1.0)));
        self.moves.balance.external_force = 0.0;
        self.moves.balance.has_fallen = false;
        self.moves.balance.danger = false;
    }

    /// TdMove_Balance.StopMove.
    pub fn balance_stop_move(&mut self, m: Move) {
        self.use_root_motion(false);
        self.physics_move_stop_move(m);
        self.pawn.face_rotation_time_left = 0.4;
    }

    /// UTdMove_Balance vt70 (0x120A280), after the TdPhysicsMove part.
    pub fn balance_tick(&mut self, _m: Move, dt: f32) {
        let b = &self.moves.balance;
        if b.has_fallen {
            return;
        }
        let p = &self.pawn;
        // the strafe input (the controller's acceleration across the facing)
        let control = p.acceleration.dot(p.rotation.vector().cross(Vec3::new(0.0, 0.0, 1.0))).clamp(-1.0, 1.0);
        let lean = b.balance_factor;
        let gravity = (lean * std::f32::consts::FRAC_PI_2).sin();
        let speed = (p.velocity.size_2d() / 300.0).clamp(0.0, 1.0);
        let speed_scale = b.speed_influence * speed + 1.0;
        let look = ((self.pc.rotation - p.rotation).normalize().yaw as f32 / 8192.0).clamp(-1.0, 1.0);
        let rate = (b.camera_influence * look + b.gravity_influence * gravity + b.control_influence * control + b.external_force) * speed_scale;
        let lean = (lean + rate * dt).clamp(-1.0, 1.0);
        let b = &mut self.moves.balance;
        b.balance_factor = lean;
        if b.danger {
            b.counter_timer += dt;
            if b.counter_timer > b.time_to_counter {
                self.balance_falloff();
            } else if lean.abs() <= 0.9 {
                b.danger = false;
            }
        } else if lean.abs() >= 1.0 {
            b.counter_timer = 0.0;
            b.danger = true;
        }
    }

    /// UTdMove_Balance vt71 (0x11FA570): back towards the spline, as fast as you walk, facing
    /// along it.
    pub fn balance_tick_move(&mut self, dt: f32) {
        if self.moves.balance.has_fallen {
            return;
        }
        let Some(vi) = self.moves.balance.volume else { return };
        let v = self.balances[vi].clone();
        let hint = self.moves.balance.current_param_on_curve as i32;
        let (closest, param) = v.find_closest_point_on_dspline(self.pawn.location, hint);
        self.moves.balance.current_param_on_curve = param;
        let mut dir = v.slope_on_spline(param / v.spline_locations.len() as f32);
        if !self.moves.balance.facing_forward {
            dir = -dir;
        }
        dir.z = 0.0;
        let speed = self.pawn.velocity.size_2d() / 300.0;
        let loc = self.pawn.location;
        let delta = Vec3::new(closest.x - loc.x, closest.y - loc.y, 0.0) * (dt * 10.0 * speed);
        self.move_actor(delta);
        self.pawn.rotation = Rotator::from_vector(dir.safe_normal());
    }

    /// TdMove_Balance.Falloff (event).
    fn balance_falloff(&mut self) {
        let m = Move::Balance;
        self.moves.balance.has_fallen = true;
        self.set_physics(Physics::Falling);
        let anim = if self.moves.balance.balance_factor < 0.0 { "walkbalancefalloffleft" } else { "walkbalancefalloffright" };
        self.play_move_anim(m, Slot::FullBody, anim, 1.0, 0.3, 0.3, true, false);
        self.use_root_motion(true);
    }

    /// TdMove_Balance.OnCeaseRelevantRootMotion.
    pub fn balance_on_cease_relevant_root_motion(&mut self, m: Move) {
        self.set_move_timer(m, 0.1, false, GO_INTO_FALLING);
        self.set_animation_movement_state(Move::Falling, 0.0);
    }

    /// TdMove_Balance.GoIntoFalling (move timer).
    pub fn balance_on_move_timer(&mut self, _m: Move, id: u8) {
        if id == GO_INTO_FALLING {
            self.set_move(Move::Falling, false, false);
        }
    }

    /// TdMove_Balance.HitWall.
    pub fn balance_hit_wall(&mut self, _m: Move) {
        self.anim.stop(Slot::FullBody, 0.1);
        self.set_move(Move::Landing, false, false);
    }
}
