//! TdMove_SpeedVault (and TdMove_VaultOver, which only subclasses it): a timed three-stage
//! precise-location move (up to the hand plant, over the ledge, down to the end position),
//! picked from `VaultTypes` by hand-plant height, speed and whether the far side is a floor.

use crate::math::{UeVec, Vec3};
use crate::moves::PreciseMode;
use crate::pawn::{Move, MoveAction, MoveActionHint, Physics, Slot};
use crate::sim::Sim;

/// `TdMove_SpeedVault.VaultType`.
#[derive(Clone, Debug)]
pub struct VaultType {
    pub anim_name: &'static str,
    pub vault_onto: bool,
    pub min_height: f32,
    pub max_height: f32,
    pub min_speed_z: f32,
    pub max_speed_z: f32,
    pub min_momentum: f32,
    pub max_momentum: f32,
    pub max_distance_time: f32,
    pub clamp_speed_min: f32,
    pub clamp_speed_max: f32,
    pub speed_addition: f32,
    pub vault_time_up: f32,
    pub vault_time_over: f32,
    pub vault_time_down: f32,
    pub handplant_offset: Vec3,
    pub ledge_offset: Vec3,
    pub melee_possible: bool,
    pub reset_camera: bool,
    pub is_stringable: bool,
}

/// VaultTypes from DefaultPawnMovement.ini [TdGame.TdMove_SpeedVault] (identical to the class
/// defaults; TdMove_VaultOver's section is empty so it inherits them).
fn vault_types() -> Vec<VaultType> {
    let v = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    let base = VaultType {
        anim_name: "",
        vault_onto: true,
        min_height: 0.0,
        max_height: 0.0,
        min_speed_z: 0.0,
        max_speed_z: 0.0,
        min_momentum: -1.0,
        max_momentum: -1.0,
        max_distance_time: 0.0,
        clamp_speed_min: 0.0,
        clamp_speed_max: 0.0,
        speed_addition: 0.0,
        vault_time_up: 0.0,
        vault_time_over: 0.2,
        vault_time_down: 0.2,
        handplant_offset: Vec3::ZERO,
        ledge_offset: Vec3::ZERO,
        melee_possible: true,
        reset_camera: false,
        is_stringable: false,
    };
    vec![
        VaultType {
            anim_name: "autostepuprightleg",
            vault_onto: true,
            min_height: 0.0,
            max_height: 48.0,
            min_speed_z: -600.0,
            max_speed_z: 0.0,
            max_distance_time: 0.2,
            clamp_speed_min: 100.0,
            clamp_speed_max: 300.0,
            vault_time_up: 0.0,
            vault_time_over: 0.3,
            vault_time_down: 0.2,
            handplant_offset: v(0.0, 0.0, 5.0),
            ledge_offset: v(0.0, 0.0, 90.0),
            ..base.clone()
        },
        VaultType {
            anim_name: "stepuprightleg88",
            vault_onto: true,
            min_height: 48.0,
            max_height: 148.0,
            min_speed_z: 0.0,
            max_speed_z: 700.0,
            max_momentum: 200.0,
            max_distance_time: 0.4,
            clamp_speed_min: 200.0,
            clamp_speed_max: 700.0,
            vault_time_over: 0.4,
            vault_time_down: 0.25,
            ledge_offset: v(0.0, -20.0, 60.0),
            ..base.clone()
        },
        VaultType {
            anim_name: "vaultOnto",
            vault_onto: true,
            min_height: 64.0,
            max_height: 148.0,
            min_speed_z: 0.0,
            max_speed_z: 10000.0,
            max_distance_time: 0.4,
            clamp_speed_min: 400.0,
            clamp_speed_max: 720.0,
            speed_addition: 80.0,
            vault_time_up: 0.0,
            vault_time_over: 0.35,
            vault_time_down: 0.3,
            handplant_offset: v(0.0, 0.0, 5.0),
            ledge_offset: v(0.0, 0.0, 25.0),
            is_stringable: true,
            ..base.clone()
        },
        VaultType {
            anim_name: "vaultOver",
            vault_onto: false,
            min_height: 64.0,
            max_height: 148.0,
            min_speed_z: 0.0,
            max_speed_z: 10000.0,
            max_distance_time: 0.4,
            clamp_speed_min: 400.0,
            clamp_speed_max: 720.0,
            speed_addition: 80.0,
            vault_time_up: 0.0,
            vault_time_over: 0.35,
            vault_time_down: 0.3,
            handplant_offset: v(0.0, 0.0, 5.0),
            ledge_offset: v(0.0, 0.0, 25.0),
            is_stringable: true,
            ..base.clone()
        },
        VaultType {
            anim_name: "VaultOverHigh",
            vault_onto: false,
            min_height: 145.0,
            max_height: 192.0,
            min_speed_z: 50.0,
            max_speed_z: 10000.0,
            max_distance_time: 0.4,
            clamp_speed_min: 200.0,
            clamp_speed_max: 400.0,
            vault_time_up: 0.28,
            vault_time_over: 0.3,
            vault_time_down: 0.45,
            handplant_offset: v(0.0, -40.0, -69.0),
            ledge_offset: v(0.0, 0.0, 5.0),
            reset_camera: true,
            ..base.clone()
        },
        VaultType {
            anim_name: "VaultOntoHigh",
            vault_onto: true,
            min_height: 145.0,
            max_height: 192.0,
            min_speed_z: 50.0,
            max_speed_z: 10000.0,
            max_distance_time: 0.4,
            clamp_speed_min: 200.0,
            clamp_speed_max: 400.0,
            vault_time_up: 0.27,
            vault_time_over: 0.3,
            vault_time_down: 0.6,
            handplant_offset: v(0.0, -65.0, -15.0),
            ledge_offset: v(0.0, 0.0, 35.0),
            reset_camera: true,
            ..base
        },
    ]
}

