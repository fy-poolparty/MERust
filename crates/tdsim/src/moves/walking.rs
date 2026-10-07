//! TdMove_Walking.

use crate::config::Config;
use crate::math::{Rotator, Vec3};
use crate::pawn::{Move, MoveAction, Slot};
use crate::sim::Sim;

const TIMER_IDLE: u8 = 0;

pub struct Walking {
    pub trigger_idle_anim_min_time: f32,
    pub trigger_idle_anim_max_time: f32,
    pub unarmed_idle_anims: Vec<String>,
    pub is_playing_idle_anim: bool,
    pub current_idle_slot: Slot,
    rng: u32,
}

impl Walking {
    pub fn new(cfg: &Config) -> Self {
        let ch = &["TdMove_Walking"];
        Walking {
            trigger_idle_anim_min_time: cfg.f32(ch, "TriggerIdleAnimMinTime", 30.0),
            trigger_idle_anim_max_time: cfg.f32(ch, "TriggerIdleAnimMaxTime", 40.0),
            unarmed_idle_anims: vec!["standidle1".into(), "standidle2".into(), "standidle3".into()],
            is_playing_idle_anim: false,
            current_idle_slot: Slot::Canned,
            rng: 0x1234_5678,
        }
    }

    /// FRand().
    pub fn frand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }
}

impl Sim {
    /// TdMove_Walking.CanDoMove: can't stand up from crouch/slide under a low ceiling.
    pub fn walking_can_do_move(&mut self, m: Move) -> bool {
        if !self.tdmove_can_do_move(m) {
            return false;
        }
        if matches!(self.pawn.movement_state, Move::Crouch | Move::Slide) {
            let start = self.pawn.location;
            let end = start + Vec3::new(0.0, 0.0, self.pawn.default_collision_height * 2.0 - 122.0);
            if self.movement_trace_for_blocking(end, start, self.pawn.extent()) {
                return false;
            }
        }
        true
    }

    pub fn walking_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        self.pawn.illegal_ledge_timer = 0.0;
        let t = self.walking_idle_trigger_time();
        self.set_move_timer(m, t, false, TIMER_IDLE);
    }

    pub fn walking_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        if self.moves.walking.is_playing_idle_anim {
            self.walking_stop_idle(m, 0.4);
        }
    }

    fn walking_idle_trigger_time(&mut self) -> f32 {
        let w = &mut self.moves.walking;
        let var = (w.trigger_idle_anim_max_time - w.trigger_idle_anim_min_time).max(0.0);
        w.trigger_idle_anim_min_time + w.frand() * var
    }

    /// The rest of TdMove_Walking.UpdateViewRotation (idle timer reset while looking around).
    pub fn walking_after_view_rotation(&mut self, m: Move, delta: &Rotator) {
        let moving_view = delta.yaw != 0 || delta.pitch != 0;
        if self.pawn.current_walking_state > crate::pawn::WalkingState::Idle || moving_view {
            if self.moves.walking.is_playing_idle_anim {
                self.walking_stop_idle(m, 0.4);
            }
            let t = self.walking_idle_trigger_time();
            self.set_move_timer(m, t, false, TIMER_IDLE);
        }
    }

    pub fn walking_handle_move_action(&mut self, m: Move, a: MoveAction) {
        if a != MoveAction::None {
            if self.moves.walking.is_playing_idle_anim {
                self.walking_stop_idle(m, 0.4);
            }
            let t = self.walking_idle_trigger_time();
            self.set_move_timer(m, t, false, TIMER_IDLE);
        }
    }

    pub fn walking_on_move_timer(&mut self, m: Move, id: u8) {
        if id == TIMER_IDLE {
            self.walking_play_idle(m);
        }
    }

    /// TdMove_Walking.PlayIdle.
    fn walking_play_idle(&mut self, m: Move) {
        let n = self.moves.walking.unarmed_idle_anims.len();
        if n == 0 {
            let t = self.walking_idle_trigger_time();
            self.set_move_timer(m, t, false, TIMER_IDLE);
            return;
        }
        let i = ((self.moves.walking.frand() * n as f32) as usize).min(n - 1);
        let name = self.moves.walking.unarmed_idle_anims[i].clone();
        // bResetCameraLook=true for all unarmed idles
        self.reset_camera_look(m, 0.6);
        self.play_move_anim(m, Slot::Canned, &name, 1.0, 0.4, 0.4, false, false);
        self.moves.walking.is_playing_idle_anim = true;
        self.moves.walking.current_idle_slot = Slot::Canned;
    }

    fn walking_stop_idle(&mut self, m: Move, blend_out: f32) {
        let slot = self.moves.walking.current_idle_slot;
        self.anim.stop(slot, blend_out);
        self.moves.base_mut(m).reset_camera_look = false;
        self.moves.walking.is_playing_idle_anim = false;
    }

    pub fn walking_on_custom_anim_end(&mut self, m: Move) {
        let t = self.walking_idle_trigger_time();
        self.set_move_timer(m, t, false, TIMER_IDLE);
        self.moves.walking.is_playing_idle_anim = false;
    }
}
