//! TdMove_Landing.

use crate::config::Config;
use crate::math::{UeVec, Vec3};
use crate::pawn::{Move, MoveActionHint, Slot};
use crate::sim::Sim;

pub struct Landing {
    pub hard_landing_damage: f32,
    pub landing_speed_reduction: f32,
    pub hard_landing_height: f32,
    pub skill_roll_landing_height: f32,
    pub soft_landing_height: f32,
    pub force_land_back: bool,
    pub last_landing_was_on_soft_object: bool,
    /// LandNode1p.Landed / the 'LandingRun' custom blend amount of the last landing (for the
    /// animation layer).
    pub landing_amount: f32,
    pub landing_serial: u32,
}

impl Landing {
    pub fn new(cfg: &Config) -> Self {
        let ch = &["TdMove_Landing"];
        Landing {
            hard_landing_damage: cfg.f32(ch, "HardLandingDamage", 15.0),
            landing_speed_reduction: cfg.f32(ch, "LandingSpeedReduction", 65.0),
            hard_landing_height: cfg.f32(ch, "HardLandingHeight", 530.0),
            skill_roll_landing_height: cfg.f32(ch, "SkillRollLandingHeight", 200.0),
            soft_landing_height: cfg.f32(ch, "SoftLandingHeight", 300.0),
            force_land_back: false,
            last_landing_was_on_soft_object: false,
            landing_amount: 0.0,
            landing_serial: 0,
        }
    }
}

impl Sim {
    pub fn landing_can_do_move(&mut self, m: Move) -> bool {
        if matches!(self.pawn.movement_state, Move::Balance | Move::SkillRoll) {
            return false;
        }
        self.tdmove_can_do_move(m)
    }

    pub fn landing_start_move(&mut self, m: Move) {
        self.tdmove_start_move(m);
        let old = self.pawn.old_movement_state;
        let came_from_180_in_air = old == Move::Turn180InAir;
        let fall_h = self.pawn.enter_falling_height - self.pawn.location.z;
        let l = &self.moves.landing;
        let hard = fall_h >= l.hard_landing_height;
        let soft_surface = hard && l.last_landing_was_on_soft_object;
        let dot = self.pawn.rotation.vector().safe_normal().dot(self.pawn.velocity.safe_normal());
        let skill_roll = !soft_surface && fall_h >= l.skill_roll_landing_height && self.can_skill_roll();
        let soft_landing_backwards = self.moves.air.soft_landing_backwards; // TdMove_SoftLanding.bMovingBackwards
        let moving_backwards = (dot < -0.3 && self.pawn.current_walking_state >= crate::pawn::WalkingState::Run)
            || came_from_180_in_air
            || (old == Move::SoftLanding && soft_landing_backwards);
        let force_back = l.force_land_back;
        let last_soft = l.last_landing_was_on_soft_object;
        if skill_roll && !moving_backwards && self.can_do_move(Move::SkillRoll) {
            self.set_move(Move::SkillRoll, false, false);
        } else if came_from_180_in_air || old == Move::LayOnGround || (hard && moving_backwards) || (last_soft && moving_backwards) || force_back {
            self.landing_land_backwards(m);
        } else if soft_surface {
            self.landing_land_on_soft_object(m);
        } else if hard && !self.pawn.uncontrolled_slide {
            self.landing_land_hard(m);
        } else {
            let amount = self.landing_amount_for_fall();
            self.landing_land_normal(m, amount);
            self.landing_end_landing(m);
        }
        self.moves.landing.force_land_back = false;
        self.moves.landing.last_landing_was_on_soft_object = false;
    }

    /// TdMove_Landing.GetLandingAmount.
    fn landing_amount_for_fall(&self) -> f32 {
        let l = &self.moves.landing;
        let h = self.pawn.enter_falling_height - self.pawn.location.z;
        let a = ((h - l.soft_landing_height) / (l.hard_landing_height - l.soft_landing_height)).max(0.0);
        a.clamp(0.2, 1.0)
    }

    /// TdMove_Landing.SubtractLandingSpeed.
    fn landing_subtract_landing_speed(&mut self) {
        let old = self.pawn.old_movement_state;
        let prev = self.moves.falling.previous_move;
        let should = (old == Move::Falling && (prev == Move::Jump || prev == Move::MeleeAir)) || old == Move::Coil || old == Move::MeleeAir;
        if should {
            let target = self.moves.jump.pre_jump_momentum - self.moves.landing.landing_speed_reduction;
            if target < self.pawn.velocity.size_2d() {
                let z = self.pawn.velocity.z;
                let mut v = self.pawn.velocity;
                v.z = 0.0;
                let mut v = v.safe_normal() * target;
                v.z = z;
                self.pawn.velocity = v;
            }
        }
    }

    /// TdMove_Landing.LandNormal.
    fn landing_land_normal(&mut self, _m: Move, mut amount: f32) {
        if self.pawn.move_action_hint == MoveActionHint::Up {
            self.pawn.force_max_accel_one_frame = true;
        }
        if self.pawn.old_movement_state != Move::VaultOver {
            self.landing_subtract_landing_speed();
        }
        if self.pawn.old_movement_state == Move::Coil {
            amount = 0.7;
        }
        let l = &mut self.moves.landing;
        l.landing_amount = amount;
        l.landing_serial += 1;
        let id = if self.pawn.velocity.z < -1000.0 { 9 } else { 8 };
        self.play_foot_step_sound(id);
    }

    /// TdMove_Landing.EndLanding.
    fn landing_end_landing(&mut self, _m: Move) {
        if self.can_stand(self.pawn.location, false) {
            self.set_move(Move::Walking, false, false);
        } else {
            self.set_move(Move::Crouch, false, false);
        }
    }

    fn landing_land_hard(&mut self, m: Move) {
        let name = if self.moves.walking.frand() < 0.5 { "FallingLandHard" } else { "FallingLandHard2" };
        self.play_move_anim(m, Slot::FullBody, name, 1.0, 0.05, 0.2, false, false);
        self.set_ignore_move_input(-1.0);
        self.set_ignore_look_input(-1.0);
        self.pawn.velocity = Vec3::ZERO;
        self.pawn.acceleration = Vec3::ZERO;
        self.reset_camera_look(m, 0.3);
    }

    fn landing_land_on_soft_object(&mut self, m: Move) {
        self.play_move_anim(m, Slot::FullBody, "FallingLandSoftLanding", 1.0, 0.1, 0.1, false, false);
        self.set_ignore_move_input(-1.0);
        self.set_ignore_look_input(-1.0);
        self.pawn.velocity = Vec3::ZERO;
        self.pawn.acceleration = Vec3::ZERO;
        self.reset_camera_look(m, 0.3);
    }

    fn landing_land_backwards(&mut self, m: Move) {
        self.play_move_anim(m, Slot::FullBody, "JumpTurnLanding", 1.0, 0.1, 0.1, false, false);
        self.set_move(Move::LayOnGround, false, false);
        self.pawn.velocity = Vec3::ZERO;
        self.pawn.acceleration = Vec3::ZERO;
        self.reset_camera_look(m, 0.3);
    }

    pub fn landing_on_custom_anim_end(&mut self, _m: Move) {
        self.set_move(Move::Walking, false, false);
    }
}