pub struct Vault {
    pub vault_clear_object_height: f32,
    pub max_time_to_ledge: f32,
    pub vault_types: Vec<VaultType>,
    pub active_vault_type: i32,
    pub vault_end_position: Vec3,
    pub over_end_location: Vec3,
    pub onto_end_location: Vec3,
    pub saved_velocity: Vec3,
    pub start_to_handplant: Vec3,
    pub vault_state: i32,
    pub move_direction: Vec3,
    pub hand_location: Vec3,
    pub target_location: Vec3,
    pub vault_onto: bool,
    pub end_move_falling: bool,
    pub end_move_in_melee: bool,
    pub vault_speed: f32,
    pub handplant_height: f32,
}

impl Vault {
    pub fn new(cfg: &crate::config::Config) -> Self {
        let c = &["TdMove_SpeedVault"];
        Vault {
            vault_clear_object_height: cfg.f32(c, "VaultClearObjectHeight", 35.0),
            max_time_to_ledge: cfg.f32(c, "MaxTimeToLedge", 0.4),
            vault_types: vault_types(),
            active_vault_type: 0,
            vault_end_position: Vec3::ZERO,
            over_end_location: Vec3::ZERO,
            onto_end_location: Vec3::ZERO,
            saved_velocity: Vec3::ZERO,
            start_to_handplant: Vec3::ZERO,
            vault_state: 0,
            move_direction: Vec3::ZERO,
            hand_location: Vec3::ZERO,
            target_location: Vec3::ZERO,
            vault_onto: false,
            end_move_falling: false,
            end_move_in_melee: false,
            vault_speed: 0.0,
            handplant_height: 0.0,
        }
    }

    fn vt(&self) -> &VaultType {
        &self.vault_types[self.active_vault_type as usize]
    }
}

impl Sim {
    /// TdMove_SpeedVault.UpdateActiveVaultType.
    fn vault_update_active_type(&self, time_to_hand_plant: f32) -> i32 {
        let momentum = self.pawn.velocity.size_2d();
        if self.pawn.movement_state == Move::Grabbing {
            return 3;
        }
        let v = &self.moves.vault;
        for (idx, t) in v.vault_types.iter().enumerate() {
            if v.handplant_height < t.min_height || v.handplant_height > t.max_height {
                continue;
            }
            if t.vault_onto != v.vault_onto && idx != 0 {
                continue;
            }
            if t.min_momentum != -1.0 && momentum < t.min_momentum {
                continue;
            }
            if t.max_momentum != -1.0 && momentum > t.max_momentum {
                continue;
            }
            if t.min_speed_z != -1.0 && self.pawn.velocity.z < t.min_speed_z {
                continue;
            }
            if t.max_speed_z != -1.0 && self.pawn.velocity.z > t.max_speed_z {
                continue;
            }
            if time_to_hand_plant > t.max_distance_time {
                continue;
            }
            return idx as i32;
        }
        -1
    }

