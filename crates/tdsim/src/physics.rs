//! Pawn physics, transcribed from MirrorsEdge.exe:
//! - ATdPawn::CalcVelocity (vt260), GetWalkAcceleration (vt261), GetSprintAcceleration (vt262)
//! - ATdPawn::physWalking (vt127), ATdPawn::stepUp (vt132) + its corner-slip helper (0x12B0EC0)
//! - ATdPlayerPawn::physFalling (0x12C0980), APawn::physFlying (0xF00C30)
//! - ATdPawn::startNewPhysics / setPhysics, APawn::processLanded / processHitWall / TwoWallAdjust
//! - ULevel::MoveActor and SingleLineCheck semantics come from `collision`.

use crate::collision::CheckResult;
use crate::math::{Rotator, UeVec, Vec3};
use crate::pawn::{Move, Physics};
use crate::sim::Sim;

/// MAXSTEPHEIGHTFUDGE
const STEP_FUDGE: f32 = 2.0;
const MIN_FLOOR_DIST: f32 = 1.9;
const MAX_FLOOR_DIST: f32 = 2.4;
const FLOOR_DIST_TARGET: f32 = 2.15;

impl Sim {
    // ------------------------------------------------------------------ world access

    /// `GWorld->MoveActor(Pawn, Delta, Rotation, 0, Hit)`.
    pub fn move_actor(&mut self, delta: Vec3) -> CheckResult {
        let start = self.pawn.location;
        if delta.is_zero() {
            return CheckResult::none(start);
        }
        if !self.pawn.collide_world {
            self.pawn.location = start + delta;
            return CheckResult::none(self.pawn.location);
        }
        let mut hit = self.world.line_check(start + delta, start, self.pawn.extent());
        // other pawns block like the world (their collision cylinders)
        if let Some((t, n)) = self.pawn_cylinder_check(start, delta, self.pawn.extent()) {
            if t < hit.time {
                hit.time = t;
                hit.normal = n;
                hit.hit = true;
                hit.location = start + delta * t;
            }
        }
        self.pawn.location = start + delta * hit.time;
        hit
    }

    /// A swept box (`extent`) against the live bots' cylinders: the first contact (time, normal).
    pub fn pawn_cylinder_check(&self, start: Vec3, delta: Vec3, extent: Vec3) -> Option<(f32, Vec3)> {
        let mut best: Option<(f32, Vec3)> = None;
        for b in self.bots.iter().filter(|b| b.alive() && b.movement_state != crate::bots::BotMove::MeleeAirAbove) {
            let r = b.collision_radius + extent.x;
            let (px, py) = (start.x - b.location.x, start.y - b.location.y);
            let (dx, dy) = (delta.x, delta.y);
            let a = dx * dx + dy * dy;
            let c = px * px + py * py - r * r;
            if c <= 0.0 {
                // already touching: only block moving further in
                let n = Vec3::new(px, py, 0.0).safe_normal();
                if (start.z - b.location.z).abs() < b.collision_height + extent.z && Vec3::new(dx, dy, 0.0).dot(n) < 0.0 {
                    if best.is_none_or(|x| 0.0 < x.0) {
                        best = Some((0.0, n));
                    }
                }
                continue;
            }
            if a < 1e-8 {
                continue;
            }
            let bq = 2.0 * (px * dx + py * dy);
            let disc = bq * bq - 4.0 * a * c;
            if disc < 0.0 {
                continue;
            }
            let t = (-bq - disc.sqrt()) / (2.0 * a);
            if !(0.0..=1.0).contains(&t) {
                continue;
            }
            let at = start + delta * t;
            if (at.z - b.location.z).abs() >= b.collision_height + extent.z {
                continue;
            }
            let n = Vec3::new(at.x - b.location.x, at.y - b.location.y, 0.0).safe_normal();
            // pull back a little, like the world's hits
            let t = (t - 0.1 / delta.length().max(1e-3)).max(0.0);
            if best.is_none_or(|x| t < x.0) {
                best = Some((t, n));
            }
        }
        best
    }

    /// `SetLocation` / `FarMoveActor`: teleport if the spot is free.
    /// Actor.SetRotation (pawns have no rotating collision).
    pub fn set_rotation(&mut self, rot: Rotator) {
        self.pawn.rotation = rot;
    }

    pub fn set_location(&mut self, loc: Vec3) -> bool {
        if self.pawn.collide_world && self.world.point_check(loc, self.pawn.extent()) {
            return false;
        }
        self.pawn.location = loc;
        true
    }

    /// `SingleLineCheck(Hit, Pawn, End, Start, TRACE_World..., Extent)`.
    pub fn trace(&self, end: Vec3, start: Vec3, extent: Vec3) -> CheckResult {
        self.world.line_check(end, start, extent)
    }

    // ------------------------------------------------------------------ physics dispatch

    /// AActor::performPhysics for the pawn (TdPawn startNewPhysics with the full tick).
    pub fn perform_physics(&mut self, dt: f32) {
        self.start_new_physics(dt, 0);
    }

    /// ATdPawn::startNewPhysics (vt250).
    pub fn start_new_physics(&mut self, dt: f32, iterations: i32) {
        if !(dt >= 0.0003 && iterations <= 7) {
            return;
        }
        self.pawn.uncontrolled_slide = false;
        match self.pawn.physics {
            Physics::None => {}
            Physics::Walking => self.phys_walking(dt, iterations),
            Physics::Falling => self.phys_falling(dt, iterations),
            Physics::Flying => self.phys_flying(dt, iterations),
            Physics::WallRunning => self.phys_wall_running(dt, iterations),
            Physics::WallClimbing => self.phys_wall_climbing(dt, iterations),
            _ => self.set_physics(Physics::None),
        }
    }

