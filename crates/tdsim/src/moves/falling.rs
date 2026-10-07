//! TdMove_Falling (script + UTdMove_Falling natives vt70/vt71).

use crate::config::Config;
use crate::math::Vec3;
use crate::pawn::{Move, Slot};
use crate::sim::Sim;

pub struct Falling {
    pub sticky_aim_after_air_time: f32,
    pub air_time: f32,
    pub close_to_ground: bool,
    pub previous_move: Move,
}

impl Falling {
    pub fn new(cfg: &Config) -> Self {
        Falling {
            sticky_aim_after_air_time: cfg.f32(&["TdMove_Falling"], "StickyAimAfterAirTime", 0.0),
            air_time: 0.0,
            close_to_ground: false,
            previous_move: Move::None,
        }
    }
}

impl Sim {
    pub fn falling_can_do_move(&mut self, m: Move) -> bool {
        let ms = self.pawn.movement_state;
        if ms == Move::SkillRoll && self.moves.base(ms).move_active_time < 0.8 {
            return false;
        }
        if matches!(ms, Move::Crouch | Move::Slide) {
            let stand = self.pawn.location + Vec3::new(0.0, 0.0, 34.0);
            if !self.can_stand(stand, false) {
                return false;
            }
        }
        self.tdmove_can_do_move(m)
    }

    pub fn falling_start_move(&mut self, m: Move) {
        // Reset()
        self.moves.falling.air_time = 0.0;
        self.moves.falling.close_to_ground = false;
        self.physics_move_start_move(m);
        let old = self.pawn.old_movement_state;
        self.moves.falling.previous_move = old;
        if old == Move::Walking {
            if self.pawn.rotation.vector().dot(self.pawn.velocity) < 0.0 {
                let below = self.pawn.location - Vec3::new(0.0, 0.0, 1.0) * self.pawn.default_collision_height * 2.0;
                if self.can_stand(below, true) {
                    self.pawn.velocity.x = 0.0;
                    self.pawn.velocity.y = 0.0;
                    self.play_move_anim(m, Slot::FullBodyDir, "JumpStill", 1.0, 0.3, 0.2, false, false);
                }
            } else {
                self.play_move_anim(m, Slot::FullBodyDir, "JumpAir", 1.0, 0.3, 0.2, false, false);
            }
        } else if old == Move::WallClimbing {
            self.reset_camera_look(m, 0.5);
        } else if old == Move::DodgeJump {
            self.set_animation_movement_state(Move::DodgeJump, -1.0);
        }
    }

    pub fn falling_stop_move(&mut self, m: Move) {
        if self.pawn.old_movement_state != Move::DodgeJump {
            self.anim.stop(Slot::FullBody, 0.2);
            self.anim.stop(Slot::FullBodyDir, 0.2);
        }
        self.physics_move_stop_move(m);
    }

    /// UTdMove_Falling vt71: once per tick, look 0.4 s ahead along the velocity from the feet;
    /// when that hits the ground, fire CloseToGround (blend out the in-air animation).
    pub fn falling_tick_close_to_ground(&mut self, _m: Move) {
        if self.moves.falling.close_to_ground {
            return;
        }
        let p = &self.pawn;
        if p.velocity.x.hypot(p.velocity.y) <= 10.0 {
            return;
        }
        let feet = p.location - Vec3::new(0.0, 0.0, p.collision_height);
        let end = feet + p.velocity * 0.4;
        let hit = self.world.line_check(end, feet, Vec3::ZERO);
        self.moves.falling.close_to_ground = hit.hit;
        if hit.hit {
            self.anim.stop(Slot::FullBodyDir, 0.3);
        }
    }
}