    /// TdMove_SpeedVault.CanDoMove.
    pub fn vault_can_do_move(&mut self, m: Move) -> bool {
        self.moves.vault.vault_onto = false;
        self.moves.vault.end_move_falling = false;
        if !self.tdmove_can_do_move(m) || !self.pawn.found_ledge {
            return false;
        }
        let ms = self.pawn.movement_state;
        if ms == Move::VaultOver || ms == Move::SpeedVaulting {
            return false;
        }
        if ms == Move::WallClimbing && self.moves.wallclimb.performed_double_jump {
            return false;
        }
        // Moves[GrabTransfer].TransferLocation when grabbing: GrabTransfer is not ported yet.
        let ref_loc = self.pawn.location;
        if self.pawn.found_ledge_excludes_hand_moves {
            return false;
        }
        if self.pawn.move_action_hint != MoveActionHint::Up && ms != Move::Grabbing {
            return false;
        }
        if self.pawn.move_ledge_normal.z < 0.3 {
            return false;
        }
        if self.pc.rotation.vector().dot(self.pawn.move_normal) > -0.2 {
            return false;
        }
        let p = &self.pawn;
        let start_to_handplant = p.move_ledge_location - ref_loc;
        let handplant_height = p.move_ledge_location.z - ref_loc.z + p.collision_height;
        let mut saved = p.velocity;
        saved.z = 0.0;
        let mut floor_velocity = saved.size_2d();
        let time_to_hand_plant = start_to_handplant.size_2d() / (floor_velocity as i32).max(300) as f32;
        let ledge = p.move_ledge_location;
        let hand = ledge - p.move_normal.cross(p.move_ledge_normal) * 20.0;
        {
            let v = &mut self.moves.vault;
            v.start_to_handplant = start_to_handplant;
            v.handplant_height = handplant_height;
            v.vault_onto = false;
            v.saved_velocity = saved;
        }
        if time_to_hand_plant > self.moves.vault.max_time_to_ledge {
            return false;
        }
        let mut md = start_to_handplant;
        md.z = 0.0;
        let md = md.safe_normal();
        {
            let v = &mut self.moves.vault;
            v.hand_location = hand;
            v.move_direction = md;
            v.vault_end_position = ledge + md * 48i32.max((floor_velocity * 0.3) as i32) as f32;
            v.vault_end_position.z = ref_loc.z;
        }
        if !self.vault_check_collision(ledge, ref_loc, floor_velocity) {
            return false;
        }
        let at = self.vault_update_active_type(time_to_hand_plant);
        self.moves.vault.active_vault_type = at;
        if at == -1 {
            return false;
        }
        let v = &mut self.moves.vault;
        let t = v.vault_types[at as usize].clone();
        if !v.vault_onto && t.vault_onto {
            v.vault_end_position = v.onto_end_location;
            v.vault_onto = t.vault_onto;
        }
        floor_velocity = ((floor_velocity + t.speed_addition) as i32).clamp(t.clamp_speed_min as i32, t.clamp_speed_max as i32) as f32;
        v.saved_velocity = v.saved_velocity.safe_normal() * floor_velocity;
        if v.vault_onto {
            let end_z = v.vault_end_position.z;
            let len = ((v.vault_end_position - ledge).size_2d() as i32).min(48i32.max((floor_velocity * t.vault_time_down) as i32));
            v.vault_end_position = ledge + md * len as f32;
            v.vault_end_position.z = end_z;
        }
        if ms != Move::Grabbing && (self.pawn.velocity.z < t.min_speed_z || self.pawn.velocity.z > t.max_speed_z) {
            return false;
        }
        true
    }

    /// TdMove_SpeedVault.FindValidOntoEndLocation.
    fn vault_find_valid_onto_end_location(&mut self, ledge: Vec3) -> bool {
        let r = self.pawn.collision_radius;
        let h = self.pawn.collision_height;
        let ext = Vec3::new(r * 1.4, r * 1.4, h * 0.5);
        let v = &self.moves.vault;
        let md = v.move_direction;
        let mut start = ledge;
        start.z = ledge.z + v.vault_clear_object_height + ext.z;
        let mut end = v.vault_end_position;
        end.z = start.z;
        let handplant_height = v.handplant_height;
        self.moves.vault.onto_end_location = end;
        if let Some(hit) = self.movement_trace(end, start, ext) {
            let mut hl = hit.location + md * ext.x;
            let mut width = (hl - ledge).dot(md);
            if width <= ext.x + 1.0 {
                let small = Vec3::new(10.0, 10.0, 10.0);
                // the script ignores this trace's result and reuses HitLocation
                if let Some(h2) = self.movement_trace(end, start, small) {
                    hl = h2.location;
                }
                hl += md * 10.0;
                width = (hl - ledge).dot(md);
            }
            if width < if handplant_height <= 48.0 { 32.0 } else { 64.0 } {
                return false;
            }
            self.moves.vault.onto_end_location = hl - md * r;
        }
        self.moves.vault.onto_end_location.z = ledge.z + h;
        true
    }

