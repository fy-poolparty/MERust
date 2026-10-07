//! TdMove_GrabTransfer: from a hang (or a ladder), jump across to another ledge round a corner,
//! sideways or straight up, and grab it.

use crate::config::Config;
use crate::math::{Rotator, UeVec, Vec3};
use crate::moves::PreciseMode;
use crate::pawn::{Move, MoveActionHint, Physics, Slot};
use crate::sim::Sim;

#[derive(Clone, Debug, Default)]
pub struct GrabTransfer {
    pub allowed_2d_transfer_distance: f32,
    pub allowed_z_transfer_distance: f32,
    pub transfer_location: Vec3,
    pub transfer_normal: Vec3,
    pub transfer_look_at_location: Vec3,
    pub transfer_ledge_normal: Vec3,
    pub transfer_hint: MoveActionHint,
    pub transfer_move: Move,
    pub transfer_speed: f32,
    pub transfer_distance: f32,
    pub fit_for_grab: bool,
    /// TransferLadder (index into Sim::ladders).
    pub transfer_ladder: Option<usize>,
}

impl GrabTransfer {
    pub fn new(cfg: &Config) -> Self {
        let ch = &["TdMove_GrabTransfer", "TdPhysicsMove", "TdMove"];
        GrabTransfer {
            allowed_2d_transfer_distance: cfg.f32(ch, "Allowed2DTransferDistance", 260.0),
            allowed_z_transfer_distance: cfg.f32(ch, "AllowedZTransferDistance", 140.0),
            ..Default::default()
        }
    }
}

/// The view's rotation relative to the pawn's, both normalised first (FRotator::GetNormalized).
fn relative_view(sim: &Sim) -> Rotator {
    (sim.pc.rotation.normalize() - sim.pawn.rotation.normalize()).normalize()
}

impl Sim {
    /// TdMove_GrabTransfer.CanDoMove.
    pub fn grab_transfer_can_do_move(&mut self, m: Move) -> bool {
        if !self.tdmove_can_do_move(m) {
            return false;
        }
        let Some((loc, normal, look_at, ledge_normal)) = self.grab_transfer_check_context_move() else { return false };
        let t = &mut self.moves.grab_transfer;
        t.transfer_location = loc;
        t.transfer_normal = normal;
        t.transfer_look_at_location = look_at;
        t.transfer_ledge_normal = ledge_normal;
        let r = self.pawn.default_collision_radius;
        let extent = Vec3::new(r, r, self.pawn.default_collision_height);
        let fit = !self.movement_trace_for_blocking(loc, self.pawn.location, extent);
        self.moves.grab_transfer.fit_for_grab = fit;
        if self.moves.grab_transfer.transfer_hint == MoveActionHint::Up {
            let to_vault = self.grab_transfer_check_reachable_vault_over();
            if !fit {
                return to_vault;
            }
        }
        if !fit {
            return false;
        }
        matches!(self.pawn.movement_state, Move::Grabbing | Move::Climb)
    }

    /// UTdMove_GrabTransfer::CheckContextMove (vt75, 0x120C1A0): the transfer direction from the
    /// move-action hint (only up while hanging free; left/right only with the view that way),
    /// then a ladder or a ledge.
    fn grab_transfer_check_context_move(&mut self) -> Option<(Vec3, Vec3, Vec3, Vec3)> {
        let mut hint = MoveActionHint::None;
        let h = self.pawn.move_action_hint;
        if h != MoveActionHint::None && h != MoveActionHint::Down {
            if self.pawn.movement_state != Move::Climb || h == MoveActionHint::Up {
                if self.grab_is_hanging_free() {
                    if h == MoveActionHint::Up {
                        hint = MoveActionHint::Up;
                    }
                } else {
                    hint = h;
                }
            } else {
                hint = h;
            }
        }
        let yaw = relative_view(self).yaw;
        let off = match hint {
            MoveActionHint::Up => !(-8192..=8192).contains(&yaw),
            MoveActionHint::Left => yaw > 8192 || yaw < -16384,
            MoveActionHint::Right => yaw < -8192 || yaw > 16384,
            _ => false,
        };
        if off {
            hint = MoveActionHint::None;
        }
        self.moves.grab_transfer.transfer_hint = hint;
        if let Some(r) = self.grab_transfer_to_ladder() {
            self.moves.grab_transfer.transfer_move = Move::IntoClimb;
            return Some(r);
        }
        let r = self.grab_transfer_to_ledge()?;
        self.moves.grab_transfer.transfer_move = Move::IntoGrab;
        Some(r)
    }

