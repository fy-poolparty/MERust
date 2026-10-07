//! TdMove_Crouch (vt70 0x11FE160) and TdMove_Slide (vt70 0x11FDDE0, vt75/vt76), plus
//! UTdMove::TestCanUnCrouch (0x11FA8D0).

use crate::math::{Rotator, UeVec, Vec3};
use crate::pawn::{Move, MoveAction, MoveActionHint, Physics, Slot};
use crate::sim::Sim;

const T_DISABLE_ROOT_OFFSET: u8 = 1;

pub struct Slide {
    pub slide_abort_speed: f32,
    pub slide_abort_time: f32,
    pub max_floor_incline_z: f32,
    pub slide_angle_target: i32,
    pub going_into: bool,
    pub request_uncrouch: bool,
}

impl Slide {
    pub fn new(cfg: &crate::config::Config) -> Self {
        let c = &["TdMove_Slide"];
        Slide {
            slide_abort_speed: cfg.f32(c, "SlideAbortSpeed", 250.0),
            slide_abort_time: cfg.f32(c, "SlideAbortTime", 2.0),
            max_floor_incline_z: cfg.f32(c, "MaxFloorInclineZ", 0.5),
            slide_angle_target: 0,
            going_into: false,
            request_uncrouch: false,
        }
    }
}

impl Sim {
    /// UTdMove::TestCanUnCrouch: room for the default (standing) cylinder above us?
    pub fn test_can_uncrouch(&self) -> bool {
        let p = &self.pawn;
        let start = p.location;
        let end = Vec3::new(start.x, start.y, start.z + p.default_collision_height * 2.0 - 122.0);
        let r = p.default_collision_radius;
        let ext = Vec3::new(r, r, p.default_collision_height);
        !self.movement_trace_for_blocking(end, start, ext)
    }

    // ---------------------------------------------------------------- Crouch

    /// TdMove_Crouch.CanDoMove (does not call super).
    pub fn crouch_can_do_move(&mut self, _m: Move) -> bool {
        self.pawn.movement_state != Move::Crouch
    }

