//! UTdMove vt70 (0x11FCE10): move the pawn to `PreciseLocation` and turn it to
//! `PreciseRotation`, then fire ReachedPreciseLocation / ReachedPreciseRotation /
//! ReachedPreciseLocationAndRotation.

use crate::math::{Rotator, UeVec, Vec3, norm_axis};
use crate::moves::{Class, PreciseMode, class_of};
use crate::pawn::Move;
use crate::sim::Sim;

impl Sim {
    pub(crate) fn update_precise_location(&mut self, m: Move, dt: f32) {
        let b = self.moves.base(m);
        if b.use_precise_location && !b.reached_precise_location {
            let target = b.precise_location;
            let speed = b.precise_location_speed;
            let mode = b.precise_location_mode;
            let loc = self.pawn.location;
            let d = target - loc;
            let dist3 = d.length();
            let dist2 = d.size_2d();
            let dist = if mode == PreciseMode::Walk { dist2 } else { dist3 };
            if dist <= 2.0 {
                self.moves.base_mut(m).reached_precise_location = true;
            } else {
                let g = self.pawn.gravity_z().abs();
                let p = &mut self.pawn;
                match mode {
                    PreciseMode::Fly => {
                        let s = (dist3 / dt).min(speed);
                        p.velocity = d.safe_normal() * s;
                        p.acceleration = Vec3::ZERO;
                    }
                    PreciseMode::Walk => {
                        let s = (dist2 / dt).min(speed);
                        p.velocity = d.safe_normal_2d() * s;
                        p.acceleration = Vec3::ZERO;
                    }
                    PreciseMode::Jump => {
                        let s = (dist2 / dt).min(speed);
                        let h = d.safe_normal_2d();
                        p.velocity.x = h.x * s;
                        p.velocity.y = h.y * s;
                        let dz = d.z;
                        if dz < 0.0 {
                            let vz = p.velocity.z;
                            let a = if dist2 <= 0.0 {
                                -g
                            } else {
                                let t = dist2 / s;
                                (dz - t * vz) * 2.0 / (t * t)
                            };
                            p.velocity.z = (dz / dt).max(a * dt + vz);
                        } else {
                            let vz = if dist2 <= 0.0 { 640.0 } else { s / dist2 * (dz * 2.0) };
                            p.velocity.z = (dz / dt).min(vz);
                        }
                        p.acceleration = Vec3::ZERO;
                    }
                    PreciseMode::SimJump => {
                        let s = (dist2 / dt).min(speed);
                        let h = d.safe_normal_2d();
                        p.velocity.x = h.x * s;
                        p.velocity.y = h.y * s;
                        let dz = d.z;
                        let t = dist2 / s;
                        if t <= dt {
                            if dz <= 0.0 {
                                p.velocity.z -= g * dt;
                                p.velocity.z = (dz / dt).max(p.velocity.z);
                            } else {
                                let r = (dz / g).sqrt();
                                p.velocity.z = g * r + g * r;
                                p.velocity.z = (dz / dt).min(p.velocity.z);
                            }
                        } else {
                            p.velocity.z = dz / t + t * g * 0.5;
                        }
                        p.acceleration = Vec3::ZERO;
                    }
                    PreciseMode::Fall => {
                        let s = (dist2 / dt).min(speed);
                        let h = d.safe_normal_2d();
                        p.velocity.x = h.x * s;
                        p.velocity.y = h.y * s;
                        let step = dt * speed;
                        p.acceleration.x = 0.0;
                        p.acceleration.y = 0.0;
                        let mut vz = p.velocity.z;
                        if vz < 0.0 {
                            if target.z > p.location.z {
                                self.precise_failed(m);
                            } else if step > dist2 {
                                vz = ((target.z - p.location.z) / dt).max(vz);
                                let next = p.location + Vec3::new(p.velocity.x * dt, p.velocity.y * dt, vz * dt);
                                let start = p.location;
                                let ext = p.extent();
                                if self.movement_trace_for_blocking(next, start, ext) {
                                    self.precise_failed(m);
                                } else {
                                    self.pawn.velocity.z = vz - g * dt;
                                }
                            }
                        }
                    }
                }
            }
        }

        let b = self.moves.base(m);
        if b.use_precise_rotation && !b.reached_precise_rotation {
            let time = b.precise_rotation_interpolation_time;
            let target_yaw = b.precise_rotation.yaw;
            let disable_ctrl = b.disable_controller_facing_pawn_yaw_rotation;
            let mut r = self.pawn.rotation;
            if time <= dt {
                r.yaw = target_yaw;
                self.moves.base_mut(m).reached_precise_rotation = true;
                if !disable_ctrl {
                    self.pc.rotation.yaw = target_yaw;
                }
            } else {
                let diff = norm_axis(target_yaw - r.yaw);
                let step = (diff as f32 * (dt / time)) as i32;
                r.yaw = norm_axis(r.yaw + step);
                if !disable_ctrl {
                    self.pc.rotation.yaw = norm_axis(self.pc.rotation.yaw + step);
                }
                self.moves.base_mut(m).precise_rotation_interpolation_time -= dt;
            }
            self.pawn.rotation = Rotator::new(r.pitch, r.yaw, r.roll);
        }

        // callbacks
        let mut both = false;
        let b = self.moves.base(m).clone();
        if b.use_precise_location && b.reached_precise_location {
            let bm = self.moves.base_mut(m);
            bm.use_precise_location = false;
            self.reached_precise_location(m);
            let b = self.moves.base(m).clone();
            if !b.use_precise_rotation || b.reached_precise_rotation {
                both = true;
                if !b.delay_rotation_and_location_callback {
                    self.reached_precise_location_and_rotation(m);
                }
            } else {
                self.moves.base_mut(m).delay_rotation_and_location_callback = true;
            }
        }
        let b = self.moves.base(m).clone();
        if b.use_precise_rotation && b.reached_precise_rotation {
            self.moves.base_mut(m).use_precise_rotation = false;
            self.reached_precise_rotation(m);
            let b = self.moves.base(m).clone();
            if !b.use_precise_location || b.reached_precise_location {
                both = true;
                if !b.delay_rotation_and_location_callback {
                    self.reached_precise_location_and_rotation(m);
                }
            } else {
                self.moves.base_mut(m).delay_rotation_and_location_callback = true;
            }
        }
        if self.moves.base(m).delay_rotation_and_location_callback && both {
            self.moves.base_mut(m).delay_rotation_and_location_callback = false;
            self.reached_precise_location_and_rotation(m);
        }
    }