    /// The ladder half of CheckContextMove (0x1208520): a box sweep (the pawn's width,
    /// HandPlantExtentCheckHeight tall) from just beside the pawn out to Allowed2DTransferDistance,
    /// right / left / along the view, for a ladder volume other than the one being climbed.
    fn grab_transfer_to_ladder(&mut self) -> Option<(Vec3, Vec3, Vec3, Vec3)> {
        let (_, y, _) = self.pawn.rotation.axes();
        let dir = match self.moves.grab_transfer.transfer_hint {
            MoveActionHint::Right => y,
            MoveActionHint::Left => -y,
            MoveActionHint::None => self.pc.rotation.vector(),
            _ => return None,
        };
        let dir = Vec3::new(dir.x, dir.y, 0.0).safe_normal();
        let b = self.moves.base(Move::GrabTransfer);
        let (w, ch, eh) = (b.hand_plant_extent_check_width, b.hand_plant_check_height, b.hand_plant_extent_check_height);
        let mut start = self.pawn.location + dir * w * 2.0;
        start.z += ch - self.pawn.default_collision_height;
        let end = start + dir * self.moves.grab_transfer.allowed_2d_transfer_distance;
        let ext = Vec3::new(self.pawn.collision_radius, self.pawn.collision_radius, eh);
        let current = self.moves.climb.ladder;
        let climbing = self.pawn.movement_state == Move::Climb;
        let mut found = None;
        for (i, l) in self.ladders.iter().enumerate() {
            if l.sweep_hits(start, end, ext) && !(climbing && current == Some(i)) {
                found = Some(i);
            }
        }
        self.moves.grab_transfer.transfer_ladder = found;
        let l = &self.ladders[found?];
        let look_at = Vec3::new(l.center.x, l.center.y, start.z);
        let step = l.closest_step(self.pawn.location.z).clamp(0, l.last_step());
        Some((l.ladder_location(step), l.wall_normal, look_at, Vec3::ZERO))
    }

    /// The ledge half of CheckContextMove (0x1208060): a hand-plant probe left, right or along
    /// the view (or straight up, two collision heights higher and two radii out) for a ledge
    /// on another wall, or above the current one for an up transfer.
    fn grab_transfer_to_ledge(&mut self) -> Option<(Vec3, Vec3, Vec3, Vec3)> {
        let mut location = self.pawn.location;
        let mut dist = self.moves.grab_transfer.allowed_2d_transfer_distance;
        let rel = relative_view(self);
        if self.moves.grab_transfer.transfer_hint == MoveActionHint::None && rel.pitch > 4096 && (-8191..=8191).contains(&rel.yaw) {
            self.moves.grab_transfer.transfer_hint = MoveActionHint::Up;
        }
        let hint = self.moves.grab_transfer.transfer_hint;
        let mut rot = self.pawn.rotation;
        match hint {
            MoveActionHint::Up => {
                location.z += self.pawn.default_collision_height * 2.0;
                dist = self.pawn.default_collision_radius * 2.0;
            }
            MoveActionHint::Left => rot.yaw -= 0x4000,
            MoveActionHint::Right => rot.yaw += 0x4000,
            _ => {}
        }
        if hint == MoveActionHint::None {
            rot = self.pc.rotation;
        }
        rot.yaw = crate::math::norm_axis(rot.yaw);
        let (ledge, found) = self.detect_possible_hand_plant_out(Move::GrabTransfer, location, rot, dist, false);
        let Some((hl, hr)) = found.filter(|_| ledge == 2) else {
            self.pawn.found_ledge = false;
            return None;
        };
        let ledge_location = (hl.ledge_location + hr.ledge_location) * 0.5;
        let move_normal = hl.move_normal;
        let same_wall = self.pawn.move_normal.dot(move_normal) > 0.99;
        if same_wall || hint == MoveActionHint::Up {
            if hint != MoveActionHint::Up {
                return None;
            }
            let dz = ledge_location.z - self.pawn.move_ledge_location.z;
            if !(1.0..=self.moves.grab_transfer.allowed_z_transfer_distance).contains(&dz) {
                return None;
            }
        }
        let r = self.pawn.default_collision_radius;
        let n2 = Vec3::new(move_normal.x, move_normal.y, 0.0).safe_normal();
        let rel_ext = self.calculate_relative_extent(r);
        let mut loc = ledge_location + n2 * (rel_ext + r);
        loc.z = ledge_location.z - self.moves.grab.desired_ledge_offset.z;
        self.pawn.found_ledge = true;
        Some((loc, n2, ledge_location, hl.ledge_normal))
    }

    /// TdMove_GrabTransfer.CheckReachableVaultOver.
    fn grab_transfer_check_reachable_vault_over(&mut self) -> bool {
        let old = self.pawn.move_ledge_location;
        self.pawn.move_ledge_location = self.moves.grab_transfer.transfer_look_at_location;
        let start = self.pawn.location;
        let mut end = start;
        end.z = self.pawn.move_ledge_location.z + self.pawn.collision_height;
        if self.movement_trace_for_blocking(start, end, self.pawn.extent()) {
            return false;
        }
        if self.can_do_move(Move::VaultOver) {
            self.moves.grab_transfer.transfer_move = Move::VaultOver;
            true
        } else {
            self.pawn.move_ledge_location = old;
            false
        }
    }

