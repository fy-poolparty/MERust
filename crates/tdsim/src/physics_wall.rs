//! ATdPawn::physWallRunning (vt289) and physWallClimbing (vt290), with their slide helpers
//! (0x12B3E50 / 0x12B4550). During both, `Floor` holds the wall normal.

use crate::collision::CheckResult;
use crate::math::{URU_PER_RAD, UeVec, Vec3, norm_axis};
use crate::pawn::{Move, Physics};
use crate::sim::Sim;

impl Sim {
    /// TdPawn.FallingOffWall (event): SetMove(MOVE_Falling).
    pub fn falling_off_wall(&mut self) {
        self.set_move(Move::Falling, false, false);
    }

    /// TdPawn.ReachedWall (event) -> Moves[MovementState].ReachedWall.
    pub fn reached_wall(&mut self) {
        let m = self.pawn.movement_state;
        self.move_reached_wall(m);
    }

    pub fn phys_wall_running(&mut self, dt: f32, mut iterations: i32) {
        let floor_yaw0 = (self.pawn.floor.y.atan2(self.pawn.floor.x) * URU_PER_RAD) as i32;
        if self.pawn.floor.is_nearly_zero() {
            self.falling_off_wall();
            if self.pawn.physics == Physics::WallRunning {
                self.set_physics(Physics::Falling);
            }
            self.start_new_physics(dt, iterations);
            return;
        }
        let friction = self.moves.base(self.pawn.movement_state).friction_modifier;
        let mut accel_dir = self.pawn.acceleration.safe_normal();
        let gs4 = self.pawn.ground_speed * 4.0;
        self.calc_velocity(&mut accel_dir, dt, gs4, friction, false, false, false);
        iterations += 1;
        self.pawn.just_teleported = false;
        let mut vel = self.pawn.velocity;
        let old_loc = self.pawn.location;
        let mut remaining = dt;
        let up_step = |s: &Sim| Vec3::new(0.0, 0.0, s.pawn.max_wall_step_height);

        while remaining > 0.0 && iterations < 8 {
            iterations += 1;
            let tick = if remaining <= 0.05 { remaining } else { (remaining * 0.5).min(0.05) };
            remaining -= tick;
            let delta = vel * tick;
            let mut hit = self.move_actor(delta);
            if hit.time < 1.0 {
                if self.pawn.is_wall_walking {
                    let rest = delta * (1.0 - hit.time);
                    if self.wallrun_slide(rest, &mut hit) {
                        return;
                    }
                } else {
                    let mut saved_n = hit.normal;
                    let up = up_step(self);
                    if hit.normal.z > self.pawn.walkable_floor_z {
                        // step over a walkable bump
                        let rest = delta * (1.0 - hit.time);
                        self.move_actor(up);
                        hit = self.move_actor(rest);
                        if self.pawn.walkable_floor_z > hit.normal.z && hit.time < 1.0 {
                            saved_n = hit.normal;
                        }
                        self.move_actor(-up);
                    }
                    if self.pawn.walkable_floor_z > saved_n.z {
                        let rest = delta * (1.0 - hit.time);
                        self.move_actor(up);
                        hit = self.move_actor(rest);
                        if self.pawn.walkable_floor_z > hit.normal.z && hit.time < 1.0 {
                            // reached the wall: slide along it and start wall walking
                            let n = hit.normal;
                            let v = self.pawn.velocity;
                            let d = v.dot(n);
                            self.pawn.velocity = Vec3::new(v.x - d * n.x, v.y - d * n.y, v.z);
                            self.pawn.is_wall_walking = true;
                            vel = self.pawn.velocity;
                            self.reached_wall();
                            self.pawn.just_teleported = true;
                        }
                        self.move_actor(-up);
                    }
                }
            }
            if self.pawn.is_wall_walking {
                // keep touching the wall 35 uu ahead
                let d2 = self.pawn.velocity.safe_normal_2d();
                if let Some(h) = self.wall_probe(d2) {
                    if self.pawn.floor.dot(h.normal) < 0.96 && h.normal.dot(delta) > 0.0 {
                        self.wallrun_fall_off(dt, iterations);
                        return;
                    }
                    let into = -self.pawn.floor * (self.pawn.max_wall_step_height + 2.0);
                    self.move_actor(into);
                    self.pawn.floor = h.normal;
                } else if self.pawn.physics == Physics::WallRunning {
                    self.wallrun_fall_off(dt, iterations);
                    return;
                }
            } else if self.moves.base(self.pawn.movement_state).move_active_time > 0.2 {
                let d2 = self.pawn.velocity.safe_normal_2d();
                if self.wall_probe(d2).is_none() && self.pawn.physics == Physics::WallRunning {
                    self.wallrun_fall_off(dt, iterations);
                    return;
                }
            }
        }

        if self.pawn.physics == Physics::WallRunning {
            // turn with the wall
            let yaw = (self.pawn.floor.y.atan2(self.pawn.floor.x) * URU_PER_RAD) as i32;
            let dyaw = norm_axis(yaw - floor_yaw0);
            self.pawn.rotation.yaw += dyaw;
            let nv = (self.pawn.location - old_loc) * (1.0 / dt);
            if self.pawn.velocity.length_squared() > nv.length_squared() && !self.pawn.just_teleported {
                self.pawn.velocity = nv;
            }
        }
    }

