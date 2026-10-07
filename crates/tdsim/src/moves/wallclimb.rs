//! TdMove_WallClimb (script) + UTdMove_WallClimb natives: CanDoMove (vt74), the per-tick update
//! (vt70), DetectPossibleHandPlant (vt75) and CheckDoubleJump (vt76).

use crate::config::Config;
use crate::math::{Rotator, UeVec, Vec3};
use crate::pawn::{Move, MoveActionHint, Physics, Slot};
use crate::sim::Sim;

pub struct WallClimb {
    pub velocity_start_limit: f32,
    pub vertical_start_angle: f32,
    pub vertical_friction: f32,
    pub max_distance_2d: f32,
    pub add_on_speed_2d_height: f32,
    pub add_on_speed_2d_max_limit: f32,
    pub add_on_speed_z_height: f32,
    pub add_on_speed_z_max_limit: f32,
    pub gravity: f32,
    pub default_gravity: f32,
    pub min_ledge_z_normal: f32,
    pub min_wall_height: f32,
    pub min_upwards_velocity_to_double_jump: f32,
    pub has_reached_wall: bool,
    pub found_possible_hand_plant: bool,
    pub performed_double_jump: bool,
    pub look_at_edge_angle: Rotator,
    pub possible_edge_destination: Vec3,
    pub into_wall_climb_speed: f32,
    pub ground_z_loc: f32,
}

impl WallClimb {
    pub fn new(cfg: &Config) -> Self {
        let c = &["TdMove_WallClimb"];
        let g = cfg.f32(c, "WallClimbingGravity", 800.0);
        WallClimb {
            velocity_start_limit: cfg.f32(c, "WallClimbingVelocityStartLimit", 0.0),
            vertical_start_angle: cfg.f32(c, "WallClimbingVerticalStartAngle", 33.0),
            vertical_friction: cfg.f32(c, "WallClimbingVerticalFriction", 6.0),
            max_distance_2d: cfg.f32(c, "WallClimbingMaxDistance2D", 120.0),
            add_on_speed_2d_height: cfg.f32(c, "AddOnSpeed2DHeight", 60.0),
            add_on_speed_2d_max_limit: cfg.f32(c, "AddOnSpeed2DMaxLimit", 650.0),
            add_on_speed_z_height: cfg.f32(c, "AddOnSpeedZHeight", 130.0),
            add_on_speed_z_max_limit: cfg.f32(c, "AddOnSpeedZMaxLimit", 320.0),
            gravity: g,
            default_gravity: g,
            min_ledge_z_normal: cfg.f32(c, "MinLegdeZNormal", 0.707),
            min_wall_height: cfg.f32(c, "MinWallHeight", 180.0),
            min_upwards_velocity_to_double_jump: cfg.f32(c, "MinUpwardsVelocityToDoubleJump", 100.0),
            has_reached_wall: false,
            found_possible_hand_plant: false,
            performed_double_jump: false,
            look_at_edge_angle: Rotator::ZERO,
            possible_edge_destination: Vec3::ZERO,
            into_wall_climb_speed: 0.0,
            ground_z_loc: 0.0,
        }
    }
}

impl Sim {
    /// UTdMove_WallClimb vt74 (native CanDoMove).
    pub fn wallclimb_can_do_move(&mut self, m: Move) -> bool {
        if !self.tdmove_can_do_move(m) {
            return false;
        }
        let p = &self.pawn;
        let w = &self.moves.wallclimb;
        if p.move_action_hint != MoveActionHint::Up {
            return false;
        }
        let d = Vec3::new(p.location.x - p.move_ledge_location.x, p.location.y - p.move_ledge_location.y, 0.0);
        if d.length_squared() > w.max_distance_2d * w.max_distance_2d {
            return false;
        }
        if p.move_ledge_result == 2 {
            if w.min_wall_height > (p.move_ledge_location.z - p.location.z) + p.collision_height {
                return false;
            }
            // something solid to put the feet on just below the head
            let fwd = p.rotation.vector().safe_normal_2d();
            let mut start = p.location;
            start.z += p.collision_height - 64.0;
            let mn = p.move_normal.safe_normal_2d();
            let m2 = mn.x.abs().max(mn.y.abs());
            let off = (m2 * (1.0 - (m2 * m2).min(1.0)).sqrt() * 0.828_427 + 1.0) * p.collision_radius + 8.0;
            let end = start + fwd * off;
            let h = self.world.line_check(end, start, Vec3::ZERO);
            if !h.hit || h.surface.exclude_foot_moves {
                return false;
            }
        }
        let p = &self.pawn;
        if w.velocity_start_limit * w.velocity_start_limit > p.velocity.size_sq_2d() || p.velocity.z <= 0.0 {
            return false;
        }
        let fwd = p.rotation.vector().safe_normal_2d();
        if p.velocity.safe_normal_2d().dot(fwd) < 0.0 {
            return false;
        }
        w.vertical_start_angle.to_radians().cos() <= (-fwd).dot(p.move_normal.safe_normal_2d()).abs()
    }

