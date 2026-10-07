//! The player's unarmed attacks (TdMove_MeleeBase and its subclasses) and getting hit
//! (TdMove_Stumble).
//!
//! The punch combo (Melee), jump kick (MeleeAir), slide kick (MeleeSlide), wall-run kick
//! (MeleeWallrun) and crouch punch (MeleeCrouch) target the best enemy in front
//! (GetMeleeTarget) and either test the hit when their wind-up ends (TestHit) or sweep a box
//! from a bone every tick (UTdMove_MeleeBase vt70, 0x1208A10) and deliver the damage on contact
//! (TriggerDamage).

use crate::combat::{DamageType, StumbleHit, StumbleState};
use crate::config::Config;
use crate::math::{Rotator, UeVec, Vec3};
use crate::moves::{class_of, Class};
use crate::pawn::{Move, MoveAction, Slot};
use crate::sim::Sim;

/// TdMove_MeleeBase.EMeleeState.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MeleeState {
    #[default]
    Pending = 0,
    AttackNormal = 1,
    AttackFinishing = 2,
    HitNormal = 3,
    HitFinishing = 4,
    MissNormal = 5,
    MissFinishing = 6,
}

/// TdMove_Melee.EMoveMeleeType / TdMove_MeleeAir.EMeleeAirType.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MeleeType {
    #[default]
    Normal,
    AtBlockingEnemy,
    AtBentOverEnemy,
    AtComboFinisher,
}

/// One melee move's TdMove_MeleeBase variables.
#[derive(Clone, Debug)]
pub struct MeleeBase {
    pub state: MeleeState,
    pub targeting: bool,
    pub hit_detection: bool,
    pub target: Option<usize>,
    pub targeting_max_distance: f32,
    pub hit_detection_start: Vec3,
    pub hit_detection_last_start: Vec3,
    pub hit_detection_bone: &'static str,
    pub trace_offset: Vec3,
    pub trace_extent: Vec3,
    pub damage: f32,
}

impl MeleeBase {
    fn new(cfg: &Config, class: &str, damage: f32, extent: f32, targeting: bool, max_dist: f32) -> Self {
        let c = &[class, "TdMove_MeleeBase"];
        MeleeBase {
            state: MeleeState::Pending,
            targeting,
            hit_detection: false,
            target: None,
            targeting_max_distance: max_dist,
            hit_detection_start: Vec3::ZERO,
            hit_detection_last_start: Vec3::ZERO,
            hit_detection_bone: "RightHand",
            trace_offset: Vec3::ZERO,
            trace_extent: Vec3::new(extent, extent, extent),
            damage: cfg.f32(c, "MeleeDamage", damage),
        }
    }
}

pub struct Melee {
    pub base: MeleeBase,
    pub left: bool,
    pub window_open: bool,
    pub combo_counter: i32,
    pub combo_queued_actions: i32,
    pub melee_type: MeleeType,
    pub blend_in_missed: f32,
    pub blend_out_missed: f32,
}

pub struct MeleeAir {
    pub base: MeleeBase,
    pub air_type: u8,
    pub impact_momentum: Vec3,
    pub look_at_angle: Rotator,
    pub min_angle: f32,
    pub min_separation: f32,
    pub max_separation: f32,
}

/// TdMove_MeleeAirAbove: landing on an enemy from above (MarioMove).
pub struct MeleeAirAbove {
    pub base: MeleeBase,
}

pub struct MeleeSlide {
    pub base: MeleeBase,
}

pub struct MeleeWallrun {
    pub base: MeleeBase,
    pub left: bool,
}

pub struct MeleeCrouch {
    pub base: MeleeBase,
}

/// TdMove_Stumble (the player's).
pub struct Stumble {
    pub hit: StumbleHit,
    pub instigator: Option<usize>,
    pub state: StumbleState,
    pub in_air: bool,
    pub current: &'static str,
}

/// All the player's melee move objects.
pub struct MeleeMoves {
    pub melee: Melee,
    pub air: MeleeAir,
    pub air_above: MeleeAirAbove,
    pub slide: MeleeSlide,
    pub wallrun: MeleeWallrun,
    pub crouch: MeleeCrouch,
}

impl MeleeMoves {
    pub fn new(cfg: &Config) -> Self {
        let mut wr = MeleeBase::new(cfg, "TdMove_MeleeWallrun", 80.0, 30.0, false, 300.0);
        wr.trace_offset = Vec3::new(30.0, 0.0, 0.0);
        MeleeMoves {
            melee: Melee {
                base: MeleeBase::new(cfg, "TdMove_Melee", 33.5, 12.0, true, 300.0),
                left: false,
                window_open: false,
                combo_counter: 0,
                combo_queued_actions: 0,
                melee_type: MeleeType::Normal,
                blend_in_missed: cfg.f32(&["TdMove_Melee"], "BlendInMissed", 0.08),
                blend_out_missed: cfg.f32(&["TdMove_Melee"], "BlendOutMissed", 0.1),
            },
            air: MeleeAir {
                base: {
                    let mut b = MeleeBase::new(cfg, "TdMove_MeleeAir", 100.0, 60.0, true, 800.0);
                    b.trace_extent = Vec3::new(60.0, 60.0, 160.0);
                    b
                },
                air_type: 0,
                impact_momentum: Vec3::ZERO,
                look_at_angle: Rotator::new(-6000, 0, 0),
                min_angle: cfg.f32(&["TdMove_MeleeAir"], "MeleeAirAboveMinAngle", 0.8),
                min_separation: cfg.f32(&["TdMove_MeleeAir"], "MeleeAirAboveMinSeparation", 150.0),
                max_separation: cfg.f32(&["TdMove_MeleeAir"], "MeleeAirAboveMaxSeparation", 500.0),
            },
            // bTargeting=false, MeleeDamage=300
            air_above: MeleeAirAbove { base: MeleeBase::new(cfg, "TdMove_MeleeAirAbove", 300.0, 5.0, false, 300.0) },
            slide: MeleeSlide { base: MeleeBase::new(cfg, "TdMove_MeleeSlide", 60.0, 40.0, true, 300.0) },
            wallrun: MeleeWallrun { base: wr, left: false },
            crouch: MeleeCrouch { base: MeleeBase::new(cfg, "TdMove_MeleeCrouch", 33.5, 30.0, true, 300.0) },
        }
    }