    /// TdMove_SpeedVault.FindValidOverEndLocation: 0 = over, 1/2 = can't go over (onto).
    fn vault_find_valid_over_end_location(&mut self, ledge: Vec3, max_ledge_width: f32) -> i32 {
        let mut ext = self.pawn.extent();
        ext.z = 48.0;
        let md = self.moves.vault.move_direction;
        let mut start = self.moves.vault.vault_end_position;
        start.z = ledge.z - ext.z;
        let mut end = ledge;
        end.z = start.z;
        self.moves.vault.over_end_location = start;
        let Some(hit) = self.movement_trace(end, start, ext) else {
            return 1;
        };
        if (hit.location - ledge).size_2d() < max_ledge_width {
            self.moves.vault.over_end_location = self.moves.vault.vault_end_position;
            return 0;
        }
        self.moves.vault.over_end_location = hit.location;
        if max_ledge_width < 80.0 {
            return 2;
        }
        let mut align = self.pawn.move_normal.dot(Vec3::new(1.0, 0.0, 0.0)).abs().acos() * 4.0 / 3.1415927;
        align = 1.0 + align * 0.414;
        let start = hit.location - md * (align * ext.x + 48.0);
        let Some(hit) = self.movement_trace(end, start, ext) else {
            return 2;
        };
        let mut end = self.moves.vault.vault_end_position;
        let start = hit.location;
        end.z = start.z;
        let Some(hit) = self.movement_trace(end, start, ext) else {
            return 2;
        };
        self.moves.vault.over_end_location = hit.location - md * 16.0;
        0
    }

    /// TdMove_SpeedVault.FindVaultFloor.
    fn vault_find_floor(&self, ledge: Vec3, wanted_end: Vec3) -> Option<f32> {
        let mut ext = self.pawn.extent();
        ext.z = 8.0;
        let mut start = wanted_end;
        start.z = ledge.z + 32.0;
        let mut end = wanted_end;
        end.z = ledge.z - if self.moves.vault.vault_onto { 32.0 } else { 240.0 };
        self.movement_trace(end, start, ext).map(|h| h.location.z - ext.z)
    }

    /// TdMove_SpeedVault.CheckCollision.
    fn vault_check_collision(&mut self, ledge: Vec3, _pawn_ref: Vec3, floor_velocity: f32) -> bool {
        if !self.vault_find_valid_onto_end_location(ledge) {
            return false;
        }
        let v = &mut self.moves.vault;
        let temp_z = v.vault_end_position.z;
        v.vault_end_position = v.onto_end_location;
        v.vault_end_position.z = temp_z;
        let temp_z = v.onto_end_location.z;
        let len = ((v.onto_end_location - ledge).size_2d() as i32).min(160);
        v.onto_end_location = ledge + v.move_direction * len as f32;
        v.onto_end_location.z = temp_z;
        let max_ledge_width = (floor_velocity * 0.2).clamp(60.0, 180.0);
        if self.vault_find_valid_over_end_location(ledge, max_ledge_width) != 0 {
            self.moves.vault.vault_onto = true;
        }
        let v = &mut self.moves.vault;
        v.vault_end_position = if v.vault_onto { v.onto_end_location } else { v.over_end_location };
        let end = v.vault_end_position;
        if let Some(floor_z) = self.vault_find_floor(ledge, end) {
            let v = &mut self.moves.vault;
            v.vault_onto = floor_z > ledge.z - 64.0;
            v.vault_end_position.z = floor_z + self.pawn.collision_height;
        } else {
            let v = &mut self.moves.vault;
            v.end_move_falling = !v.vault_onto;
        }
        let end = self.moves.vault.vault_end_position;
        self.can_stand(end, false)
    }

    /// TdMove_SpeedVault.StartMove.
    pub fn vault_start_move(&mut self, m: Move) {
        self.moves.base_mut(m).disable_face_rotation = true;
        self.moves.vault.end_move_in_melee = false;
        self.physics_move_start_move(m);
        self.pawn.velocity = Vec3::ZERO;
        let t = self.moves.vault.vt().clone();
        if t.reset_camera {
            self.reset_camera_look(m, 0.2);
        }
        self.moves.vault.vault_state = if t.vault_time_up > 0.0 { 0 } else { 1 };
        self.moves.vault.target_location = self.pawn.location;
        self.anim.stop(Slot::FullBodyDir, 0.2);
        // AnimNameVariation is None for every type
        self.play_move_anim(m, Slot::FullBody, t.anim_name, 1.0, 0.15, 0.2, false, false);
        self.vault_update_movement(m);
    }