    /// TdMove_Crouch.StartMove.
    pub fn crouch_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        self.set_physics(Physics::Walking);
        self.anim.stop(Slot::FullBodyDir, 0.25);
        self.anim.stop(Slot::LowerBody, 0.25);
        if self.pawn.old_movement_state == Move::Walking {
            // EnableFootPlacement / SetRootOffset((0,0,15), 0.1): visual only on the 1p mesh
            self.set_root_offset(Vec3::new(0.0, 0.0, 15.0), 0.1);
            self.set_move_timer(m, 0.15, false, T_DISABLE_ROOT_OFFSET);
        }
    }

    pub fn crouch_on_move_timer(&mut self, _m: Move, id: u8) {
        if id == T_DISABLE_ROOT_OFFSET {
            self.set_root_offset(Vec3::ZERO, 0.1);
        }
    }

    /// TdMove_Crouch.StopMove.
    pub fn crouch_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        if self.pawn.pending_movement_state == Move::Walking {
            self.play_move_anim(m, Slot::Camera, "CrouchIntoStand", 1.0, 0.2, 0.2, false, false);
        }
        self.pawn.avoid_ledges = true;
    }

    /// TdMove_Crouch.HandleMoveAction.
    pub fn crouch_handle_move_action(&mut self, _m: Move, a: MoveAction) {
        self.pawn.avoid_ledges = a == MoveAction::Crouch;
    }

    /// UTdMove_Crouch vt70: only refreshes bCanUnCrouch.
    pub fn crouch_tick(&mut self) {
        self.pawn.can_uncrouch = self.test_can_uncrouch();
    }

    // ---------------------------------------------------------------- Slide

    /// TdMove_Slide.CanDoMove.
    pub fn slide_can_do_move(&mut self, _m: Move) -> bool {
        let p = &self.pawn;
        if p.movement_state == Move::SkillRoll {
            return false;
        }
        if p.velocity.dot(p.rotation.vector()) < 350.0 {
            return false;
        }
        let mut vel_dir = p.velocity;
        vel_dir.z = 0.0;
        let vel_dir = vel_dir.safe_normal();
        let mut slope = -p.floor;
        slope.z = 0.0;
        let slope = slope.safe_normal();
        let vel_dot_floor = vel_dir.dot(slope);
        let incline = (1.0 - (p.floor.z * p.floor.z).clamp(0.0, 1.0)).sqrt() * vel_dot_floor;
        if incline > self.moves.slide.max_floor_incline_z {
            return false;
        }
        true
    }

    /// TdMove_Slide.CanStopMove.
    pub fn slide_can_stop_move(&mut self) -> bool {
        self.moves.slide.request_uncrouch = true;
        !self.moves.slide.going_into
    }

    /// TdMove_Slide.StartMove.
    pub fn slide_start_move(&mut self, m: Move) {
        self.tdmove_start_move(m);
        self.anim.stop(Slot::UpperBody, 0.1);
        self.play_move_anim(m, Slot::FullBody, "CrouchSlide", 1.0, 0.4, 0.4, false, false);
        self.moves.slide.going_into = true;
        self.moves.slide.request_uncrouch = false;
        self.set_move_countdown(m, 0.5);
        self.start_slide_effect();
    }

    /// TdMove_Slide.StopMove.
    pub fn slide_stop_move(&mut self, m: Move) {
        self.tdmove_stop_move(m);
        self.stop_slide_effect();
        self.anim.stop(Slot::FullBody, 0.2);
        self.pawn.velocity = self.pawn.velocity / 2.0;
        self.play_move_anim(m, Slot::FullBody, "CrouchSlideToCrouch", 1.0, 0.1, 0.2, false, false);
        self.pawn.face_rotation_time_left = 0.4;
        self.pawn.leg_rotation = self.pc.rotation.yaw;
    }

    /// TdMove_Slide.OnTimer.
    pub fn slide_on_timer(&mut self, _m: Move) {
        self.moves.slide.going_into = false;
        if self.moves.slide.request_uncrouch {
            self.set_move(Move::Crouch, false, false);
        }
    }

    /// UTdMove_Slide vt76: stop when pulling back or too slow.
    fn slide_should_abort(&self) -> bool {
        let p = &self.pawn;
        let s = self.moves.slide.slide_abort_speed;
        p.move_action_hint == MoveActionHint::Down || s * s > p.velocity.length_squared()
    }

    /// UTdMove_Slide::FloorDeclineTooSteep (vt75).
    pub fn slide_floor_decline_too_steep(&self) -> bool {
        let f = self.pawn.floor;
        (f.y + f.x) * 0.0 - f.z * 1.0 > -0.72
    }

    /// UTdMove_Slide vt70 (after the TdMove precise-location update): steer the slide.
    pub fn slide_tick(&mut self, m: Move, dt: f32) {
        if self.health <= 0 {
            return;
        }
        if self.slide_should_abort() {
            // event AbortMove
            self.set_move(Move::Crouch, false, false);
        }
        let d = ((self.pc.rotation.yaw - self.pawn.rotation.yaw) as u16 as i16) as i32;
        let mut v = (dt * 0.2 * d as f32) as i32;
        match self.pawn.move_action_hint {
            MoveActionHint::Left => v += (dt * -2000.0) as i32,
            MoveActionHint::Right => v += (dt * 2000.0) as i32,
            _ => {}
        }
        self.pawn.rotation.yaw += v;
        let speed = self.pawn.velocity.size_2d();
        self.pawn.velocity = self.pawn.rotation.vector() * speed;
        self.moves.slide.slide_angle_target = 0x4000 - Rotator::from_vector(self.pawn.rotation.vector()).pitch;
        self.pawn.can_uncrouch = self.test_can_uncrouch();
        let _ = m;
    }
}