    /// ATdPawn::setPhysics (vt120) over APawn::setPhysics.
    pub fn set_physics(&mut self, new: Physics) {
        let p = &mut self.pawn;
        let old = p.physics;
        if old != new && new == Physics::Walking {
            p.new_floor_smooth = p.location.z - p.collision_height - 2.0;
        }
        if old != new {
            if old == Physics::WallClimbing || old == Physics::WallRunning {
                p.is_wall_walking = false;
            }
            if new == Physics::Falling {
                p.enter_falling_height = p.location.z;
            }
        }
        // APawn::setPhysics
        if old != new {
            p.physics = new;
            if !matches!(new, Physics::None | Physics::Walking | Physics::Rotating | Physics::Spider) {
                p.base = false;
            }
            if matches!(new, Physics::None | Physics::Rotating) {
                p.velocity = Vec3::ZERO;
                p.acceleration = Vec3::ZERO;
            }
        }
    }

    // ------------------------------------------------------------------ velocity

    /// ATdPawn::CalcVelocity (vt260).
    #[allow(clippy::too_many_arguments)]
    pub fn calc_velocity(
        &mut self,
        accel_dir: &mut Vec3,
        dt: f32,
        max_speed: f32,
        friction: f32,
        fluid: bool,
        brake: bool,
        buoyant: bool,
    ) {
        // Root motion (Mesh.RootMotionMode == RMM_Accel): APawn::CalcVelocity's root motion
        // branch, then scaled by the move's RootMotionScale.
        if !self.pawn.force_regular_velocity && (self.pawn.force_rm_velocity || self.pawn.is_using_root_motion) {
            let delta = std::mem::take(&mut self.root_motion_delta);
            let p = &mut self.pawn;
            p.velocity = delta * (1.0 / dt);
            *accel_dir = p.velocity.safe_normal();
            p.acceleration = p.velocity * (1.0 / dt);
            p.rm_velocity = p.velocity;
            let s = self.moves.base(p.movement_state).root_motion_scale;
            p.velocity = Vec3::new(p.velocity.x * s.x, p.velocity.y * s.y, p.velocity.z * s.z);
            return;
        }

        let speed_mod = self.move_speed_modifier(self.pawn.movement_state);
        let mult = self.mobility_multiplier() * speed_mod;
        let p = &mut self.pawn;
        let max_accel = p.accel_rate * mult;
        let max_speed = mult * max_speed;

        if p.force_max_accel || p.force_max_accel_one_frame {
            p.force_max_accel_one_frame = false;
            if !p.acceleration.is_nearly_zero() {
                p.acceleration = *accel_dir * max_accel;
            } else if p.velocity.is_nearly_zero() {
                p.acceleration = p.rotation.vector() * max_accel;
                *accel_dir = p.acceleration.safe_normal();
            } else {
                p.acceleration = p.velocity.safe_normal() * max_accel;
                *accel_dir = p.acceleration.safe_normal();
            }
        }

        let precise = self.moves.base(p.movement_state).use_precise_location;
        let mut skip_friction = false;
        if !precise {
            if brake && p.acceleration.is_zero() {
                // Braking: integrate friction in 0.03 s substeps and keep the time-averaged velocity.
                let v0 = p.velocity;
                let mut avg = Vec3::ZERO;
                let mut remaining = dt;
                if dt > 0.0 {
                    loop {
                        let step = remaining.min(0.03);
                        remaining -= step;
                        p.velocity -= p.velocity * 2.0 * step * friction * p.braking_friction_strength;
                        if p.velocity.dot(v0) > 0.0 {
                            avg += p.velocity * step * (1.0 / dt);
                        }
                        if remaining <= 0.0 {
                            break;
                        }
                    }
                }
                p.velocity = avg;
                if p.velocity.dot(v0) < 0.0 || p.velocity.length_squared() < 100.0 {
                    p.velocity = Vec3::ZERO;
                }
                skip_friction = true;
            } else {
                if p.acceleration.length_squared() > max_accel * max_accel {
                    p.acceleration = p.acceleration.safe_normal() * max_accel;
                }
                if matches!(p.physics, Physics::WallRunning | Physics::WallClimbing) {
                    p.velocity.x -= p.velocity.x * dt * friction;
                    p.velocity.y -= p.velocity.y * dt * friction;
                } else {
                    p.velocity -= p.velocity * dt * friction * 0.1;
                }
            }
        }
        let _ = skip_friction;
        let fluid_f = if fluid { 1.0 } else { 0.0 };
        p.velocity = p.velocity * (1.0 - fluid_f * dt * friction) + p.acceleration * dt;
        if buoyant {
            // GravityZ * (1 - Buoyancy); the player's Buoyancy is 0.
            p.velocity.z += p.gravity_z() * dt;
        }
        if p.velocity.length_squared() > max_speed * max_speed {
            p.velocity = p.velocity.safe_normal() * max_speed;
        }
        p.rm_velocity = p.velocity;
    }