    fn precise_failed(&mut self, m: Move) {
        let b = self.moves.base_mut(m);
        b.use_precise_location = false;
        b.reached_precise_location = false;
        b.delay_rotation_and_location_callback = false;
        self.failed_to_reach_precise_location(m);
    }

    pub fn reached_precise_location(&mut self, m: Move) {
        match class_of(m) {
            Class::Jump => self.jump_reached_precise_location(m),
            Class::IntoGrab => self.into_grab_reached_precise_location(m),
            Class::SpringBoard => self.springboard_reached_precise_location(m),
            Class::IntoClimb => self.into_climb_reached_precise_location(m),
            Class::Climb => self.climb_reached_precise_location(m),
            Class::GrabTransfer => self.grab_transfer_reached_precise_location(m),
            Class::IntoZipLine => self.into_zipline_reached_precise_location(m),
            Class::Disarm => self.disarm_reached_precise_location(m),
            Class::MeleeAirAbove => self.melee_air_above_reached_precise_location(m),
            _ => {}
        }
    }

    pub fn reached_precise_rotation(&mut self, m: Move) {
        match class_of(m) {
            Class::WallClimb180TurnJump => self.wallclimb_180_reached_precise_rotation(m),
            // TdMove_180Turn.ReachedPreciseRotation
            Class::Turn180 => {
                self.set_move(Move::Walking, false, false);
            }
            _ => {}
        }
    }

    pub fn reached_precise_location_and_rotation(&mut self, _m: Move) {}

    pub fn failed_to_reach_precise_location(&mut self, m: Move) {
        match class_of(m) {
            Class::IntoGrab => self.into_grab_failed_precise_location(m),
            Class::SpeedVault | Class::VaultOver => self.vault_failed_precise_location(m),
            Class::SpringBoard => self.springboard_failed_precise_location(m),
            Class::IntoClimb => self.into_climb_failed_precise_location(m),
            Class::GrabTransfer => self.grab_transfer_fall(m),
            Class::IntoZipLine => self.into_zipline_fall(m),
            Class::Disarm => self.disarm_failed_precise_location(m),
            Class::MeleeAirAbove => self.melee_air_above_failed_precise_location(m),
            _ => {}
        }
    }
}