    /// Box probe (extent 1/4 of the cylinder, lowered by 3/4 of that height) from 35 uu ahead
    /// into the wall by MaxWallStepHeight + 2.
    fn wall_probe(&self, dir2d: Vec3) -> Option<CheckResult> {
        let ext = self.pawn.extent() * 0.25;
        let drop = Vec3::new(0.0, 0.0, ext.z * 0.75);
        let start = self.pawn.location + dir2d * 35.0 - drop;
        let end = start - self.pawn.floor * (self.pawn.max_wall_step_height + 2.0);
        let h = self.trace(end, start, ext);
        if h.time >= 1.0 { None } else { Some(h) }
    }

    fn wallrun_fall_off(&mut self, dt: f32, iterations: i32) {
        self.set_physics(Physics::Falling);
        self.falling_off_wall();
        self.start_new_physics(dt, iterations);
    }

    /// 0x12B3E50. Returns true when wall running ended (landed / fell).
    fn wallrun_slide(&mut self, rest: Vec3, hit: &mut CheckResult) -> bool {
        let into = -self.pawn.floor * self.pawn.max_wall_step_height;
        if hit.normal.z > 0.707 && self.pawn.physics == Physics::WallRunning {
            self.event_landed(hit.normal);
            self.set_physics(Physics::Walking);
            return true;
        }
        if hit.normal.z < -0.707 && self.pawn.physics == Physics::WallRunning {
            self.process_hit_wall(hit.normal);
            self.falling_off_wall();
            self.set_physics(Physics::Falling);
            return true;
        }
        if self.pawn.floor.dot(hit.normal) < 0.1 {
            self.move_actor(-into);
            *hit = self.move_actor(rest);
        }
        if hit.time < 1.0 {
            if hit.normal.dot(self.pawn.floor) < 0.4 {
                self.move_actor(into);
                self.falling_off_wall();
                self.set_physics(Physics::Falling);
                return true;
            }
            // the wall turns: carry the rest of the move into the new wall's frame
            let o = self.pawn.floor;
            let f = hit.normal;
            self.pawn.floor = f;
            hit.normal.z = 0.0;
            hit.normal = hit.normal.safe_normal();
            let a = f.cross(o).safe_normal();
            let b = a.cross(o).safe_normal();
            let c = a.cross(f);
            let new = c * rest.dot(b) + a * rest.dot(a) + f * rest.dot(o);
            if new.dot(rest) >= 0.0 {
                self.pawn.just_teleported = true;
                *hit = self.move_actor(new);
            }
        }
        let into = -self.pawn.floor * self.pawn.max_wall_step_height;
        *hit = self.move_actor(into);
        false
    }