    /// ATdPawn::GetWalkAcceleration (vt261).
    pub fn get_walk_acceleration(&mut self, a_forward: f32, a_strafe: f32, _delta_rotation: i32, dt: f32) -> Vec3 {
        let p = &mut self.pawn;
        let (x, y, _) = p.rotation.axes();
        let dir = (x * a_forward + y * a_strafe).safe_normal();
        if dir.x.abs() < 1e-4 && dir.y.abs() < 1e-4 && dir.z.abs() < 1e-4 {
            p.speed_sprint_energy = 0.0;
            return Vec3::ZERO;
        }
        if p.speed_sprint_energy > 0.0 {
            let d = dir.dot(x).clamp(0.0, 0.9);
            let e = p.speed_sprint_energy
                - (1.0 - d.powf(p.speed_energy_deceleration_exponent)) * (p.ground_speed - p.speed_max_base_velocity)
                    / p.speed_energy_deceleration_time
                    * dt;
            p.speed_sprint_energy = e.max(0.0);
        }
        let range = p.speed_max_base_velocity - p.speed_min_base_velocity + p.speed_sprint_energy;
        let fwd_target = dir.dot(x) * p.speed_min_base_velocity + range * a_forward;
        let right_target = dir.dot(y) * p.speed_min_base_velocity + range * a_strafe;
        let v_fwd = p.velocity.dot(x);
        let v_right = p.velocity.dot(y);
        let mut a = (x * fwd_target - x * v_fwd) * p.speed_walk_velocity_acceleration_factor
            + (y * right_target - y * v_right) * p.speed_strafe_velocity_acceleration_factor;
        if p.physics != Physics::Falling {
            a += p.velocity * p.ground_friction * 0.1;
        }
        round_tenth(a)
    }

    /// ATdPawn::GetSprintAcceleration (vt262).
    pub fn get_sprint_acceleration(&mut self, a_forward: f32, a_strafe: f32, delta_rotation: i32, dt: f32) -> Vec3 {
        let p = &mut self.pawn;
        let (x, y, _) = p.rotation.axes();
        let speed = p.velocity.size_2d();
        let dir = (x * a_forward + y * a_strafe).safe_normal();
        if dir.x.abs() < 1e-4 && dir.y.abs() < 1e-4 && dir.z.abs() < 1e-4 {
            p.speed_sprint_energy = 0.0;
            return Vec3::ZERO;
        }
        p.speed_sprint_energy = (speed - p.speed_max_base_velocity).max(0.0);
        let curve = p.accel_curve_light_weapon.eval(speed, 0.0);
        let mut a = dir * curve;
        // a heavy gun: AccelCurve_HeavyWeapon, braking down to its last key's speed
        if self.weapon.as_ref().is_some_and(|w| w.class.heavy) {
            let max = p.accel_curve_heavy_weapon.points.last().map(|k| k.in_val).unwrap_or(400.0);
            a = if speed > max { -dir * (speed - max) } else { dir * p.accel_curve_heavy_weapon.eval(speed, 0.0) };
        }
        if p.physics != Physics::Falling {
            let diff = Vec3::new(dir.x * speed - p.velocity.x, dir.y * speed - p.velocity.y, dir.z * speed);
            let mag = diff.size_2d();
            let steer = (mag / dt).min(p.speed_sprint_velocity_acceleration_factor * mag);
            let diff_n = diff.safe_normal_2d();
            a = a + diff_n * steer + p.velocity * p.ground_friction * 0.1;
        }
        a -= p.velocity * p.speed_turn_deceleration_factor * (delta_rotation as f32).abs() * (1.0 / 32768.0);
        round_tenth(a)
    }

    // ------------------------------------------------------------------ walking