    /// TdMove_WallClimb.StartMove.
    pub fn wallclimb_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        let jump_add = self.moves.jump.jump_add_xy;
        let speed = self.pawn.velocity.size_2d() - jump_add;
        let rot = self.pawn.rotation;
        let g = self.pawn.gravity_z().abs();
        {
            let w = &mut self.moves.wallclimb;
            w.has_reached_wall = false;
            w.found_possible_hand_plant = false;
            w.performed_double_jump = false;
            w.into_wall_climb_speed = speed;
            w.look_at_edge_angle = rot;
            w.gravity = g;
        }
        self.pawn.floor = self.pawn.move_normal; // SetBase(MovementActor, MoveNormal)
        if self.wallclimb_detect_possible_hand_plant(m) {
            self.wallclimb_found_possible_hand_plant(m);
        }
        self.anim.play(Slot::FullBody, "WallRunVertical", 0.2, 0.2, 0.0, true, false, false);
        self.moves.wallclimb.ground_z_loc = self.pawn.last_jump_location.z - self.pawn.collision_height;
        if self.moves.wallclimb.into_wall_climb_speed < 100.0 {
            let suck = -self.pawn.move_normal * 400.0;
            self.pawn.velocity.x = suck.x;
            self.pawn.velocity.y = suck.y;
        }
    }

    pub fn wallclimb_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.anim.stop(Slot::FullBody, 0.25);
    }

    fn wallclimb_found_possible_hand_plant(&mut self, m: Move) {
        let dest = self.moves.wallclimb.possible_edge_destination;
        self.wallclimb_look_at_ledge(m, dest);
        self.moves.wallclimb.found_possible_hand_plant = true;
    }

    /// TdMove_WallClimb.LookAtLedge.
    fn wallclimb_look_at_ledge(&mut self, m: Move, ledge: Vec3) {
        let mut eye = self.pawn.location;
        eye.z += self.pawn.base_eye_height;
        if ledge.z > eye.z {
            let wall = Rotator::from_vector(-self.pawn.move_normal).normalize();
            let mut a = self.pawn.rotation;
            a.pitch = Rotator::from_vector((ledge - eye).safe_normal()).pitch;
            a.yaw = if ((wall.yaw - self.pawn.rotation.yaw) as f32).abs() > 4096.0 { self.pawn.rotation.yaw } else { wall.yaw };
            let a = a.normalize();
            self.moves.wallclimb.look_at_edge_angle = a;
            let b = self.moves.base_mut(m);
            b.look_at_target_angle = true;
            b.look_at_target_interpolation_time = 0.2;
            b.look_at_target_duration = -1.0;
            b.look_at_target_angle_v = a;
        } else {
            let b = self.moves.base_mut(m);
            b.look_at_target_angle = false;
            b.look_at_target_location = false;
            b.look_at_target_duration = -1.0;
        }
    }

    /// TdMove_WallClimb.ReachedWall.
    pub fn wallclimb_reached_wall(&mut self, _m: Move) {
        let w = &self.moves.wallclimb;
        let smb = self.pawn.speed_max_base_velocity;
        let i = ((((w.into_wall_climb_speed - smb) as i32).max(0)) as f32 / (w.add_on_speed_2d_max_limit - smb)).clamp(0.0, 1.0);
        let mut add = w.add_on_speed_2d_height * i;
        let i2 = (self.pawn.velocity.z / w.add_on_speed_z_max_limit).clamp(0.0, 1.0);
        add += w.add_on_speed_z_height * i2;
        let g = w.default_gravity;
        self.moves.wallclimb.gravity = g;
        self.pawn.velocity.z = (4.0 * add * g).max(0.01).sqrt();
        self.moves.wallclimb.has_reached_wall = true;
        let rate = if self.moves.wallclimb.found_possible_hand_plant {
            let d = self.moves.wallclimb.possible_edge_destination.z - (self.pawn.location.z + self.pawn.collision_height);
            let t = (2.0 * d / g).max(0.01).sqrt();
            if t > 0.4 { (0.4 / t).clamp(0.1, 0.85) } else { 0.3 }
        } else {
            let t = self.pawn.velocity.z / (2.0 * g);
            (0.4 / t).clamp(0.1, 1.0)
        };
        if let Some(s) = self.anim.slots.get_mut(&Slot::FullBody) {
            s.rate = rate;
        }
    }

    /// UTdMove_WallClimb vt75: probe for a ledge above, from where the wall was found.
    fn wallclimb_detect_possible_hand_plant(&mut self, m: Move) -> bool {
        let p = &self.pawn;
        let loc = p.move_ledge_location + p.move_normal * p.collision_radius + Vec3::new(0.0, 0.0, 215.0);
        let rot = p.rotation;
        let dist = self.moves.base(m).hand_plant_check_distance;
        let saved = (p.move_ledge_location, p.move_ledge_normal, p.move_normal);
        let r = self.detect_possible_hand_plant(m, loc, rot, dist, true);
        self.pawn.move_ledge_result = r;
        if r != 2 {
            // DetectPossibleHandPlant only writes its outputs on success
            let _ = saved;
            return false;
        }
        self.moves.wallclimb.possible_edge_destination = self.pawn.move_ledge_location;
        // the native writes the ledge into a local, not into MoveLedge*: restore
        self.pawn.move_ledge_location = saved.0;
        self.pawn.move_ledge_normal = saved.1;
        self.pawn.move_normal = saved.2;
        true
    }

    /// UTdMove_WallClimb vt70 (after the TdPhysicsMove part).
    pub fn wallclimb_tick(&mut self, m: Move) {
        self.moves.base_mut(m).friction_modifier = 0.0;
        if self.pawn.physics == Physics::Falling {
            self.set_move(Move::Falling, false, false);
            return;
        }
        if !self.moves.wallclimb.found_possible_hand_plant && self.wallclimb_detect_possible_hand_plant(m) {
            self.wallclimb_found_possible_hand_plant(m);
        }
        self.wallclimb_check_double_jump(m);
        if self.moves.wallclimb.has_reached_wall {
            let n = self.pawn.floor;
            let v = self.pawn.velocity;
            self.pawn.velocity = v - n * v.dot(n);
            let f = self.moves.wallclimb.vertical_friction;
            self.moves.base_mut(m).friction_modifier = f;
        }
        self.pawn.acceleration = Vec3::new(0.0, 0.0, -self.moves.wallclimb.gravity);
    }

    /// UTdMove_WallClimb vt76 (CheckDoubleJump).
    fn wallclimb_check_double_jump(&mut self, m: Move) {
        let w = &self.moves.wallclimb;
        if w.performed_double_jump || !w.found_possible_hand_plant {
            return;
        }
        if w.min_upwards_velocity_to_double_jump > self.pawn.velocity.z && w.possible_edge_destination.z - w.ground_z_loc < 480.0 {
            let v = ((w.possible_edge_destination.z - (self.pawn.collision_height + self.pawn.location.z) + 4.0) * w.gravity * 4.0).max(0.01);
            self.pawn.velocity.z = v.sqrt();
            self.pawn.acceleration = self.pawn.velocity.safe_normal();
            self.moves.wallclimb.performed_double_jump = true;
            // event PerformDoubleJump
            self.play_move_anim(m, Slot::FullBody, "JumpSlow", 1.0, 0.15, 0.15, false, false);
        }
    }

    /// TdMove_WallClimb.UpdateViewRotation tail.
    pub fn wallclimb_after_view_rotation(&mut self, m: Move) {
        if self.moves.wallclimb.found_possible_hand_plant {
            let d = self.moves.wallclimb.possible_edge_destination;
            self.wallclimb_look_at_ledge(m, d);
        } else {
            let mut t = self.pawn.location - self.pawn.move_normal * self.pawn.collision_radius;
            t.z += self.pawn.base_eye_height + 100.0;
            self.wallclimb_look_at_ledge(m, t);
        }
    }
}
