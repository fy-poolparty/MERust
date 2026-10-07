//! TdMove_SpringBoard: step on a low obstacle (IntermediateFootPlantHeight) in front of a higher
//! one and launch off it. Three precise-location stages, then a fixed launch velocity.

use crate::config::Config;
use crate::math::{UeVec, Vec3};
use crate::moves::PreciseMode;
use crate::pawn::{Move, Physics, Slot};
use crate::sim::Sim;

pub struct SpringBoard {
    pub max_height: f32,
    pub min_height: f32,
    pub jump_z: f32,
    pub jump_xy_add: f32,
    pub jump_xy_min: f32,
    pub intermediate_foot_plant_height: f32,
    pub intermediate_foot_plant_distance: f32,
    pub check_distance_time: f32,
    pub step_time1: f32,
    pub step_time2: f32,
    pub intermediate_location: Vec3,
    pub springboard_location: Vec3,
    pub state: i32,
    pub saved_initial_speed: i32,
}

impl SpringBoard {
    pub fn new(cfg: &Config) -> Self {
        let c = &["TdMove_SpringBoard"];
        SpringBoard {
            max_height: cfg.f32(c, "SpringBoardMaxHeight", 148.0),
            min_height: cfg.f32(c, "SpringBoardMinHeight", 80.0),
            jump_z: cfg.f32(c, "SpringBoardJumpZ", 950.0),
            jump_xy_add: cfg.f32(c, "SpringBoardJumpXYAdd", -100.0),
            jump_xy_min: cfg.f32(c, "SpringBoardJumpXYMin", 400.0),
            intermediate_foot_plant_height: cfg.f32(c, "IntermediateFootPlantHeight", 64.0),
            intermediate_foot_plant_distance: cfg.f32(c, "IntermediateFootPlantDistance", 112.0),
            check_distance_time: cfg.f32(c, "CheckDistanceTime", 1.0),
            step_time1: 0.2,
            step_time2: 0.2,
            intermediate_location: Vec3::ZERO,
            springboard_location: Vec3::ZERO,
            state: 0,
            saved_initial_speed: 0,
        }
    }
}

impl Sim {
    /// TdMove_SpringBoard.CanDoMove.
    pub fn springboard_can_do_move(&mut self, m: Move) -> bool {
        if !self.tdmove_can_do_move(m) || !self.pawn.found_ledge {
            return false;
        }
        let p = &self.pawn;
        let sb = &self.moves.springboard;
        let ledge_height = p.move_ledge_location.z - (p.location.z - p.collision_height);
        let dist = (p.move_ledge_location - p.location).size_2d();
        if dist < 20.0 {
            return false;
        }
        let time_to_ledge = dist / (self.get_average_speed(0.25) as i32).max(1) as f32;
        if time_to_ledge > sb.check_distance_time {
            return false;
        }
        if (ledge_height - sb.intermediate_foot_plant_height).abs() >= 20.0 {
            return false;
        }
        let mut ext = Vec3::new(4.0, 4.0, 8.0);
        ext.z = (sb.max_height - sb.min_height) * 0.5;
        let mut start = p.move_ledge_location;
        start.z = p.location.z - p.collision_height;
        start.z += sb.min_height + ext.z;
        let end = start + p.rotation.vector() * sb.intermediate_foot_plant_distance * 1.414;
        let Some(fp) = self.find_ledge(start, end, ext) else {
            return false;
        };
        let mut test = fp.ledge_location;
        test.z += self.pawn.default_collision_height + 4.0;
        if !self.can_stand(test, false) {
            return false;
        }
        if fp.ledge_normal.z < 0.8 || fp.feet_excluded {
            return false;
        }
        let mut n = fp.move_normal;
        n.z = 0.0;
        let n = n.safe_normal();
        let between = fp.ledge_location - self.pawn.move_ledge_location;
        // VSize2D(V << N): project onto the foot-plant wall normal
        let proj = n * between.dot(n);
        if (proj.size_2d() - self.moves.springboard.intermediate_foot_plant_distance).abs() >= 20.0 {
            return false;
        }
        let sb = &mut self.moves.springboard;
        sb.intermediate_location = self.pawn.move_ledge_location + Vec3::new(0.0, 0.0, 65.0);
        sb.springboard_location = fp.ledge_location + Vec3::new(0.0, 0.0, 92.0);
        true
    }

    /// TdMove_SpringBoard.StartMove.
    pub fn springboard_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        self.moves.springboard.saved_initial_speed = 200i32.max(self.pawn.velocity.size_2d() as i32);
        self.moves.springboard.state = 0;
        let mut to_ledge = self.moves.springboard.intermediate_location - self.pawn.location;
        to_ledge.z = 0.0;
        // LandNode.Landed / LandingRun blend: animation only
        if to_ledge.size_2d() > 120.0 {
            let target = self.pawn.location + to_ledge.safe_normal() * (to_ledge.size_2d() - 120.0);
            let speed = self.moves.springboard.saved_initial_speed as f32 * 1.2;
            self.set_precise_location(m, target, PreciseMode::Walk, speed);
        } else {
            self.springboard_reached_precise_location(m);
        }
        self.pawn.collide_world = false;
    }

    /// TdMove_SpringBoard.ReachedPreciseLocation.
    pub fn springboard_reached_precise_location(&mut self, m: Move) {
        match self.moves.springboard.state {
            0 => {
                let t = self.moves.springboard.intermediate_location;
                let speed = (t - self.pawn.location).size_2d() / self.moves.springboard.step_time1;
                self.set_precise_location(m, t, PreciseMode::Fly, speed);
                let anim = if self.is_left_leg_forward() { "SpringBoardRightLeg" } else { "SpringBoardLeftLeg" };
                self.play_move_anim(m, Slot::FullBody, anim, 1.0, 0.15, 0.25, false, false);
                self.moves.springboard.state = 1;
            }
            1 => {
                let t = self.moves.springboard.springboard_location;
                let speed = (t - self.pawn.location).size_2d() / self.moves.springboard.step_time2;
                self.set_precise_location(m, t, PreciseMode::Fly, speed);
                self.moves.springboard.state = 2;
            }
            2 => {
                let sb = &self.moves.springboard;
                let xy = sb.jump_xy_min.max(sb.saved_initial_speed as f32 + sb.jump_xy_add);
                let jz = sb.jump_z;
                self.pawn.velocity = self.pawn.rotation.vector() * xy;
                self.pawn.velocity.z = jz;
                self.pawn.last_jump_location = self.pawn.location;
                self.pawn.collide_world = true;
                self.set_physics(Physics::Falling);
                self.stop_ignore_move_input();
            }
            _ => {}
        }
    }

    /// TdMove_SpringBoard.FailedToReachPreciseLocation.
    pub fn springboard_failed_precise_location(&mut self, _m: Move) {
        self.set_move(Move::Walking, false, false);
    }

    /// TdMove_SpringBoard.StopMove.
    pub fn springboard_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.anim.stop(Slot::FullBody, 0.1);
        self.pawn.collide_world = true;
    }
}