    /// ATdPawn::physWalking (vt127).
    pub fn phys_walking(&mut self, dt: f32, mut iterations: i32) {
        // Slope-dependent friction.
        let ms = self.pawn.movement_state;
        let move_friction = self.moves.base(ms).friction_modifier;
        let friction_mod = {
            let p = &self.pawn;
            let vel_dir = p.velocity.safe_normal_2d();
            let floor_dir = p.floor.safe_normal_2d();
            let uphill = (-floor_dir).dot(vel_dir);
            let fz2 = (p.floor.z * p.floor.z).clamp(0.0, 1.0);
            let s = (1.0 - fz2).sqrt() * uphill;
            if matches!(ms, Move::Slide | Move::RumpSlide | Move::MeleeSlide) {
                let scale = if uphill < 0.0 { p.downward_slide_friction_scale } else { p.upward_slide_friction_scale };
                (scale * s + 1.0) * move_friction
            } else {
                let scale = if uphill < 0.0 { p.downward_walk_friction_scale } else { p.upward_walk_friction_scale };
                let f = ((scale * s + 1.0) * move_friction).max(p.min_walk_friction_modify);
                f.min(p.max_walk_friction_modify)
            }
        };

        self.pawn.velocity.z = 0.0;
        self.pawn.acceleration.z = 0.0;
        let mut accel_dir =
            if self.pawn.acceleration.is_zero() { self.pawn.acceleration } else { self.pawn.acceleration.safe_normal() };
        let gs = self.pawn.ground_speed;
        let gf = self.pawn.ground_friction * friction_mod;
        self.calc_velocity(&mut accel_dir, dt, gs, gf, false, true, false);

        let desired_move = Vec3::new(self.pawn.velocity.x, self.pawn.velocity.y, 0.0);
        let grav_dir = Vec3::new(0.0, 0.0, -1.0);
        let down = grav_dir * (self.pawn.max_step_height + STEP_FUDGE);
        let old_location = self.pawn.location;
        let old_floor = self.pawn.floor;
        let old_base = self.pawn.base;
        let old_z = self.pawn.location.z;
        self.pawn.just_teleported = false;
        let mut checked_fall = false;
        let mut remaining = dt;
        let mut floor_dist = 0.0f32;
        let mut floor_hit = CheckResult::none(self.pawn.location);
        let mut time_tick = 0.0;
        let mut delta = Vec3::ZERO;
        let mut sub_loc = self.pawn.location;

        let mut fell = false;
        while remaining > 0.0 && iterations < 8 {
            iterations += 1;
            time_tick = if remaining > 0.05 { (remaining * 0.5).min(0.05) } else { remaining };
            remaining -= time_tick;
            delta = desired_move * time_tick;
            sub_loc = self.pawn.location;
            let zero_delta = delta.is_nearly_zero();
            let mut wall_hit: Option<Vec3> = None;

            if zero_delta {
                remaining = 0.0;
            } else {
                // Player ledge avoidance (Controller->WantsLedgeCheck -> ATdPlayerPawn::CheckForLedges).
                if self.wants_ledge_check() {
                    let (d, must_stop) = self.check_for_ledges(accel_dir, delta, grav_dir, &mut checked_fall);
                    delta = d;
                    if must_stop {
                        remaining = 0.0;
                    }
                }
                let mut hit = if self.pawn.floor.z < 0.98 && delta.dot(self.pawn.floor) < 0.0 {
                    // moving up a slope: let stepUp handle it
                    CheckResult { time: 0.0, ..CheckResult::none(self.pawn.location) }
                } else {
                    self.move_actor(delta)
                };
                if hit.time < 1.0 {
                    let desired_dir = delta * (1.0 / delta.length());
                    let rest = delta * (1.0 - hit.time);
                    self.step_up(grav_dir, desired_dir, rest, &mut hit);
                    if self.pawn.physics == Physics::Falling {
                        let desired_dist = delta.length();
                        let actual = (self.pawn.location - sub_loc).size_2d();
                        remaining += time_tick * (1.0 - (actual / desired_dist).min(1.0));
                        // eventFalling(): Pawn.Falling is empty for the player
                        self.start_new_physics(remaining, iterations);
                        return;
                    }
                    if hit.time < 1.0 && hit.hit {
                        wall_hit = Some(hit.normal);
                    }
                }
            }

            // ---- drop to floor
            let mut found: CheckResult;
            if let Some(n) = wall_hit {
                // stepUp ended against something: treat it as the floor hit at MAXFLOORDIST
                found = CheckResult { time: 0.1, normal: n, hit: true, ..CheckResult::none(self.pawn.location) };
                floor_dist = MAX_FLOOR_DIST;
            } else if zero_delta && self.pawn.base && !self.pawn.force_floor_check {
                found = CheckResult { time: 0.1, normal: self.pawn.floor, hit: true, ..CheckResult::none(self.pawn.location) };
                floor_dist = MAX_FLOOR_DIST;
            } else {
                self.pawn.force_floor_check = false;
                let col = self.pawn.location;
                found = self.trace(col + down, col, self.pawn.extent());
                floor_dist = (self.pawn.max_step_height + STEP_FUDGE) * found.time;
                if found.hit {
                    self.pawn.floor = found.normal;
                }
            }

            let mut slid_down = false;
            if !(found.hit && found.normal.z >= self.pawn.walkable_floor_z) && found.hit && !delta.is_nearly_zero()
                && found.normal.dot(delta) < 0.0
            {
                // Moving into an unwalkable slope: slide down along it by MaxStepHeight.
                let up = Vec3::new(0.0, 0.0, self.pawn.max_step_height);
                let n = found.normal;
                let d = n.dot(up);
                let slide = -(up - n * d);
                let h = self.move_actor(slide);
                if h.hit && h.time < 1.0 {
                    self.pawn.base = true;
                }
                self.pawn.floor = found.normal;
                slid_down = true;
            }
            if !slid_down {
                let same_base = found.hit && self.pawn.base;
                if !found.hit || (same_base && floor_dist <= MAX_FLOOR_DIST) {
                    if floor_dist < MIN_FLOOR_DIST {
                        let n = found.normal;
                        self.move_actor(Vec3::new(0.0, 0.0, FLOOR_DIST_TARGET - floor_dist));
                        found.time = 0.0;
                        found.normal = n;
                    }
                } else {
                    let n = found.normal;
                    let amount = (floor_dist - FLOOR_DIST_TARGET).max(0.0);
                    self.move_actor(down.safe_normal() * amount);
                    found.time = 0.0;
                    found.normal = n;
                    if found.hit && self.pawn.physics == Physics::Walking {
                        self.pawn.base = true;
                    }
                }
            }

            let fz = found.normal.z;
            if !found.hit || fz < self.pawn.walkable_floor_z {
                fell = true;
                break;
            }
            self.pawn.base = true;
            if fz < 0.99 && self.pawn.ground_friction * fz < 3.3 {
                // Low friction: slide down the slope under gravity.
                let gf = self.pawn.ground_friction.max(0.5);
                let g = self.pawn.gravity_z() * dt / (gf + gf) * dt;
                let n = found.normal;
                let d = n.z * g;
                let slide = Vec3::new(-(n.x * d), -(n.y * d), g - d * n.z);
                if slide.z * g >= 0.0 {
                    self.move_actor(slide);
                }
            }
            if remaining <= 0.0 {
                break;
            }
        }

        if fell {
            // No floor under us: fall, or stop at the ledge if we can't walk off it.
            let p = &self.pawn;
            let may_fall = checked_fall
                || p.just_teleported
                || (p.can_jump && (p.can_walk_off_ledges || !(p.is_walking || p.is_crouched)));
            if may_fall {
                let desired_dist = delta.length();
                if desired_dist != 0.0 {
                    let actual = (self.pawn.location - sub_loc).size_2d();
                    remaining += time_tick * (1.0 - (actual / desired_dist).min(1.0));
                } else {
                    remaining = 0.0;
                }
                self.pawn.velocity.z = 0.0;
                // eventFalling() is empty; then the player's ledge drop handling:
                if self.pawn.physics == Physics::Walking {
                    self.set_physics(Physics::Falling);
                    let small_drop = floor_dist <= 36.0 && self.pawn.walkable_floor_z <= self.pawn.floor.z && found_hit_z(&self.pawn);
                    if !(small_drop || self.is_in_move(Move::StumbleFalling)) {
                        self.set_move(Move::Falling, false, false);
                    }
                }
                self.start_new_physics(remaining, iterations);
                return;
            }
            // Can't fall: go back to where this tick started.
            self.pawn.velocity = Vec3::ZERO;
            self.pawn.acceleration = Vec3::ZERO;
            self.set_location(old_location);
            self.pawn.floor = old_floor;
            self.pawn.base = old_base;
            return;
        }

        if self.pawn.physics == Physics::Walking {
            let dz = (self.pawn.location.z - old_z).abs();
            if dz > 4.0 && self.pawn.max_step_height >= dz {
                // vt266: smooth the 1p mesh over the step
                let off = old_z - self.pawn.location.z;
                let t = self.pawn.target_mesh_translation_z;
                self.pawn.mesh_translation_z = (self.pawn.mesh_translation_z + off).clamp(t - 24.0, t + 24.0);
            }
            if !self.pawn.just_teleported {
                self.pawn.velocity = (self.pawn.location - old_location) * (1.0 / dt);
            }
            self.pawn.velocity.z = 0.0;
        }
        let _ = floor_hit;
        floor_hit = CheckResult::none(self.pawn.location);
        let _ = floor_hit;
    }

