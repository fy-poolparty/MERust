//! TdMOVE_Disarm: snatching an enemy's gun (GBA_SwitchWeapon / RMB), and the controller side
//! of that button (TdPlayerController.SwitchWeapon / SnatchAttempt).
//!
//! From behind it always works; from the front only while the enemy's swing has been going
//! for its DisarmWindow (TdAIController.QueryDisarmState). Otherwise Faith grabs at nothing
//! (SnatchFail).

use crate::bots::DisarmState;
use crate::math::{norm_axis, Rotator, UeVec, Vec3};
use crate::moves::PreciseMode;
use crate::pawn::{Move, MoveAction, Slot};
use crate::sim::Sim;

#[derive(Clone, Debug, Default)]
pub struct Disarm {
    pub target: Option<usize>,
    pub state: Option<DisarmState>,
    pub anim: &'static str,
    pub offset: f32,
    pub target_location: Vec3,
    pub target_rotation: Rotator,
    pub force_miss: bool,
    pub move_enemy: bool,
    /// The weapon class taken (TakeDisarmedPawnsWeapon).
    pub took: bool,
}

/// TdMOVE_Disarm.DisarmOffset after ChooseDisarmType.
const DISARM_OFFSET: f32 = 125.899;

impl Sim {
    /// TdPlayerController.SwitchWeapon: drop the gun, else SnatchAttempt, else pick one up,
    /// else a forced-miss disarm.
    pub fn switch_weapon_press(&mut self) {
        let ms = self.pawn.movement_state;
        // AgainstWallState, firing, Jump / Landing, and busy moves block it (and the dead)
        if self.pawn.dying
            || self.pawn.against_wall_state != crate::body::AgainstWall::None
            || self.weapon.as_ref().is_some_and(|w| w.firing)
            || matches!(ms, Move::Jump | Move::Landing)
            || self.moves.base(ms).movement_group > 0
        {
            return;
        }
        if self.weapon.is_some() {
            self.drop_weapon();
            return;
        }
        if self.snatch_attempt() {
            return;
        }
        if matches!(ms, Move::Walking | Move::Crouch | Move::Jump | Move::Slide) && self.try_pick_up_weapon() {
            return;
        }
        self.moves.disarm.force_miss = true;
        self.handle_move_action(MoveAction::Snatch);
        self.moves.disarm.force_miss = false;
    }

    /// TdPlayerController.SnatchAttempt.
    fn snatch_attempt(&mut self) -> bool {
        if self.pawn.movement_state == Move::Snatch || self.weapon.is_some() {
            return false;
        }
        let fwd = self.pawn.velocity.dot(self.pawn.rotation.vector());
        let range = (fwd.max(0.0) * 0.4).max(170.0).min(360.0);
        let Some(t) = self.get_close_combat_target(range, 0.7) else { return false };
        self.pc.target_pawn = Some(t);
        let b = &self.bots[t];
        let in_front = (b.location - self.pawn.location).safe_normal().dot(self.pawn.rotation.vector()) >= 0.0;
        if b.movement_state == crate::bots::BotMove::Stumble && !in_front {
            self.handle_move_action(MoveAction::Melee);
        } else {
            self.handle_move_action(MoveAction::Snatch);
        }
        true
    }

    /// TdPlayerController.GetHumanTarget(MaxDistance, MaxAngle): the nearest live enemy
    /// within MaxAngle of the view, if it's within MaxDistance (2D) and CloseCombatMaxRange.
    fn get_close_combat_target(&self, max_distance: f32, max_angle: f32) -> Option<usize> {
        let view = self.pc.rotation.vector();
        let p = self.pawn.location;
        let mut best: (f32, Option<usize>) = (1.0e7, None);
        for (i, b) in self.bots.iter().enumerate() {
            if !b.alive() || !self.bot_visible_to_player(i) {
                continue;
            }
            if view.dot((b.location - p).safe_normal()) > max_angle {
                let d = (p - b.location).length();
                if d < best.0 {
                    best = (d, Some(i));
                }
            }
        }
        let i = best.1?;
        let b = &self.bots[i];
        let d2 = Vec3::new(p.x - b.location.x, p.y - b.location.y, 0.0).length();
        (d2 < max_distance && (p - b.location).length() < 360.0).then_some(i)
    }