    pub fn base(&self, m: Move) -> Option<&MeleeBase> {
        Some(match m {
            Move::Melee => &self.melee.base,
            Move::MeleeAir => &self.air.base,
            Move::MeleeAirAbove => &self.air_above.base,
            Move::MeleeSlide => &self.slide.base,
            Move::MeleeWallrun => &self.wallrun.base,
            Move::MeleeCrouch => &self.crouch.base,
            _ => return None,
        })
    }

    pub fn base_mut(&mut self, m: Move) -> Option<&mut MeleeBase> {
        Some(match m {
            Move::Melee => &mut self.melee.base,
            Move::MeleeAir => &mut self.air.base,
            Move::MeleeAirAbove => &mut self.air_above.base,
            Move::MeleeSlide => &mut self.slide.base,
            Move::MeleeWallrun => &mut self.wallrun.base,
            Move::MeleeCrouch => &mut self.crouch.base,
            _ => return None,
        })
    }
}

impl Stumble {
    pub fn new() -> Self {
        Stumble { hit: StumbleHit::default(), instigator: None, state: StumbleState::HitNone, in_air: false, current: "" }
    }
}

/// SetMoveTimer ids.
const MELEE_TIMER: u8 = 0;

/// Moves[m] is a TdMove_MeleeBase (IsA('TdMove_MeleeBase')).
pub fn is_melee_move(m: Move) -> bool {
    matches!(m, Move::Melee | Move::MeleeAir | Move::MeleeSlide | Move::MeleeWallrun | Move::MeleeCrouch | Move::MeleeAirAbove | Move::MeleeBarge | Move::MeleeVault)
}

impl Sim {
    fn mb(&mut self, m: Move) -> &mut MeleeBase {
        self.moves.melee.base_mut(m).expect("not a melee move")
    }

    /// A bot's location (the target pawn's).
    fn bot_location(&self, i: Option<usize>) -> Option<Vec3> {
        i.map(|i| self.bots[i].location)
    }

    // ------------------------------------------------------------------ TdMove_MeleeBase

    /// TdMove_MeleeBase.CanDoMove.
    fn melee_base_can_do_move(&mut self, m: Move) -> bool {
        self.tdmove_can_do_move(m)
    }