    // ------------------------------------------------------------------ stepping

    /// ATdPawn::stepUp (vt132).
    pub fn step_up(&mut self, grav_dir: Vec3, desired_dir: Vec3, delta: Vec3, hit: &mut CheckResult) {
        let down = grav_dir * (self.pawn.max_step_height + STEP_FUDGE);
        let mut step_down = true;
        let mut try_slip = true;
        let hn = hit.normal;
        if -(hn.dot(grav_dir)) >= 0.08 && hn.z < self.pawn.walkable_floor_z {
            // Steep but not vertical.
            if self.pawn.physics != Physics::Walking {
                let len = delta.length();
                let d = Vec3::new(delta.x, delta.y, delta.z + len * hn.z);
                *hit = self.move_actor(d);
                step_down = false;
            }
        } else {
            // Step up: lift, move, check the floor where we end up.
            let saved = *hit;
            self.move_actor(-down + hn * 0.1);
            let before = self.pawn.location;
            *hit = self.move_actor(delta);
            if hit.time >= 1.0 || hit.normal.z < self.pawn.walkable_floor_z {
                let col = self.pawn.location;
                let f = self.trace(col + down, col, self.pawn.extent());
                if f.time < 1.0 && f.hit && f.normal.z < self.pawn.walkable_floor_z {
                    // would land on an unwalkable surface: undo the step
                    *hit = saved;
                    let back = before - self.pawn.location;
                    let _ = before;
                    self.move_actor(Vec3::new(back.x, back.y, back.z));
                    try_slip = false;
                }
            }
        }

        if hit.time < 1.0 {
            let into = -(hit.normal.dot(desired_dir));
            if into < 0.08 && delta.length_squared() * hit.time > 144.0 {
                // glancing hit and still far to go: step down and recurse
                if step_down {
                    *hit = self.move_actor(down);
                }
                let rest = delta * (1.0 - hit.time);
                self.step_up(grav_dir, desired_dir, rest, hit);
                return;
            }
            let mut slipped = false;
            if try_slip && hit.normal.z < self.pawn.walkable_floor_z && self.is_player_pawn() {
                // slide around the obstacle sideways (corner slip)
                let side = (delta - hit.normal * delta.dot(hit.normal)).safe_normal_2d();
                let use_custom = self.moves.base(self.pawn.movement_state).use_custom_collision;
                let mut d = delta;
                slipped = self.corner_slip(side, use_custom, false, &mut d, hit);
            }
            if !slipped {
                self.process_hit_wall(hit.normal);
                if self.pawn.physics == Physics::Falling {
                    return;
                }
                // adjust and try again
                let mut n = hit.normal;
                n.z = 0.0;
                let n = n.safe_normal();
                hit.normal = n;
                let original = delta;
                let mut adj = (delta - n * delta.dot(n)) * (1.0 - hit.time);
                let adj_n = adj.safe_normal();
                if adj_n.dot(original.safe_normal()) > 0.707 {
                    adj = adj_n * original.length() * (1.0 - hit.time);
                }
                if adj.dot(original) >= 0.0 {
                    let old_normal = n;
                    *hit = self.move_actor(adj);
                    if try_slip && hit.time < 1.0 {
                        let hn2 = hit.normal.safe_normal_2d();
                        let ad = adj.safe_normal();
                        if hn2.dot(ad) < -0.707 && self.is_player_pawn() {
                            let side = -(adj - hit.normal * adj.dot(hit.normal)).safe_normal_2d();
                            let mut d = adj;
                            if self.corner_slip(side, false, true, &mut d, hit) {
                                if step_down {
                                    *hit = self.move_actor(down);
                                }
                                return;
                            }
                        }
                        self.process_hit_wall(hit.normal);
                        if self.pawn.physics == Physics::Falling {
                            return;
                        }
                        let mut d = adj;
                        let mut hn = hit.normal;
                        let mut on = old_normal;
                        two_wall_adjust(desired_dir, &mut d, &mut hn, &mut on, hit.time);
                        *hit = self.move_actor(d);
                    }
                }
            }
        }
        // the final step down writes the caller's hit: physWalking uses it as the floor
        if step_down {
            *hit = self.move_actor(down);
        }
    }