    /// TdMOVE_Disarm.CanDoMove.
    pub fn disarm_can_do_move(&mut self, m: Move) -> bool {
        if !self.tdmove_can_do_move(m) || self.weapon.is_some() {
            return false;
        }
        if self.moves.disarm.force_miss {
            self.moves.disarm.state = Some(DisarmState::Miss);
            self.moves.disarm.target = None;
            return true;
        }
        let Some(t) = self.pc.target_pawn else { return false };
        if t >= self.bots.len() || !self.bots[t].alive() {
            return false;
        }
        let b = &self.bots[t];
        if (b.location.z - self.pawn.location.z).abs() > 70.0 {
            return false;
        }
        // MovementTraceForBlockingBetweenActors
        if self.world.line_check(b.location, self.pawn.location, Vec3::ZERO).hit {
            return false;
        }
        let state = self.query_disarm_state(t);
        self.moves.disarm.target = Some(t);
        self.moves.disarm.state = Some(state);
        state != DisarmState::NotPossible
    }

    /// TdMOVE_Disarm.StartMove.
    pub fn disarm_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        self.moves.disarm.took = false;
        let miss = self.moves.disarm.force_miss || self.moves.disarm.state == Some(DisarmState::Miss);
        if miss {
            self.moves.disarm.target_location = self.pawn.location;
            self.moves.disarm.target_rotation = self.pawn.rotation;
            // StartMiss
            self.pawn.first_person_dpg = crate::pawn::Dpg::Foreground;
            self.use_root_motion(true);
            self.play_move_anim(m, Slot::FullBody, "SnatchFail", 1.0, 0.1, 0.4, true, false);
            self.moves.disarm.force_miss = false;
            if self.moves.base(m).current_custom_anim.is_none() {
                self.set_move(Move::Walking, false, false);
            }
            return;
        }
        let t = self.moves.disarm.target.unwrap();
        self.reset_camera_look(m, 0.2);
        let b = &self.bots[t];
        let mut to = b.location - self.pawn.location;
        to.z = 0.0;
        let to = to.safe_normal();
        let to_rot = Rotator::from_vector(to);
        // ChooseDisarmType
        let facing_each_other = self.pawn.rotation.vector().dot(b.rotation.vector()) <= 0.0;
        let yaw_offset = if facing_each_other { 32768 } else { 0 };
        self.moves.disarm.offset = DISARM_OFFSET;
        let l = self.bots[t].loadout;
        let patrol_light = l.body.package == "CH_TKY_Cop_Patrol" && l.weapon.is_some_and(|w| !w.heavy);
        let steyr = l.weapon.is_some_and(|w| w.name == "TdWeapon_SMG_SteyrTMP");
        self.moves.disarm.anim = if facing_each_other {
            if !patrol_light {
                "SnatchFwd"
            } else if steyr {
                // the Steyr: SnatchFwd2 or 3
                if self.frand() < 0.5 { "SnatchFwd2" } else { "SnatchFwd3" }
            } else {
                // a patrol cop with a light weapon: one of the three
                match (self.frand() * 3.0) as i32 {
                    0 => "SnatchFwd",
                    1 => "SnatchFwd2",
                    _ => "SnatchFwd3",
                }
            }
        } else {
            "SnatchBack"
        };
        if !self.bot_start_canned_disarm(t) {
            self.set_move(Move::Walking, false, false);
            return;
        }
        self.moves.disarm.move_enemy = false;
        let b = &self.bots[t];
        let target_location = b.location - to * self.moves.disarm.offset;
        let target_rotation = Rotator::new(0, to_rot.yaw, 0);
        self.moves.disarm.target_location = target_location;
        self.moves.disarm.target_rotation = target_rotation;
        let look = Rotator::new(0, norm_axis(target_rotation.yaw + yaw_offset), 0);
        let step = Vec3::new(0.0, 0.0, self.pawn.max_step_height);
        let ext = Vec3::new(self.pawn.collision_radius, self.pawn.collision_radius, self.pawn.collision_height);
        if (target_location.z - self.pawn.location.z).abs() > 3.0 {
            // HandleHeightDifference
            if b.location.z > self.pawn.location.z {
                let tl = b.location - Rotator::new(0, norm_axis(b.rotation.yaw + yaw_offset), 0).vector() * self.moves.disarm.offset;
                self.moves.disarm.target_location = tl;
                self.set_precise_location(m, tl, PreciseMode::Fly, 300.0);
            } else {
                self.disarm_move_enemy(t, look);
            }
        } else if self.world.line_check(target_location + step, self.pawn.location + step, ext).hit {
            // HandlePlayerUnableToMove
            self.disarm_move_enemy(t, look);
        } else {
            // AlignPawn
            let speed = self.pawn.velocity.size_2d().max(400.0);
            self.set_precise_location(m, target_location, PreciseMode::Walk, speed);
            self.bot_set_look_at_direction(t, look);
        }
        self.set_precise_rotation(m, target_rotation, 0.2);
        self.pawn.velocity = Vec3::ZERO;
        self.pawn.acceleration = Vec3::ZERO;
        // PlayDisarmStart: TakeDisarmedPawnsWeapon, then the snatch on both
        let class = self.bots[t].weapon.as_ref().map(|w| w.class).unwrap_or(&crate::weapons::GLOCK18C);
        let ammo = self.bots[t].loadout.ammo_disarmed;
        self.give_weapon(class, ammo);
        // CreateInventory(WeaponClass, bDoNotActivate): not Pawn.Weapon until StopMove
        if let Some(w) = self.weapon.as_mut() {
            w.current = false;
        }
        // TakeDisarmedPawnsWeapon: SetWeaponAnimState(WS_Unarmed) until the move ends
        self.set_weapon_anim_state(crate::weapons::WeaponAnimState::Unarmed);
        self.moves.disarm.took = true;
        // the view locks onto the disarmed pawn (TargetingPawn, UpdateMeleeAutoLockOn)
        self.pc.targeting_pawn = Some(t);
        self.pc.targeting_pawn_interp = 0.0;
        let anim = self.moves.disarm.anim;
        self.play_move_anim(m, Slot::Canned, anim, 1.0, 0.1, 0.0, false, false);
        self.bot_play_disarm_start(t, anim);
        if self.moves.base(m).current_custom_anim.is_none() {
            self.set_move(Move::Walking, false, false);
        }
    }

    /// The enemy steps to the player instead (TdMove_Disarmed.SetPreciseLocation).
    fn disarm_move_enemy(&mut self, t: usize, look: Rotator) {
        self.moves.disarm.move_enemy = true;
        let to = self.pawn.location + self.pawn.rotation.vector() * self.moves.disarm.offset;
        let b = &mut self.bots[t];
        b.location.x = to.x;
        b.location.y = to.y;
        self.bot_set_look_at_direction(t, look);
    }

    /// TdMOVE_Disarm.StopMove.
    pub fn disarm_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        // TdMove.StopMove: AimMode NoHands with a weapon -> SetArmed (relaxed); then
        // SetCurrentWeapon(DisarmedWeapon) -> PlayWeaponSwitch -> PlayWeaponDeploy (ready)
        if let Some(w) = self.weapon.as_mut() {
            // SetCurrentWeapon: Activate -> WeaponEquipping
            w.current = true;
            w.equipping = w.class.equip_time;
            self.set_weapon_anim_state(crate::weapons::WeaponAnimState::Relaxed);
            self.play_weapon_deploy();
        }
        self.pc.targeting_pawn = None;
        self.moves.disarm.target = None;
        self.moves.disarm.took = false;
        self.use_root_motion(false);
    }

    /// TdMOVE_Disarm.ReachedPreciseLocation.
    pub fn disarm_reached_precise_location(&mut self, _m: Move) {
        self.pawn.location = self.moves.disarm.target_location;
        self.pawn.velocity = Vec3::ZERO;
        self.pawn.acceleration = Vec3::ZERO;
    }

    /// TdMOVE_Disarm.FailedToReachPreciseLocation -> AbortDisarm.
    pub fn disarm_failed_precise_location(&mut self, _m: Move) {
        if let Some(t) = self.moves.disarm.target {
            // TdMove_Disarmed.AbortDisarm
            self.bots[t].anim.stop(Slot::Canned, 0.2);
        }
        self.weapon = None;
        self.update_anim_sets();
        self.anim.stop(Slot::Canned, 0.2);
        self.set_move(Move::Walking, false, false);
    }
}