    /// TdMove_GrabTransfer.StartMove.
    pub fn grab_transfer_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        let t = self.moves.grab_transfer.clone();
        if !t.fit_for_grab && t.transfer_move == Move::VaultOver {
            self.pawn.collide_world = false;
        }
        self.anim.stop(Slot::FullBody, 0.1);
        self.use_root_motion(false);
        self.use_root_rotation(false);
        self.moves.base_mut(m).disable_face_rotation = true;
        let dist = (t.transfer_location - self.pawn.location).length();
        self.pawn.move_location = t.transfer_location;
        self.pawn.move_normal = t.transfer_normal;
        self.pawn.move_ledge_location = t.transfer_look_at_location;
        self.pawn.move_ledge_normal = t.transfer_ledge_normal;
        self.moves.grab_transfer.transfer_distance = dist;
        self.moves.grab_transfer.transfer_speed = (dist / 0.55).max(200.0);
        let old = self.pawn.old_movement_state;
        self.set_animation_movement_state(old, 0.0);
        if t.transfer_hint != MoveActionHint::Up {
            self.set_look_at_target_location(m, t.transfer_look_at_location, 0.2, -1.0);
            let anim = if t.transfer_hint == MoveActionHint::Right { "HangTurnRightStart" } else { "HangTurnLeftStart" };
            self.play_move_anim(m, Slot::FullBody, anim, 1.0, 0.2, 0.1, false, false);
            self.set_move_countdown(m, 0.2);
        } else {
            self.grab_transfer_on_timer(m);
        }
    }

    /// TdMove_GrabTransfer.OnTimer: the jump itself.
    pub fn grab_transfer_on_timer(&mut self, m: Move) {
        let t = self.moves.grab_transfer.clone();
        self.set_precise_location(m, t.transfer_location, PreciseMode::Fly, t.transfer_speed);
        self.pawn.face_rotation_time_left = 0.5;
        self.pawn.leg_rotation = self.pc.rotation.yaw;
        self.grab_transfer_play_transfer_animation(m);
        self.set_animation_movement_state(Move::GrabTransfer, 0.0);
        if t.transfer_move != Move::VaultOver {
            self.set_look_at_target_location(m, t.transfer_look_at_location, 0.8, -1.0);
        } else {
            self.reset_camera_look(m, 0.2);
        }
        self.moves.base_mut(m).disable_face_rotation = false;
    }

    /// TdMove_GrabTransfer.PlayTransferAnimation.
    fn grab_transfer_play_transfer_animation(&mut self, m: Move) {
        self.pawn.gravity_modifier = 0.85;
        if self.moves.grab_transfer.transfer_hint == MoveActionHint::Up {
            let anim = if self.grab_is_hanging_free() { "hangfreetransferup" } else { "hangtransferup" };
            self.play_move_anim(m, Slot::FullBody, anim, 1.0, 0.1, 0.1, false, false);
        } else {
            self.play_move_anim(m, Slot::FullBody, "HangTurnJump", 1.0, 0.2, 0.2, false, false);
        }
    }

    /// TdMove_GrabTransfer.StopMove.
    pub fn grab_transfer_stop_move(&mut self, m: Move) {
        self.pawn.gravity_modifier = 1.0;
        self.physics_move_stop_move(m);
    }

    /// TdMove_GrabTransfer.ReachedPreciseLocation.
    pub fn grab_transfer_reached_precise_location(&mut self, _m: Move) {
        self.pawn.velocity = Vec3::ZERO;
        self.pawn.acceleration = Vec3::ZERO;
        self.pawn.location = self.moves.grab_transfer.transfer_location;
        let next = self.moves.grab_transfer.transfer_move;
        if next == Move::IntoClimb {
            let l = self.moves.grab_transfer.transfer_ladder;
            self.moves.into_climb.ladder = l;
            self.moves.climb.ladder = l;
            if self.can_do_move(Move::IntoClimb) {
                self.set_move(Move::IntoClimb, false, false);
                self.pawn.active_movement_volume = None;
            } else {
                self.moves.into_climb.ladder = None;
                self.moves.climb.ladder = None;
                self.set_move(Move::Falling, false, false);
            }
        } else {
            self.set_move(next, false, false);
        }
    }

    /// TdMove_GrabTransfer.FailedToReachPreciseLocation / HitWall.
    pub fn grab_transfer_fall(&mut self, _m: Move) {
        self.set_move(Move::Falling, false, false);
    }
}

#[allow(dead_code)]
fn _physics(_: Physics) {}