    /// TdMove_SpeedVault.StopMove.
    pub fn vault_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        // DisableVaultIK(0.1): hand IK, visual only
    }

    /// TdMove_SpeedVault.OnTimer.
    pub fn vault_on_timer(&mut self, m: Move) {
        self.vault_update_movement(m);
    }

    /// TdMove_SpeedVault.UpdateVaultMovement.
    fn vault_update_movement(&mut self, m: Move) {
        let t = self.moves.vault.vt().clone();
        let md = self.moves.vault.move_direction;
        match self.moves.vault.vault_state {
            0 => {
                let mut tl = self.pawn.move_ledge_location;
                tl.z += t.handplant_offset.z;
                tl += md * t.handplant_offset.y;
                let speed = (tl - self.pawn.location).length() / t.vault_time_up;
                self.moves.vault.target_location = tl;
                self.moves.vault.vault_speed = speed;
                self.set_precise_location(m, tl, PreciseMode::Fly, speed);
                self.set_move_countdown(m, t.vault_time_up);
                self.moves.vault.vault_state = 1;
            }
            1 => {
                let tl = self.moves.vault.target_location;
                self.set_location(tl);
                self.pawn.velocity = Vec3::ZERO;
                let mut tl = self.pawn.move_ledge_location;
                tl.z += t.ledge_offset.z;
                tl += md * t.ledge_offset.y;
                let speed = (tl - self.pawn.location).size_2d() / t.vault_time_over;
                self.moves.vault.target_location = tl;
                self.moves.vault.vault_speed = speed;
                self.set_precise_location(m, tl, PreciseMode::Jump, speed);
                let ds = self.delta_seconds;
                self.anim.set_position(Slot::FullBody, t.vault_time_up - ds);
                // EnableVaultIK(0.1): hand IK, visual only
                self.stop_ignore_look_input();
                self.set_move_countdown(m, t.vault_time_over);
                self.moves.vault.vault_state = 2;
            }
            2 => {
                let tl = self.moves.vault.target_location;
                self.set_location(tl);
                self.pawn.velocity = Vec3::ZERO;
                let falling = self.moves.vault.end_move_falling;
                if falling {
                    self.set_physics(Physics::Falling);
                }
                let end = self.moves.vault.vault_end_position;
                let speed = (end - self.pawn.location).size_2d() / t.vault_time_down;
                self.set_precise_location(m, end, if falling { PreciseMode::Fall } else { PreciseMode::Jump }, speed);
                self.moves.vault.vault_state = 3;
                self.moves.vault.saved_velocity = md * speed;
                self.moves.base_mut(m).disable_face_rotation = false;
                self.pawn.face_rotation_time_left = 0.4;
                self.pawn.leg_rotation = self.pc.rotation.yaw;
                self.set_move_countdown(m, t.vault_time_down);
                // DisableVaultIK(0.1)
            }
            _ => {
                self.moves.base_mut(m).use_precise_location = false;
                let v = &self.moves.vault;
                if v.vault_onto || !v.end_move_falling {
                    let next = if v.vault_onto { Move::Walking } else { Move::Landing };
                    self.set_move(next, false, false);
                    let mut sv = self.moves.vault.saved_velocity;
                    sv.z = 0.0f32.max(self.pawn.velocity.z);
                    self.moves.vault.saved_velocity = sv;
                    self.pawn.velocity = sv;
                    self.pawn.acceleration = sv.safe_normal();
                    self.pc.acceleration_time = 0.2;
                } else {
                    let sv = self.moves.vault.saved_velocity;
                    self.pawn.velocity = self.pawn.velocity.safe_normal() * sv.size_2d();
                    self.pawn.acceleration = sv.safe_normal();
                    self.pc.acceleration_time = 0.2;
                    self.set_move(Move::Falling, false, false);
                }
            }
        }
    }

    /// TdMove_SpeedVault.FailedToReachPreciseLocation.
    pub fn vault_failed_precise_location(&mut self, m: Move) {
        if self.moves.vault.vault_state == 3 {
            self.vault_update_movement(m);
        }
    }

    /// TdMove_SpeedVault.HandleMoveAction.
    pub fn vault_handle_move_action(&mut self, _m: Move, a: MoveAction) {
        if a == MoveAction::Melee && self.moves.vault.vt().melee_possible {
            self.moves.vault.end_move_in_melee = true;
        }
    }

    /// TdMove_SpeedVault.IsThisMoveStringable.
    pub fn vault_is_stringable(&self) -> bool {
        self.moves.vault.vt().is_stringable
    }
}