    /// TdMove_MeleeBase.StartMove.
    fn melee_base_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        let d = self.mb(m).targeting_max_distance;
        let t = self.get_melee_target(d * 3.0);
        let b = self.mb(m);
        b.target = t;
        b.state = MeleeState::AttackNormal;
    }

    /// TdMove_MeleeBase.StopMove.
    pub(crate) fn melee_base_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.anim.stop(Slot::Canned, 0.15);
        self.anim.stop(Slot::FullBody, 0.15);
        self.anim.stop(Slot::Weapon, 0.15);
        if let Some(t) = self.mb(m).target {
            self.pc.targeting_pawn = Some(t);
            self.pc.targeting_pawn_interp = 0.0;
        }
    }

    /// TdMove_MeleeBase.DeliverDamage.
    fn melee_deliver_damage(&mut self, m: Move, damage: f32, hit_location: Vec3, momentum: Vec3, t: DamageType) {
        if let Some(i) = self.mb(m).target {
            self.bot_take_damage(i, damage as i32, hit_location, momentum, t);
        }
    }

    /// UTdMove_MeleeBase vt70 (0x1208A10): the target, then the bone sweep.
    pub fn melee_base_tick(&mut self, m: Move) {
        let b = self.mb(m);
        if b.targeting && b.target.is_none() {
            let d = b.targeting_max_distance;
            let t = self.get_melee_target(d);
            self.mb(m).target = t;
        }
        if !self.mb(m).hit_detection {
            return;
        }
        let bone = self.mb(m).hit_detection_bone;
        let off = self.mb(m).trace_offset;
        let (x, y, z) = self.pawn.rotation.axes();
        let start = self.bone_location(bone) + x * off.x + y * off.y + z * off.z;
        let last = self.mb(m).hit_detection_last_start;
        // the sweep direction, kept within 35 degrees of the facing (0x11F2EB0)
        let facing = self.pawn.rotation.vector();
        let mut dir = (start - last).safe_normal();
        let cone = 35.0f32.to_radians();
        let d = dir.dot(facing);
        if d < cone {
            dir = (facing + (dir - facing * d) * cone.sin()).safe_normal();
        }
        dir = dir.safe_normal();
        let end = start + dir * 10.0;
        let ext = self.mb(m).trace_extent;
        let b = self.mb(m);
        b.hit_detection_start = start;
        b.hit_detection_last_start = start;
        if let Some(i) = self.trace_bots(start, end, ext) {
            self.melee_trigger_damage(m, i);
        }
    }

    /// SingleLineCheck(TRACE_Pawns): the first live bot a box sweep touches.
    fn trace_bots(&self, start: Vec3, end: Vec3, ext: Vec3) -> Option<usize> {
        let mut best = (f32::MAX, None);
        for (i, b) in self.bots.iter().enumerate() {
            if !b.alive() {
                continue;
            }
            // segment to the cylinder grown by the extent
            let r = b.collision_radius + ext.x.max(ext.y);
            let h = b.collision_height + ext.z;
            for k in 0..=4 {
                let p = start + (end - start) * (k as f32 / 4.0);
                let d = p - b.location;
                if (d.x * d.x + d.y * d.y).sqrt() <= r && d.z.abs() <= h {
                    let t = k as f32;
                    if t < best.0 {
                        best = (t, Some(i));
                    }
                    break;
                }
            }
        }
        best.1
    }

    /// TdPawn.Mesh.GetBoneLocation for the hit detection bone, from the posed mesh when the
    /// presentation layer supplies it.
    fn bone_location(&self, bone: &str) -> Vec3 {
        if let Some((n, at)) = &self.hit_bone_world {
            if n.eq_ignore_ascii_case(bone) {
                return *at;
            }
        }
        let (x, y, _) = self.pawn.rotation.axes();
        let side = if bone.starts_with("Left") { -15.0 } else { 15.0 };
        let up = if bone.contains("Hand") { 40.0 } else if bone.contains("Foot") { -40.0 } else { -20.0 };
        self.pawn.location + x * 50.0 + y * side + Vec3::new(0.0, 0.0, up)
    }

    /// The class's TriggerDamage event.
    fn melee_trigger_damage(&mut self, m: Move, victim: usize) {
        match class_of(m) {
            Class::MeleeAir => self.melee_air_trigger_damage(m, victim),
            Class::MeleeSlide => self.melee_slide_trigger_damage(m, victim),
            Class::MeleeWallrun => self.melee_wallrun_trigger_damage(m, victim),
            Class::MeleeCrouch => self.melee_crouch_trigger_damage(m, victim),
            _ => {}
        }
    }

    /// TdMove_MeleeBase.UpdateTargetPawn: the target gets ready (bots don't block yet).
    /// TdMove_MeleeBase.UpdateTargetPawn: TargetPawn.PrepareForMeleeAttack(GetDamageType()).
    fn melee_update_target(&mut self, m: Move) {
        let Some(t) = self.moves.melee.base(m).and_then(|b| b.target) else { return };
        let ty = match m {
            Move::Melee => {
                let mm = &self.moves.melee.melee;
                if mm.melee_type == MeleeType::AtBentOverEnemy { DamageType::MeleeSoccerKick } else if mm.left { DamageType::MeleeLeft } else { DamageType::MeleeRight }
            }
            Move::MeleeAir => DamageType::MeleeAir,
            Move::MeleeSlide => DamageType::MeleeSlide,
            Move::MeleeCrouch => DamageType::MeleeCrouch,
            _ => return,
        };
        self.bot_prepare_for_melee_attack(t, ty);
    }

    /// (dot of the 2D/3D direction to the target with the facing, distance)
    fn melee_to_target(&self, m: Move, flat: bool) -> Option<(f32, f32)> {
        let t = self.bot_location(self.moves.melee.base(m)?.target)?;
        let mut d = t - self.pawn.location;
        let dist = d.length();
        if flat {
            d.z = 0.0;
        }
        Some((d.safe_normal().dot(self.pawn.rotation.vector()), dist))
    }

    fn set_target_death_anim(&mut self, m: Move, t: u8) {
        if let Some(i) = self.mb(m).target {
            self.bots[i].active_death_anim_type = t;
        }
    }

    /// TdMove.UpdateMeleeAutoLockOn (from the melee moves' UpdateViewRotation): the view swings
    /// onto the targeted enemy (mouse input: the stick checks never fire).
    pub fn melee_auto_lock_on(&mut self, dt: f32, view: Rotator, delta: &mut Rotator) {
        if self.pc.targeting_pawn.is_none() {
            self.pc.targeting_pawn = self.get_human_target(90.0, self.pc.close_combat_max_angle);
        }
        let Some(t) = self.pc.targeting_pawn else { return };
        let b = &self.bots[t];
        let soft = self.pc.soft_lock_strength;
        if soft == 0.0 {
            self.pc.targeting_pawn = None;
            return;
        }
        let p = self.pawn.location;
        if (b.location - p).length() > 350.0 || (b.location.z - p.z).abs() > 150.0 || !b.alive() {
            self.pc.targeting_pawn = None;
            self.pc.targeting_pawn_interp = 0.0;
            return;
        }
        let to = Rotator::from_vector(b.location - p).normalize();
        let to_delta = (to - self.pc.rotation.normalize()).normalize().yaw as f32;
        if to_delta.abs() < 100.0 {
            self.pc.targeting_pawn_interp = 0.0;
            if to_delta.abs() > self.pc.targeting_cutoff_angle {
                self.pc.targeting_pawn = None;
                return;
            }
            let k = (to_delta.abs() / self.pc.targeting_cutoff_angle).powf(soft).clamp(0.1, 1.0);
            delta.yaw = (delta.yaw as f32 * k) as i32;
            return;
        }
        self.pc.targeting_pawn_interp = (self.pc.targeting_pawn_interp + 2.0 * dt).min(1.0);
        let focus = Rotator::from_vector(b.location - p);
        let target = if self.pc.targeting_pawn_interp == 1.0 {
            focus
        } else {
            // RLerp(.., bShortestPath)
            let a = self.pc.rotation.normalize();
            let dy = crate::math::norm_axis(focus.yaw - a.yaw);
            Rotator::new(a.pitch, a.yaw + (dy as f32 * self.pc.targeting_pawn_interp) as i32, a.roll)
        };
        delta.yaw = crate::math::norm_axis(target.yaw - view.yaw);
    }

    /// TdPlayerController.GetHumanTarget: the nearest enemy within MaxAngle of the view.
    pub fn get_human_target(&self, _max_distance: f32, max_angle: f32) -> Option<usize> {
        let view = self.pc.rotation.vector();
        let p = self.pawn.location;
        let mut best = (f32::MAX, None);
        for (i, b) in self.bots.iter().enumerate() {
            if !b.alive() {
                continue;
            }
            if view.dot((b.location - p).safe_normal()) > max_angle {
                let d = (p - b.location).length();
                if d < best.0 {
                    best = (d, Some(i));
                }
            }
        }
        best.1
    }

    // ------------------------------------------------------------------ TdMove_Melee

    /// TdMove_Melee.CanDoMove.
    pub fn melee_can_do_move(&mut self, m: Move) -> bool {
        if !self.melee_base_can_do_move(m) {
            return false;
        }
        self.pawn.against_wall_state == crate::body::AgainstWall::None
    }

    /// TdMove_Melee.StartMove.
    pub fn melee_start_move(&mut self, m: Move) {
        self.melee_base_start_move(m);
        self.melee_reset();
        let legs = crate::math::norm_axis(self.pawn.leg_rotation - self.pawn.rotation.yaw) as f32;
        let mm = &mut self.moves.melee.melee;
        if mm.base.state == MeleeState::AttackNormal && legs.abs() > 4000.0 {
            mm.left = legs > 0.0;
        } else {
            mm.left = !mm.left;
        }
        self.melee_trigger_move(m);
    }

    fn melee_reset(&mut self) {
        let mm = &mut self.moves.melee.melee;
        mm.combo_counter = 0;
        mm.combo_queued_actions = 0;
        self.melee_close_window(Move::Melee);
    }

    fn melee_open_window(&mut self, m: Move, delay: f32) {
        self.set_move_countdown(m, delay);
        self.moves.melee.melee.window_open = true;
    }

    fn melee_close_window(&mut self, m: Move) {
        self.moves.base_mut(m).timer = 0.0;
        self.moves.melee.melee.window_open = false;
    }

    /// TdMove_Melee.HandleMoveAction: another press in the window queues the next swing.
    pub fn melee_handle_move_action(&mut self, _m: Move, a: MoveAction) {
        let mm = &mut self.moves.melee.melee;
        if a != MoveAction::Melee || !mm.window_open {
            return;
        }
        if mm.combo_counter < 2 {
            mm.combo_counter += 1;
            mm.combo_queued_actions += 1;
        }
    }

    /// TdMove_Melee.StopMove.
    pub fn melee_stop_move(&mut self, m: Move) {
        self.melee_base_stop_move(m);
        self.melee_reset();
        self.anim.stop(Slot::UpperBody, 0.3);
        self.anim.stop(Slot::FullBody, 0.3);
    }

    /// TdMove_Melee.OnTimer: the combo window closes.
    pub fn melee_on_timer(&mut self, m: Move) {
        self.melee_close_window(m);
    }

    /// TdMove_Melee.OnCustomAnimEnd.
    pub fn melee_on_custom_anim_end(&mut self, m: Move) {
        match self.moves.melee.melee.base.state {
            MeleeState::AttackNormal | MeleeState::AttackFinishing => {
                if self.melee_test_hit(m) {
                    self.melee_trigger_hit(m);
                } else {
                    self.melee_trigger_miss(m);
                }
            }
            MeleeState::HitNormal | MeleeState::HitFinishing | MeleeState::MissNormal | MeleeState::MissFinishing => {
                if self.moves.melee.melee.combo_queued_actions > 0 {
                    // super.StartMove() (TdMove_MeleeBase: no Reset, no side pick; UELib prints
                    // it as StartMove(), but Reset there would zero the queue it decrements next
                    // and the ComboCounter == 2 finisher could never happen), then the other arm
                    self.melee_base_start_move(m);
                    let mm = &mut self.moves.melee.melee;
                    mm.combo_queued_actions -= 1;
                    mm.left = !mm.left;
                    self.melee_trigger_move(m);
                } else {
                    self.set_move(Move::Walking, false, false);
                }
            }
            MeleeState::Pending => {}
        }
    }

    /// TdMove_Melee.TriggerMove.
    fn melee_trigger_move(&mut self, m: Move) {
        self.melee_open_window(m, 0.33);
        self.moves.melee.melee.base.targeting = true;
        self.anim.stop(Slot::Camera, 0.05);
        self.anim.stop(Slot::Weapon, 0.1);
        self.set_animation_movement_state(Move::Walking, 0.0);
        let target = self.moves.melee.melee.base.target;
        // TargetPawn IsA TdBotPawn_Assault (SWAT, sniper) or TdBotPawn_PatrolCop (its Steyr /
        // Remington subclasses too): not the support cop or the dummy
        let bent_over = target.is_some_and(|i| {
            let b = &self.bots[i];
            let class_ok = b.loadout.weapon.is_some() && b.loadout.body.package != "CH_TKY_Cop_Support";
            class_ok && b.movement_state == crate::bots::BotMove::Stumble && matches!(b.stumble_state, StumbleState::HitMeleeAirBodyFront | StumbleState::HitMeleeSlideFront)
        });
        let left = self.moves.melee.melee.left;
        if bent_over {
            self.reset_camera_look(m, 0.4);
            self.play_move_anim(m, Slot::FullBody, "MeleeBentOverStart", 1.0, 0.1, -1.0, false, false);
            let mm = &mut self.moves.melee.melee;
            mm.base.hit_detection_bone = "RightFoot";
            mm.left = false;
            mm.melee_type = MeleeType::AtBentOverEnemy;
            self.set_target_death_anim(m, 8);
        } else {
            let mm = &self.moves.melee.melee;
            if mm.combo_queued_actions == 0 && mm.combo_counter == 2 {
                self.moves.melee.melee.melee_type = MeleeType::AtComboFinisher;
                self.play_move_anim(m, Slot::UpperBody, "MeleeStartShove", 1.0, 0.1, -1.0, false, false);
                self.set_ignore_move_input(1.0);
                self.set_target_death_anim(m, 1);
            } else {
                self.moves.melee.melee.melee_type = MeleeType::Normal;
                self.play_move_anim(m, Slot::UpperBody, if left { "MeleeStartLeft" } else { "MeleeStartRight" }, 1.5, 0.1, -1.0, false, false);
                self.set_target_death_anim(m, if left { 8 } else { 6 });
            }
            self.moves.melee.melee.base.hit_detection_bone = if left { "LeftHand" } else { "RightHand" };
        }
        self.melee_update_target(m);
    }

    /// TdMove_Melee.TriggerMiss.
    fn melee_trigger_miss(&mut self, m: Move) {
        self.moves.melee.melee.base.targeting = false;
        self.set_animation_movement_state(Move::Walking, 0.0);
        let mm = &self.moves.melee.melee;
        let (left, queued) = (mm.left, mm.combo_queued_actions > 0);
        let (bi, bo) = (mm.blend_in_missed, if queued { 0.6 } else { mm.blend_out_missed });
        match mm.melee_type {
            MeleeType::Normal => self.play_move_anim(m, Slot::UpperBody, if left { "MeleeMissedLeft" } else { "MeleeMissedRight" }, 1.5, bi, bo, false, false),
            MeleeType::AtBlockingEnemy => self.play_move_anim(m, Slot::UpperBody, if left { "MeleeMissed2Left" } else { "MeleeMissed2Right" }, 1.5, bi, bo, false, false),
            MeleeType::AtBentOverEnemy => self.play_move_anim(m, Slot::FullBody, "MeleeBentOverFinish", 1.0, 0.1, 0.1, false, false),
            MeleeType::AtComboFinisher => {
                self.play_move_anim(m, Slot::UpperBody, "MeleeHitShove", 1.0, 0.0, 0.1, false, false);
                self.melee_reset();
            }
        }
        let b = &mut self.moves.melee.melee.base;
        b.state = if b.state == MeleeState::AttackNormal { MeleeState::MissNormal } else { MeleeState::MissFinishing };
    }

    /// TdMove_Melee.TriggerHit.
    fn melee_trigger_hit(&mut self, m: Move) {
        self.moves.melee.melee.base.targeting = false;
        self.set_animation_movement_state(Move::Walking, 0.0);
        let mm = &self.moves.melee.melee;
        let (left, queued) = (mm.left, mm.combo_queued_actions > 0);
        let bo = if queued { 0.3 } else { 0.1 };
        match mm.melee_type {
            MeleeType::Normal => self.play_move_anim(m, Slot::UpperBody, if left { "MeleeHitLeft" } else { "MeleeHitRight" }, 1.5, 0.2, bo, false, false),
            MeleeType::AtBlockingEnemy => self.play_move_anim(m, Slot::UpperBody, if left { "MeleeHit2Left" } else { "MeleeHit2Right" }, 1.5, 0.2, bo, false, false),
            MeleeType::AtBentOverEnemy => self.play_move_anim(m, Slot::FullBody, "MeleeBentOverFinish", 1.0, 0.1, 0.15, false, false),
            MeleeType::AtComboFinisher => {
                self.play_move_anim(m, Slot::UpperBody, "MeleeHitShove", 1.0, 0.0, 0.1, false, false);
                self.melee_reset();
            }
        }
        let b = &mut self.moves.melee.melee.base;
        b.state = if b.state == MeleeState::AttackNormal { MeleeState::HitNormal } else { MeleeState::HitFinishing };
    }

    /// TdMove_Melee.TestHit.
    fn melee_test_hit(&mut self, m: Move) -> bool {
        let Some((dot, dist)) = self.melee_to_target(m, true) else { return false };
        if !(dot > 0.8 && dist < 170.0) {
            return false;
        }
        let mm = &self.moves.melee.melee;
        let left = mm.left;
        let fwd = self.pawn.rotation.vector();
        let mut momentum = fwd * 150.0;
        let mut t = if left { DamageType::MeleeLeft } else { DamageType::MeleeRight };
        let mut damage = mm.base.damage;
        let hit = self.bone_location(if left { "LeftHand" } else { "RightHand" });
        match mm.melee_type {
            MeleeType::AtBentOverEnemy | MeleeType::AtComboFinisher => {
                // (Vector(Rotation) + (0,0,0.5)) * 200 + VRand() * 100
                momentum = (fwd + Vec3::new(0.0, 0.0, 0.5)) * 200.0;
                t = DamageType::MeleeSoccerKick;
                damage += if mm.melee_type == MeleeType::AtBentOverEnemy { 20.0 } else { 10.0 };
            }
            _ => {}
        }
        self.melee_deliver_damage(m, damage, hit, momentum, t);
        true
    }

    // ------------------------------------------------------------------ TdMove_MeleeAir

    /// TdMove_MeleeAir.CanDoMove.
    pub fn melee_air_can_do_move(&mut self, m: Move) -> bool {
        let ms = self.pawn.movement_state;
        if ms == Move::Jump && self.moves.base(Move::Jump).move_active_time < 0.1 {
            return false;
        }
        match ms {
            Move::IntoGrab | Move::Jump | Move::Falling | Move::WallRunJump | Move::GrabJump => {
                self.moves.melee.air.air_type = if self.pawn.velocity.size_2d() > 200.0 { 0 } else { 1 };
            }
            _ => return false,
        }
        self.melee_base_can_do_move(m)
    }

    /// TdMove_MeleeAir.StartMove.
    pub fn melee_air_start_move(&mut self, m: Move) {
        self.melee_base_start_move(m);
        let air = &self.moves.melee.air;
        if air.air_type == 0 {
            if let Some(t) = self.bot_location(air.base.target) {
                let to = (t - self.pawn.location).safe_normal();
                if to.dot(Vec3::new(0.0, 0.0, 1.0)) < -0.6 && self.pawn.velocity.z < 0.0 {
                    self.moves.melee.air.air_type = 2;
                }
                let sep = self.pawn.location.z - t.z;
                let air = &self.moves.melee.air;
                if to.z < -air.min_angle && sep > air.min_separation && sep < air.max_separation && self.pawn.velocity.z < 0.0 {
                    let t = self.moves.melee.air.base.target.unwrap();
                    // TargetBot.CanDoMove(MOVE_MeleeAirAbove)
                    if !self.bot_can_do_air_above(t) {
                        self.set_move(Move::Falling, false, false);
                        return;
                    }
                    self.moves.melee.air_above.base.target = Some(t);
                    self.set_move(Move::MeleeAirAbove, false, false);
                    return;
                }
            }
        }
        self.melee_air_trigger_move(m);
    }

    /// TdMove_MeleeAir.TriggerMove.
    fn melee_air_trigger_move(&mut self, m: Move) {
        match self.moves.melee.air.air_type {
            0 | 1 => {
                if self.moves.melee.air.air_type == 0 {
                    if let Some(t) = self.bot_location(self.moves.melee.air.base.target) {
                        let b = &self.bots[self.moves.melee.air.base.target.unwrap()];
                        let eye = t + Vec3::new(0.0, 0.0, b.base_eye_height);
                        self.set_look_at_target_location(m, eye - Vec3::new(0.0, 0.0, 20.0), 0.2, -1.0);
                    } else {
                        let a = self.pawn.rotation + self.moves.melee.air.look_at_angle;
                        self.set_look_at_target_angle(m, a, 0.2, -1.0);
                    }
                    self.play_move_anim(m, Slot::FullBody, "MeleeInAir", 1.0, 0.1, 0.2, false, false);
                    let air = &mut self.moves.melee.air;
                    air.base.hit_detection_bone = "RightFoot";
                    air.base.hit_detection = true;
                    air.impact_momentum = self.pawn.velocity * 1.6;
                } else {
                    self.play_move_anim(m, Slot::FullBody, "MeleeInAirStill", 1.0, 0.1, 0.2, false, false);
                    self.moves.melee.air.base.hit_detection_bone = "RightFoot";
                    self.set_move_countdown(m, 0.25);
                    self.pawn.velocity.z = 0.0;
                    self.moves.melee.air.impact_momentum = self.pawn.velocity * 1.6;
                }
                self.set_target_death_anim(m, 1);
            }
            _ => {
                self.play_move_anim(m, Slot::FullBody, "MeleeFromAbove", 1.0, 0.1, 0.1, false, false);
                self.moves.melee.air.base.hit_detection_bone = "LeftFoot";
                self.set_move_countdown(m, 0.15);
                self.moves.melee.air.impact_momentum = self.pawn.velocity * 1.6;
            }
        }
        self.melee_update_target(m);
    }

    /// TdMove_MeleeAir / MeleeSlide / MeleeWallrun.OnTimer: hit detection on.
    pub fn melee_hit_detection_on(&mut self, m: Move) {
        self.mb(m).hit_detection = true;
    }

    /// TdMove_MeleeAir.OnCustomAnimEnd.
    pub fn melee_air_on_custom_anim_end(&mut self, _m: Move) {
        self.set_move(Move::Falling, false, false);
        self.reset_camera_look(Move::Falling, 0.6);
    }

    /// TdMove_MeleeAir.Landed.
    pub fn melee_air_landed(&mut self, m: Move) {
        if self.moves.melee.air.air_type != 0 {
            return;
        }
        let fall = self.pawn.enter_falling_height - self.pawn.location.z;
        let hard = self.moves.landing.hard_landing_height;
        let detecting = self.moves.melee.air.base.hit_detection;
        if detecting && fall < hard {
            self.set_move(Move::Landing, false, false);
            self.set_move(Move::Walking, false, false);
            self.reset_camera_look(Move::Walking, 0.4);
            self.anim.stop(Slot::FullBody, 0.3);
            self.play_move_anim(m, Slot::FullBody, "MeleeInAirLand2", 1.0, 0.3, 0.3, false, false);
            let id = if self.pawn.velocity.z < -1000.0 { 9 } else { 8 };
            self.play_foot_step_sound(id);
            self.moves.melee.air.base.hit_detection = false;
        } else if detecting {
            self.moves.melee.air.base.hit_detection = false;
            if self.can_do_move(Move::Landing) {
                self.set_move(Move::Landing, false, false);
            }
        } else {
            self.set_move(Move::Walking, false, false);
            self.play_move_anim(m, Slot::FullBody, "MeleeInAirLand", 1.0, 0.1, 0.1, false, false);
            self.reset_camera_look(Move::Walking, 0.2);
        }
    }

    /// TdMove_MeleeAir.TriggerDamage.
    fn melee_air_trigger_damage(&mut self, m: Move, victim: usize) {
        let air = &self.moves.melee.air;
        if air.base.target != Some(victim) {
            return;
        }
        let verify = match air.air_type {
            0 | 1 => self.melee_to_target(m, false).is_some_and(|(dot, dist)| dot > 0.4 && dist < 140.0),
            _ => true,
        };
        if !verify {
            return;
        }
        let hit = self.bone_location("RightFoot");
        let speed = (self.get_average_speed(0.25) / 650.0).clamp(0.6, 1.0);
        let damage = self.moves.melee.air.base.damage * speed;
        let momentum = self.moves.melee.air.impact_momentum;
        self.melee_deliver_damage(m, damage, hit, momentum, DamageType::MeleeAir);
        let air_type = self.moves.melee.air.air_type;
        if air_type == 0 && self.moves.melee.air.base.state == MeleeState::AttackNormal {
            self.anim.set_blend_out(Slot::FullBody, 0.1);
            self.play_move_anim(m, Slot::FullBody, "MeleeInAirHit", 1.0, 0.1, 0.2, false, false);
        }
        if air_type == 2 || air_type == 0 {
            let t = self.bots[victim].location;
            self.pawn.velocity = (self.pawn.location - t).safe_normal() * 500.0;
            self.pawn.velocity.z = 50.0;
            self.pawn.acceleration = self.pawn.velocity.safe_normal();
        }
        if self.moves.melee.air.base.state == MeleeState::AttackFinishing {
            self.reset_camera_look(m, 0.2);
        }
        self.moves.melee.air.base.hit_detection = false;
    }

    // ------------------------------------------------------------------ TdMove_MeleeAirAbove

    /// TdMove_MeleeAirAbove.StartMove: the target starts its canned MeleeAirAboveBot, then
    /// the player flies onto its head (SetPreciseLocation, PreciseMode Jump, at the current
    /// speed) facing the way it faces.
    pub fn melee_air_above_start_move(&mut self, m: Move) {
        let Some(t) = self.moves.melee.air_above.base.target.filter(|&t| self.bots[t].alive()) else {
            self.set_move(Move::Falling, false, false);
            return;
        };
        // TdAIController.StartCannedMove(MOVE_MeleeAirAbove)
        if !self.bot_start_canned_air_above(t) {
            self.set_move(Move::Falling, false, false);
            return;
        }
        self.melee_base_start_move(m);
        self.moves.melee.air_above.base.target = Some(t);
        self.set_animation_movement_state(Move::Jump, 0.0);
        self.reset_camera_look(m, 0.2);
        let b = &self.bots[t];
        let target_location = b.location + Vec3::new(0.0, 0.0, b.collision_height + self.pawn.collision_height);
        let back = b.rotation.vector() * -1.0;
        let target_rotation = Rotator::new(0, Rotator::from_vector(back).yaw, 0);
        let speed = self.pawn.velocity.length();
        self.set_precise_location(m, target_location, crate::moves::PreciseMode::Jump, speed);
        self.set_precise_rotation(m, target_rotation, 0.2);
    }

    /// TdMove_MeleeAirAbove.ReachedPreciseLocation -> PlayCannedAnim.
    pub fn melee_air_above_reached_precise_location(&mut self, m: Move) {
        self.set_animation_movement_state(Move::None, 0.0);
        self.use_root_motion(true);
        self.use_root_rotation(true);
        self.play_move_anim(m, Slot::FullBody, "MarioMove", 1.0, 0.1, 0.2, true, true);
        if let Some(t) = self.moves.melee.air_above.base.target {
            self.bot_trigger_canned_air_above(t);
        }
        self.set_move_countdown(m, 1.0);
    }

    /// TdMove_MeleeAirAbove.FailedToReachPreciseLocation.
    pub fn melee_air_above_failed_precise_location(&mut self, _m: Move) {
        self.set_move(Move::Falling, false, false);
    }

    /// TdMove_MeleeAirAbove.OnTimer -> TriggerDamage: MeleeDamage at the right foot.
    pub fn melee_air_above_on_timer(&mut self, m: Move) {
        let Some(t) = self.moves.melee.air_above.base.target else { return };
        let hit = self.bone_location("RightFoot");
        let damage = self.moves.melee.air_above.base.damage;
        self.bots[t].active_death_anim_type = 0;
        self.melee_deliver_damage(m, damage, hit, Vec3::ZERO, DamageType::MeleeAirAbove);
    }

    /// TdMove_MeleeAirAbove.OnCustomAnimEnd.
    pub fn melee_air_above_on_custom_anim_end(&mut self, _m: Move) {
        self.set_move(Move::Walking, false, false);
    }

    // ------------------------------------------------------------------ TdMove_MeleeSlide

    /// TdMove_MeleeSlide.StartMove + TriggerMove.
    pub fn melee_slide_start_move(&mut self, m: Move) {
        self.melee_base_start_move(m);
        self.moves.melee.slide.base.hit_detection = false;
        self.moves.base_mut(m).timer = 0.0;
        self.play_move_anim(m, Slot::FullBody, "MeleeSlide", 1.0, 0.1, 0.1, false, false);
        self.moves.melee.slide.base.hit_detection_bone = "RightFoot";
        self.set_animation_movement_state(Move::Slide, 0.0);
        self.set_move_countdown(m, 0.2542);
        self.melee_update_target(m);
    }

    /// TdMove_MeleeSlide.StopMove.
    pub fn melee_slide_stop_move(&mut self, m: Move) {
        self.melee_base_stop_move(m);
        self.set_animation_movement_state(Move::None, 0.0);
    }

    /// TdMove_MeleeSlide.OnCustomAnimEnd.
    pub fn melee_slide_on_custom_anim_end(&mut self, _m: Move) {
        let stand = self.can_stand(self.pawn.location, false);
        self.set_move(if stand { Move::Walking } else { Move::Crouch }, false, false);
    }

    /// TdMove_MeleeSlide.TriggerDamage.
    fn melee_slide_trigger_damage(&mut self, m: Move, victim: usize) {
        if self.moves.melee.slide.base.target != Some(victim) {
            return;
        }
        self.bots[victim].active_death_anim_type = 1;
        if !self.melee_to_target(m, false).is_some_and(|(dot, dist)| dot > 0.4 && dist < 140.0) {
            return;
        }
        let hit = self.bone_location("RightFoot");
        let momentum = self.pawn.rotation.vector() * self.pawn.velocity.size_2d() * 1.6;
        let damage = self.moves.melee.slide.base.damage;
        self.melee_deliver_damage(m, damage, hit, momentum, DamageType::MeleeSlide);
        self.moves.melee.slide.base.hit_detection = false;
    }

    // ------------------------------------------------------------------ TdMove_MeleeWallrun

    /// TdMove_MeleeWallrun.StartMove + TriggerMove.
    pub fn melee_wallrun_start_move(&mut self, m: Move) {
        self.melee_base_start_move(m);
        self.moves.melee.wallrun.base.hit_detection = true;
        let left = self.pawn.old_movement_state == Move::WallRunningLeft;
        self.moves.melee.wallrun.left = left;
        if let Some(i) = self.moves.melee.wallrun.base.target {
            let b = &self.bots[i];
            let to = (b.location - self.pawn.location + Vec3::new(0.0, 0.0, b.base_eye_height)).safe_normal();
            self.set_precise_rotation(m, Rotator::from_vector(to), 0.1);
            self.pawn.velocity = to * self.pawn.velocity.size_2d() * 0.75;
        } else {
            let turn = if left { 6000 } else { -6000 };
            let r = self.pawn.rotation + Rotator::new(0, turn, 0);
            self.set_precise_rotation(m, r, 0.1);
            // Velocity << rot(0, -+6000, 0): the inverse rotation
            let a = (-turn as f32) / crate::math::URU_PER_RAD;
            let (sn, cs) = (-a).sin_cos();
            let v = self.pawn.velocity;
            self.pawn.velocity = Vec3::new(v.x * cs - v.y * sn, v.x * sn + v.y * cs, v.z) * 1.2;
        }
        self.reset_camera_look(m, 0.1);
        self.moves.melee.wallrun.base.hit_detection_bone = if left { "LeftLeg" } else { "RightLeg" };
        self.play_move_anim(m, Slot::FullBody, if left { "MeleeWallRunLeft" } else { "MeleeWallRunRight" }, 1.0, 0.1, 0.2, false, false);
    }

    /// TdMove_MeleeWallrun.TriggerDamage (its target check compares the victim to itself, so
    /// any pawn the sweep touches lands the kick on the target).
    fn melee_wallrun_trigger_damage(&mut self, m: Move, _victim: usize) {
        let Some((dot, dist)) = self.melee_to_target(m, false) else { return };
        if !(dot > 0.4 && dist < 140.0) {
            return;
        }
        let bone = self.moves.melee.wallrun.base.hit_detection_bone;
        let hit = self.bone_location(bone);
        let damage = self.moves.melee.wallrun.base.damage;
        let momentum = self.pawn.rotation.vector() * 500.0;
        self.melee_deliver_damage(m, damage, hit, momentum, DamageType::MeleeWallRun);
        self.pawn.velocity.x *= -0.5;
        self.pawn.velocity.y *= -0.5;
        self.moves.melee.wallrun.base.hit_detection = false;
    }

    // ------------------------------------------------------------------ TdMove_MeleeCrouch

    /// TdMove_MeleeCrouch.CanDoMove (against a wall it needs something to barge: not ported).
    pub fn melee_crouch_can_do_move(&mut self, m: Move) -> bool {
        if !self.melee_base_can_do_move(m) {
            return false;
        }
        self.pawn.against_wall_state == crate::body::AgainstWall::None
    }

    /// TdMove_MeleeCrouch.StartMove + TriggerMove.
    pub fn melee_crouch_start_move(&mut self, m: Move) {
        self.melee_base_start_move(m);
        self.moves.melee.crouch.base.hit_detection = false;
        self.set_animation_movement_state(Move::Crouch, 0.0);
        self.play_move_anim(m, Slot::UpperBody, "MeleeCrouchStart", 1.0, 0.1, -1.0, false, false);
        self.moves.melee.crouch.base.hit_detection_bone = "RightHand";
        self.melee_update_target(m);
    }

    /// TdMove_MeleeCrouch.StopMove.
    pub fn melee_crouch_stop_move(&mut self, m: Move) {
        self.melee_base_stop_move(m);
        self.anim.stop(Slot::UpperBody, 0.2);
        self.set_animation_movement_state(Move::None, 0.0);
    }

    /// TdMove_MeleeCrouch.OnCustomAnimEnd.
    pub fn melee_crouch_on_custom_anim_end(&mut self, m: Move) {
        match self.moves.melee.crouch.base.state {
            MeleeState::AttackNormal | MeleeState::AttackFinishing => {
                let hit = self.melee_crouch_test_hit(m);
                self.play_move_anim(m, Slot::UpperBody, "MeleeCrouchHit", 1.0, 0.1, 0.2, false, false);
                let b = &mut self.moves.melee.crouch.base;
                b.state = if hit { MeleeState::HitNormal } else { MeleeState::MissNormal };
            }
            MeleeState::Pending => {}
            _ => {
                let stand = self.can_stand(self.pawn.location + Vec3::new(0.0, 0.0, 30.0), false);
                self.set_move(if stand { Move::Walking } else { Move::Crouch }, false, false);
            }
        }
    }

    /// TdMove_MeleeCrouch.TestHit.
    fn melee_crouch_test_hit(&mut self, m: Move) -> bool {
        let Some((dot, dist)) = self.melee_to_target(m, false) else { return false };
        if !(dot > 0.8 && dist < 110.0) {
            return false;
        }
        let hit = self.bone_location("LeftHand");
        let damage = self.moves.melee.crouch.base.damage;
        let momentum = self.pawn.rotation.vector() * 800.0;
        self.melee_deliver_damage(m, damage, hit, momentum, DamageType::MeleeCrouch);
        true
    }

    /// TdMove_MeleeCrouch.TriggerDamage.
    fn melee_crouch_trigger_damage(&mut self, m: Move, victim: usize) {
        if self.moves.melee.crouch.base.target != Some(victim) {
            return;
        }
        if !self.melee_to_target(m, false).is_some_and(|(dot, dist)| dot > 0.4 && dist < 170.0) {
            return;
        }
        let hit = self.bone_location("RightFoot");
        let damage = self.moves.melee.crouch.base.damage;
        let momentum = self.pawn.rotation.vector().cross(Vec3::new(0.0, 0.0, 1.0)) * 700.0;
        self.pawn.velocity = Vec3::ZERO;
        self.melee_deliver_damage(m, damage, hit, momentum, DamageType::MeleeCrouch);
        self.moves.melee.crouch.base.hit_detection = false;
    }

    // ------------------------------------------------------------------ TdMove_Stumble

    /// TdMove_Stumble.CanDoMove.
    pub fn stumble_can_do_move(&mut self, m: Move) -> bool {
        let ms = self.pawn.movement_state;
        if matches!(ms, Move::Crouch | Move::MeleeCrouch) && self.can_stand(self.pawn.location + Vec3::new(0.0, 0.0, 30.0), false) {
            return true;
        }
        if self.moves.base(ms).use_custom_collision {
            return false;
        }
        self.tdmove_can_do_move(m)
    }

    /// TdMove_Stumble.StartMove + PlayStumbleAnimation.
    pub fn stumble_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        let st = crate::combat::stumble_state(&self.moves.stumble.hit, self.pawn.location, self.pawn.rotation, self.pawn.collision_height);
        self.moves.stumble.state = st;
        self.anim.stop(Slot::Weapon, 0.2);
        self.anim.stop(Slot::UpperBody, 0.2);
        self.anim.stop(Slot::FullBody, 0.2);
        use StumbleState as S;
        match st {
            S::HitMeleeFrontLeft | S::HitMeleeFrontRight | S::HitMeleeBargeFront | S::HitMeleeCrouchFront | S::HitMeleeWallrunRight | S::HitMeleeWallrunLeft | S::HitMeleeSlideFront | S::HitMeleeAirHeadFront | S::HitMeleeAirBodyFront => {
                if self.moves.stumble.in_air {
                    self.play_move_anim(m, Slot::FullBody, "gethitleft", 1.0, 0.1, 0.2, true, false);
                    let r = self.moves.stumble.hit.instigator_rotation.vector();
                    self.pawn.velocity = r * 300.0;
                    self.pawn.velocity.z = 80.0;
                    self.moves.stumble.current = "gethitleft";
                } else {
                    self.use_root_motion(true);
                    self.reset_camera_look(m, 0.2);
                    // ShouldMeleeCauseStumbleFar: the bot is blocking (not ported)
                    self.play_move_anim(m, Slot::FullBody, "GetHitStumbleBwd", 1.0, 0.1, 0.2, true, false);
                    self.moves.stumble.current = "GetHitStumbleBwd";
                    self.set_move_countdown(m, 0.5);
                }
            }
            S::HitMeleeBack | S::HitMeleeBackHead => {
                self.use_root_motion(true);
                self.play_move_anim(m, Slot::FullBody, "StumbleFwd", 1.0, 0.3, 0.2, true, false);
                self.moves.stumble.current = "StumbleFwd";
            }
            _ => {
                self.set_move(Move::Walking, false, false);
            }
        }
    }

    /// TdMove_Stumble.StopMove.
    pub fn stumble_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.moves.stumble.in_air = false;
        self.pc.targeting_pawn_interp = 0.0;
    }

    /// TdMove_Stumble.OnTimer.
    pub fn stumble_on_timer(&mut self, _m: Move) {
        if matches!(self.moves.stumble.current, "GetHitStumbleBwdFar" | "GetHitStumbleBwd") {
            self.set_move(Move::Walking, false, false);
        }
    }

    /// TdMove_Stumble.OnCustomAnimEnd.
    pub fn stumble_on_custom_anim_end(&mut self, _m: Move) {
        if self.pawn.animation_movement_state != Move::Turn180InAir {
            self.set_move(Move::Walking, false, false);
        }
    }

    /// The melee moves' move timers (TdMove_MeleeSlide.OnFindBargeTargetTimer: barging isn't
    /// ported).
    pub fn melee_on_move_timer(&mut self, _m: Move, id: u8) {
        let _ = id == MELEE_TIMER;
    }
}