    /// ATdPawn helper 0x12B0EC0: when blocked head-on, sidestep 32 uu, try the move, step back.
    /// Returns true when the pawn got past (the forward move completed).
    fn corner_slip(&mut self, side: Vec3, use_custom_collision: bool, check_ahead: bool, delta: &mut Vec3, hit: &mut CheckResult) -> bool {
        let saved = *hit;
        let start = self.pawn.location;
        if side.is_zero() {
            return false;
        }
        let dn = delta.safe_normal();
        if -(saved.normal.dot(dn)) < 0.707 {
            return false;
        }
        let side_hit = self.move_actor(side * 32.0);
        let mut ok = false;
        if side_hit.time >= 1.0 || side_hit.time < 0.0 {
            let mut blocked_ahead = false;
            if check_ahead {
                let d2 = delta.safe_normal_2d();
                let l = self.pawn.location;
                let ahead = self.trace(l + d2 * 34.0, l, self.pawn.extent());
                blocked_ahead = ahead.time < 1.0;
            }
            if !blocked_ahead {
                let down = Vec3::new(0.0, 0.0, -self.pawn.max_step_height);
                *delta *= 1.0 - saved.time;
                let mut h = self.move_actor(*delta);
                let mut t = h.time;
                let mut down_t = 0.0;
                let mut undo_down = false;
                if h.time <= 0.05 && use_custom_collision {
                    h = self.move_actor(down);
                    down_t = h.time;
                    if down_t > 0.05 {
                        h = self.move_actor(*delta);
                        t = h.time;
                        undo_down = t <= 0.05;
                    }
                }
                ok = t >= 1.0;
                if undo_down {
                    self.move_actor(-down * down_t);
                }
            }
            self.move_actor(-side * 32.0);
        } else {
            self.move_actor(-side * 32.0 * side_hit.time);
        }
        let moved = (self.pawn.location - start).size_sq_2d();
        let jumped = moved > delta.size_sq_2d();
        self.pawn.just_teleported = jumped;
        if jumped {
            // OffsetMeshXY(old - new, true): hide the jump visually
            let d = start - self.pawn.location;
            self.offset_mesh_xy(d, true);
        }
        *hit = saved;
        ok
    }

    // ------------------------------------------------------------------ falling