    pub fn phys_wall_climbing(&mut self, dt: f32, mut iterations: i32) {
        if self.pawn.floor.is_nearly_zero() || self.pawn.velocity.z < 0.0 {
            self.falling_off_wall();
            if self.pawn.physics == Physics::WallClimbing {
                self.set_physics(Physics::Falling);
            }
            self.start_new_physics(dt, iterations);
            return;
        }
        let mut accel_dir = self.pawn.acceleration.safe_normal();
        let old_vel = self.pawn.velocity;
        let friction = self.moves.base(self.pawn.movement_state).friction_modifier;
        let gs4 = self.pawn.ground_speed * 4.0;
        self.calc_velocity(&mut accel_dir, dt, gs4, friction, false, false, false);
        self.pawn.just_teleported = false;
        let mut vel = self.pawn.velocity;
        let old_loc = self.pawn.location;
        let mut remaining = dt;
        while remaining > 0.0 && iterations < 8 {
            iterations += 1;
            let tick = if remaining <= 0.05 { remaining } else { (remaining * 0.5).min(0.05) };
            remaining -= tick;
            let delta = vel * tick;
            let mut hit = self.move_actor(delta);
            if hit.time < 1.0 {
                if self.pawn.is_wall_walking {
                    let rest = delta * (1.0 - hit.time);
                    if self.wallclimb_slide(rest, &mut hit) {
                        return;
                    }
                } else {
                    if hit.normal.z.abs() >= 0.2 {
                        self.wallclimb_fall_off(dt, iterations);
                        return;
                    }
                    // reached the wall: keep only the speed along it plus the vertical speed
                    let n = hit.normal;
                    let v = self.pawn.velocity;
                    let d = v.dot(n);
                    let along = Vec3::new(v.x - d * n.x, v.y - d * n.y, v.z - d * n.z);
                    let s = along.size_2d();
                    let dir = along.safe_normal_2d();
                    self.pawn.just_teleported = true;
                    self.pawn.is_wall_walking = true;
                    self.pawn.velocity = Vec3::new(dir.x * s, dir.y * s, dir.z * s + v.z);
                    vel = self.pawn.velocity;
                    self.reached_wall();
                }
            }
            if self.pawn.is_wall_walking {
                let into = -self.pawn.floor * (self.pawn.max_wall_step_height + 2.0);
                let start = self.pawn.location;
                let h = self.trace(start + into, start, self.pawn.extent());
                if h.time >= 1.0 {
                    self.wallclimb_fall_off(dt, iterations);
                    return;
                }
                if h.normal.z.abs() > 0.2 {
                    self.move_actor(into);
                    if self.pawn.physics == Physics::WallClimbing {
                        self.set_physics(Physics::Falling);
                    }
                    self.falling_off_wall();
                    self.start_new_physics(dt, iterations);
                    return;
                }
                self.move_actor(into);
                self.pawn.floor = h.normal;
            }
        }
        if !self.pawn.just_teleported && self.pawn.physics == Physics::WallClimbing {
            self.pawn.velocity = (self.pawn.location - old_loc) * (1.0 / dt);
            if old_vel.z > self.pawn.velocity.z || old_vel.z >= 0.0 {
                self.pawn.velocity = self.pawn.velocity * 2.0 - old_vel;
            }
        }
    }

    fn wallclimb_fall_off(&mut self, dt: f32, iterations: i32) {
        self.falling_off_wall();
        if self.pawn.physics == Physics::WallClimbing {
            self.set_physics(Physics::Falling);
        }
        self.start_new_physics(dt, iterations);
    }

    /// 0x12B4550. Returns true when the climb ended.
    fn wallclimb_slide(&mut self, rest: Vec3, hit: &mut CheckResult) -> bool {
        let away = self.pawn.floor * self.pawn.max_wall_step_height;
        if self.pawn.floor.dot(hit.normal) >= 0.95 {
            // same wall: nothing to do
            return false;
        }
        // step away from the wall and retry (the pawn stays off the wall; the probe after the
        // move pulls it back)
        self.move_actor(away);
        *hit = self.move_actor(rest);
        if hit.time >= 1.0 || self.pawn.floor.dot(hit.normal) >= 0.95 {
            return false;
        }
        // blocked again by something else: undo, push back to the wall, fall off
        self.move_actor(-rest * (1.0 - hit.time));
        self.move_actor(-away);
        if self.pawn.physics == Physics::WallClimbing {
            self.falling_off_wall();
            self.set_physics(Physics::Falling);
        }
        self.pawn.velocity.z = 0.0;
        true
    }
}
