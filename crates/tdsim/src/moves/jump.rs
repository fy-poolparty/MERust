//! TdMove_Jump.

use crate::config::Config;
use crate::math::{UeVec, Vec3};
use crate::moves::PreciseMode;
use crate::pawn::{Move, Physics, Slot};
use crate::sim::Sim;

pub struct Jump {
    pub base_jump_z: f32,
    pub base_jump_z_heavy: f32,
    pub jump_add_xy: f32,
    pub long_jump_slow_threshold: f32,
    pub long_jump_normal_threshold: f32,
    pub long_jump_fast_threshold: f32,
    pub jump_blend_in_time: f32,
    pub jump_blend_out_time: f32,
    pub pre_jump_momentum: f32,
    pub wanted_jump_velocity: Vec3,
    pub can_do_move_taser_limit: f32,
}

impl Jump {
    pub fn new(cfg: &Config) -> Self {
        let ch = &["TdMove_Jump"];
        Jump {
            base_jump_z: cfg.f32(ch, "BaseJumpZ", 630.0),
            base_jump_z_heavy: cfg.f32(ch, "BaseJumpZHeavy", 430.0),
            jump_add_xy: cfg.f32(ch, "JumpAddXY", 100.0),
            long_jump_slow_threshold: cfg.f32(ch, "LongJumpSlowThreshold", 400.0),
            long_jump_normal_threshold: cfg.f32(ch, "LongJumpNormalThreshold", 500.0),
            long_jump_fast_threshold: cfg.f32(ch, "LongJumpFastThreshold", 700.0),
            jump_blend_in_time: cfg.f32(ch, "JumpBlendInTime", 0.1),
            jump_blend_out_time: cfg.f32(ch, "JumpBlendOutTime", 0.2),
            pre_jump_momentum: 0.0,
            wanted_jump_velocity: Vec3::ZERO,
            can_do_move_taser_limit: 0.5,
        }
    }
}

/// UnrealScript native 1500: project `a` onto `b`.
pub fn project_on_to(a: Vec3, b: Vec3) -> Vec3 {
    let d = b.dot(b);
    if d == 0.0 { Vec3::ZERO } else { b * (a.dot(b) / d) }
}

impl Sim {
    pub fn jump_can_do_move(&mut self, m: Move) -> bool {
        if !self.tdmove_can_do_move(m) {
            return false;
        }
        if self.pawn.uncontrolled_slide && self.pawn.movement_state != Move::RumpSlide {
            return false;
        }
        if self.mobility_multiplier() < self.moves.jump.can_do_move_taser_limit {
            return false;
        }
        true
    }

    pub fn jump_start_move(&mut self, m: Move) {
        // IsOkToJump() is false in the shipping build, so NotifyJump never runs.
        self.pawn.last_jump_location = self.pawn.location;
        let mut v = self.pawn.velocity;
        self.moves.jump.pre_jump_momentum = v.size_2d();
        let fwd = self.pawn.rotation.vector();
        let heavy = self.heavy_weapon();
        if fwd.dot(self.pawn.velocity) > 10.0 && !heavy {
            v += fwd * self.moves.jump.jump_add_xy;
        }
        // BaseJumpZHeavy with a heavy gun
        let z = if heavy { self.moves.jump.base_jump_z_heavy } else { self.moves.jump.base_jump_z };
        v.z = z * self.mobility_multiplier();
        self.moves.jump.wanted_jump_velocity = v;
        self.physics_move_start_move(m);

        if self.pawn.found_ledge && !heavy {
            let p = &self.pawn;
            let z_to_ledge = p.move_ledge_location.z - (p.location.z - p.collision_height);
            let mut mn2 = -p.move_normal;
            mn2.z = 0.0;
            let mn2 = mn2.safe_normal();
            if z_to_ledge < 112.0 && mn2.dot(p.velocity.safe_normal()) > 0.7071 {
                let mut at_ledge_z = p.location;
                at_ledge_z.z = p.move_ledge_location.z;
                let mut t2d = project_on_to(p.move_ledge_location - at_ledge_z, mn2).size_2d();
                t2d -= p.collision_radius / mn2.x.abs().max(mn2.y.abs());
                t2d -= 8.0;
                t2d = t2d.max(0.0);
                t2d /= project_on_to(v, -mn2).size_2d();
                let z_with_time = v.z * t2d - t2d * t2d * p.gravity_z().abs() * 0.5;
                if z_with_time < z_to_ledge {
                    let mut wanted = p.move_ledge_location;
                    wanted.z += p.collision_height + 2.0;
                    let mut start = p.location;
                    start.z = wanted.z;
                    if !self.movement_trace_for_blocking(wanted, start, self.pawn.extent()) {
                        // "Jump assist": fly to the ledge height with collision off, then jump.
                        self.set_physics(Physics::Flying);
                        self.pawn.collide_world = false;
                        let speed = v.length();
                        self.set_precise_location(m, wanted, PreciseMode::Fly, speed);
                        return;
                    }
                }
            }
        }
        self.jump_start_jump(m);
    }

    pub fn jump_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        if !self.pawn.collide_world {
            self.pawn.collide_world = true;
            self.set_physics(Physics::Falling);
        }
        let pending = self.pawn.pending_movement_state;
        if pending != Move::Falling && pending != Move::VaultOver {
            self.anim.stop(Slot::FullBodyDir, 0.2);
        }
    }

    /// TdMove_Jump.StartJump.
    pub fn jump_start_jump(&mut self, m: Move) {
        let fwd = self.pawn.rotation.vector();
        let fv = fwd.dot(self.pawn.velocity);
        let (bi, bo) = (self.moves.jump.jump_blend_in_time, self.moves.jump.jump_blend_out_time);
        if fv < 5.0 {
            self.play_move_anim(m, Slot::FullBodyDir, "JumpStill", 1.0, 0.15, 0.15, false, false);
        } else if fv < self.moves.jump.long_jump_normal_threshold {
            self.play_move_anim(m, Slot::FullBodyDir, "JumpSlow", 1.0, bi, bo, false, false);
        } else {
            let land = self.pawn.location + fwd * (fv * 1.1);
            if self.movement_trace_for_blocking(land - Vec3::new(0.0, 0.0, 200.0), land, self.pawn.extent()) {
                self.play_move_anim(m, Slot::FullBodyDir, "JumpSlow", 1.0, bi, bo, false, false);
            } else {
                self.play_move_anim(m, Slot::FullBodyDir, "JumpFast", 1.0, bi, bo, false, false);
            }
        }
        self.set_physics(Physics::Falling);
        self.pawn.velocity = self.moves.jump.wanted_jump_velocity;
    }

    /// TdMove_Jump.ReachedPreciseLocation.
    pub fn jump_reached_precise_location(&mut self, m: Move) {
        self.pawn.collide_world = true;
        self.jump_start_jump(m);
    }
}