    /// ATdPlayerPawn::physFalling (0x12C0980).
    pub fn phys_falling(&mut self, dt: f32, mut iterations: i32) {
        let root_motion = !self.pawn.force_regular_velocity && self.pawn.is_using_root_motion;
        if root_motion {
            if !self.pawn.force_rm_velocity {
                let d = std::mem::take(&mut self.root_motion_delta);
                self.pawn.velocity.x = d.x * (1.0 / dt);
                self.pawn.velocity.y = d.y * (1.0 / dt);
                self.pawn.acceleration = Vec3::ZERO;
                self.pawn.rm_velocity = self.pawn.velocity;
            } else {
                self.pawn.velocity = self.pawn.rm_velocity;
            }
        }
        let real_accel = self.pawn.acceleration;
        self.pawn.acceleration.z = 0.0;
        let mut bound_speed = 0.0f32;
        if !root_motion {
            let p = &self.pawn;
            let mut tick_air = p.air_control;
            if tick_air > 0.05 {
                let tw = (p.acceleration.safe_normal() * (tick_air * p.accel_rate) + p.velocity) * dt;
                if tw.x != 0.0 || tw.y != 0.0 {
                    let col = p.location;
                    let h = self.trace(col + Vec3::new(tw.x, tw.y, 0.0), col, p.extent());
                    if h.hit {
                        tick_air = 0.0;
                    }
                }
            }
            let p = &mut self.pawn;
            let mut max_accel = p.accel_rate * tick_air;
            let vel2d = p.velocity.size_2d();
            if vel2d < 10.0 && tick_air > 0.0 {
                max_accel += (10.0 - vel2d) / dt;
            } else if vel2d >= p.ground_speed {
                if tick_air <= 0.05 {
                    max_accel = 1.0;
                } else {
                    bound_speed = vel2d;
                }
            }
            if p.acceleration.length_squared() > max_accel * max_accel {
                p.acceleration = p.acceleration.safe_normal() * max_accel;
            }
        }

        let mut remaining = dt;
        while remaining > 0.0 && iterations < 8 {
            iterations += 1;
            let time_tick = if remaining > 0.05 { (remaining * 0.5).min(0.05) } else { remaining };
            remaining -= time_tick;
            let old_location = self.pawn.location;
            self.pawn.just_teleported = false;
            let mut old_velocity = self.pawn.velocity;
            // NewFallVelocity: Vel*(1 - FluidFriction*dt) + (Accel + Gravity)*(1 - Buoyancy)*dt with
            // the player's (0, 0) buoyancy/friction outside water.
            let g = self.pawn.gravity_z();
            let a = self.pawn.acceleration + Vec3::new(0.0, 0.0, g);
            self.pawn.velocity = old_velocity + a * time_tick;
            if !root_motion && bound_speed != 0.0 {
                let v2 = self.pawn.velocity.size_sq_2d();
                if v2 > bound_speed * bound_speed {
                    let n = Vec3::new(self.pawn.velocity.x, self.pawn.velocity.y, 0.0).safe_normal();
                    self.pawn.velocity.x = n.x * bound_speed;
                    self.pawn.velocity.y = n.y * bound_speed;
                }
            }
            let mut adjusted = self.pawn.velocity * time_tick;

            let mut retries = 0;
            let mut landed_or_done = false;
            loop {
                let mut retry = false;
                let mut hit = self.move_actor(adjusted);
                if self.pawn.physics != Physics::Falling {
                    return;
                }
                if hit.time < 1.0 {
                    if hit.normal.z >= self.pawn.walkable_floor_z {
                        // Landing on a walkable surface.
                        if self.landing_needs_slide_off(&hit) && adjusted.z != 0.0 {
                            if !self.check_valid_floor(adjusted, hit.normal, true) {
                                self.pawn.just_teleported = true;
                                adjusted *= 1.0 - hit.time;
                                retry = true;
                            }
                        } else if adjusted.z != 0.0 {
                            self.check_valid_floor(adjusted, hit.normal, false);
                        }
                        if !retry {
                            remaining += (1.0 - hit.time) * time_tick;
                            if !self.pawn.just_teleported && hit.time > 0.1 && hit.time * time_tick > 0.003 {
                                self.pawn.velocity = (self.pawn.location - old_location) * (1.0 / (hit.time * time_tick));
                            }
                            self.process_landed(hit.normal, remaining, iterations);
                            return;
                        }
                    } else {
                        // Hit a wall or ceiling.
                        self.process_hit_wall(hit.normal);
                        if self.pawn.physics != Physics::Falling {
                            return;
                        }
                        if -self.pawn.walkable_floor_z >= hit.normal.z && self.pawn.velocity.z > 0.0 {
                            self.ceiling_corner_push();
                        }
                        let old_hit_normal = hit.normal;
                        let mut slide = self.compute_slide_vector(adjusted, &hit);
                        // LABEL_171 when the slide is done: the "old" velocity becomes the actual XY
                        // average so the refine below doesn't reflect off the wall.
                        let mut reset_old_xy = true;
                        if slide.dot(adjusted) >= 0.0 {
                            let target = self.pawn.location + slide;
                            hit = self.move_actor(slide);
                            let mut lifted = false;
                            let slide_n = slide.safe_normal();
                            let mut to_131 = true;
                            if self.pawn.walkable_floor_z - 1.0 > slide_n.z {
                                if hit.time < 1.0 {
                                    // steep downward slide blocked: retry to the slide target 1.9 higher
                                    let d = target + Vec3::new(0.0, 0.0, 1.9) - self.pawn.location;
                                    hit = self.move_actor(d);
                                    lifted = true;
                                } else {
                                    to_131 = false;
                                }
                            }
                            if to_131 && hit.time < 1.0 {
                                if hit.normal.z < self.pawn.walkable_floor_z {
                                    self.process_hit_wall(hit.normal);
                                    if self.pawn.physics != Physics::Falling {
                                        return;
                                    }
                                    let dir = adjusted.safe_normal();
                                    let mut hn = hit.normal;
                                    let mut on = old_hit_normal;
                                    let ditch = on.z > 0.0 && hn.z > 0.0 && slide.z == 0.0 && hn.dot(on) < 0.0;
                                    two_wall_adjust(dir, &mut slide, &mut hn, &mut on, hit.time);
                                    hit = self.move_actor(slide);
                                    if ditch || hit.normal.z >= self.pawn.walkable_floor_z {
                                        self.process_landed(hit.normal, 0.0, iterations);
                                        return;
                                    }
                                } else {
                                    // walkable: the same CheckValidFloor landing test as a direct hit
                                    if self.landing_needs_slide_off(&hit) && adjusted.z != 0.0 {
                                        if !self.check_valid_floor(adjusted, hit.normal, true) {
                                            self.pawn.just_teleported = true;
                                            adjusted *= 1.0 - hit.time;
                                            retry = true;
                                        }
                                    } else if adjusted.z != 0.0 {
                                        self.check_valid_floor(adjusted, hit.normal, false);
                                    }
                                    if !retry {
                                        self.process_landed(hit.normal, 0.0, iterations);
                                        return;
                                    }
                                }
                            } else if to_131 && lifted {
                                self.move_actor(Vec3::new(0.0, 0.0, -1.9));
                            }
                        }
                        if reset_old_xy {
                            let avg = (self.pawn.location - old_location) * (1.0 / time_tick);
                            old_velocity.x = avg.x;
                            old_velocity.y = avg.y;
                            reset_old_xy = false;
                        }
                        let _ = reset_old_xy;
                    }
                }
                retries += 1;
                if !(retry && retries < 2) {
                    landed_or_done = true;
                    break;
                }
            }
            let _ = landed_or_done;

            if !root_motion && !self.pawn.just_teleported && self.pawn.physics != Physics::None {
                // refine: average actual velocity, then the end velocity "has 2x accel of the average"
                self.pawn.velocity = (self.pawn.location - old_location) * (1.0 / time_tick);
                if old_velocity.z > self.pawn.velocity.z || old_velocity.z >= 0.0 {
                    self.pawn.velocity = self.pawn.velocity * 2.0 - old_velocity;
                }
                let tv = self.pawn.terminal_velocity;
                if self.pawn.velocity.length_squared() > tv * tv {
                    self.pawn.velocity = self.pawn.velocity.safe_normal() * tv;
                }
            }
            if remaining <= 0.0 {
                break;
            }
        }
        self.pawn.acceleration = real_accel;
    }

    /// TdPawn vt254 -> APawn::ComputeSlideVector: slide along the hit surface.
    fn compute_slide_vector(&self, delta: Vec3, hit: &CheckResult) -> Vec3 {
        let n = hit.normal;
        (delta - n * delta.dot(n)) * (1.0 - hit.time)
    }

    /// Landing where FindLedge-style moves aren't allowed (high falls, 180 in air, surfaces that
    /// exclude both hand and foot moves): the player slides off edges instead of standing on them.
    fn landing_needs_slide_off(&self, hit: &CheckResult) -> bool {
        let ms = self.pawn.movement_state;
        if ms == Move::FallingUncontrolled {
            return false;
        }
        let hard = self.moves.landing.hard_landing_height;
        self.pawn.enter_falling_height - self.pawn.location.z > hard
            || (hit.surface.exclude_hand_moves && hit.surface.exclude_foot_moves)
            || ms == Move::Turn180InAir
    }


    // ------------------------------------------------------------------ flying

    /// APawn::physFlying (0xF00C30).
    pub fn phys_flying(&mut self, dt: f32, _iterations: i32) {
        let mut accel_dir =
            if self.pawn.acceleration.is_zero() { self.pawn.acceleration } else { self.pawn.acceleration.safe_normal() };
        let air = self.pawn.air_speed;
        let ff = self.pawn.fluid_friction * 0.5;
        self.calc_velocity(&mut accel_dir, dt, air, ff, true, false, false);
        let old = self.pawn.location;
        self.pawn.just_teleported = false;
        let adjusted = self.pawn.velocity * dt;
        let mut hit = self.move_actor(adjusted);
        let mut old_z = old.z;
        if hit.time >= 1.0 {
            self.pawn.floor = Vec3::new(0.0, 0.0, 1.0);
        } else {
            self.pawn.floor = hit.normal;
            let grav = Vec3::new(0.0, 0.0, -1.0);
            let vel_dir = self.pawn.velocity.safe_normal();
            let updown = grav.dot(vel_dir);
            let adj_dir = adjusted.safe_normal();
            if hit.normal.z.abs() < 0.2 && updown < 0.5 && updown > -0.2 {
                let step_z = self.pawn.location.z;
                self.step_up(grav, adj_dir, adjusted * (1.0 - hit.time), &mut hit);
                old_z += self.pawn.location.z - step_z;
            } else {
                self.process_hit_wall(hit.normal);
                let n = hit.normal;
                let mut delta = (adjusted - n * adjusted.dot(n)) * (1.0 - hit.time);
                if delta.dot(adjusted) >= 0.0 {
                    hit = self.move_actor(delta);
                    if hit.time < 1.0 {
                        self.process_hit_wall(hit.normal);
                        let mut hn = hit.normal;
                        let mut on = n;
                        two_wall_adjust(adj_dir, &mut delta, &mut hn, &mut on, hit.time);
                        self.move_actor(delta);
                    }
                }
            }
        }
        if !self.pawn.just_teleported {
            let l = self.pawn.location;
            self.pawn.velocity = Vec3::new(l.x - old.x, l.y - old.y, l.z - old_z) * (1.0 / dt);
        }
    }

    // ------------------------------------------------------------------ events

    /// APawn::processHitWall for the player: the controller doesn't consume it, so it reaches
    /// TdPawn.HitWall -> Moves[MovementState].HitWall.
    pub fn process_hit_wall(&mut self, normal: Vec3) {
        self.move_hit_wall(normal);
    }

    /// APawn::processLanded (vt125).
    pub fn process_landed(&mut self, normal: Vec3, remaining: f32, iterations: i32) {
        self.pawn.floor = normal;
        // TdPlayerController.NotifyLanded (PlayerWalking) returns false, so eventLanded runs:
        self.event_landed(normal);
        if self.pawn.physics == Physics::Falling {
            // SetPostLandedPhysics
            self.set_physics(Physics::Walking);
        }
        if self.pawn.physics == Physics::Walking {
            self.pawn.acceleration = self.pawn.acceleration.safe_normal();
            self.pawn.base = true;
        }
        self.start_new_physics(remaining, iterations);
    }

    /// TdPlayerPawn.Landed.
    pub fn event_landed(&mut self, normal: Vec3) {
        if self.pawn.uncontrolled_fall {
            // TdPlayerPawn state UncontrolledFall.Landed
            if self.cfg_take_fall_damage || self.pawn.movement_state != Move::SoftLanding {
                self.take_falling_damage();
            }
            if self.can_do_move(Move::Landing) {
                self.set_move(Move::Landing, false, false);
            }
            if self.health > 0 {
                // GotoState('') -> EndState: FallingSound.FadeOut(0.1)
                self.pawn.uncontrolled_fall = false;
                self.sound(crate::sound::SoundEvent::LoopStop { slot: crate::sound::LoopSlot::Falling, fade_out: 0.1 });
            } else {
                self.sound(crate::sound::SoundEvent::Cue(crate::sound::DEATH_IMPACT_SOUND.into()));
            }
            return;
        }
        if self.cfg_take_fall_damage {
            self.take_falling_damage();
        }
        self.move_landed(normal);
        self.pawn.enter_falling_height = self.pawn.location.z;
    }

    pub fn is_player_pawn(&self) -> bool {
        true
    }

    /// Unused rotator helper kept for parity with UE's FaceRotation math.
    pub fn rot_delta(a: Rotator, b: Rotator) -> Rotator {
        (a - b).normalize()
    }
}

fn found_hit_z(_p: &crate::pawn::Pawn) -> bool {
    true
}

/// `Round(x * 10) / 10` per component, as the acceleration natives do.
fn round_tenth(v: Vec3) -> Vec3 {
    Vec3::new((v.x * 10.0 + 0.5).floor() * 0.1, (v.y * 10.0 + 0.5).floor() * 0.1, (v.z * 10.0 + 0.5).floor() * 0.1)
}

/// AActor::TwoWallAdjust.
pub fn two_wall_adjust(desired_dir: Vec3, delta: &mut Vec3, hit_normal: &mut Vec3, old_hit_normal: &mut Vec3, hit_time: f32) {
    if old_hit_normal.dot(*hit_normal) > 0.0 {
        // adjust to new wall
        *delta = (*delta - *hit_normal * delta.dot(*hit_normal)) * (1.0 - hit_time);
        if desired_dir.dot(*delta) <= 0.0 {
            *delta = Vec3::ZERO;
        }
    } else {
        // 90 degrees or tighter corner: move along the crease
        let dir = hit_normal.cross(*old_hit_normal).safe_normal();
        *delta = dir * (delta.dot(dir) * (1.0 - hit_time));
        if desired_dir.dot(*delta) < 0.0 {
            *delta = -*delta;
        }
    }
}
