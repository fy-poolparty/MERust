//! Unarmed combat between the player and the bots: damage types (TdDmgType_*), the hit
//! reactions they pick (TdMove_StumbleBase.GetStumbleState), melee targeting
//! (UTdPlayerController::GetMeleeTarget) and damage delivery both ways.

use crate::math::{Rotator, UeVec, Vec3};
use crate::pawn::{Move, MoveAction};
use crate::sim::Sim;

/// The TdDmgType_Melee* classes (and the bots' generic melee).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageType {
    Melee,
    MeleeLeft,
    MeleeRight,
    MeleeAir,
    MeleeSlide,
    MeleeWallRun,
    MeleeCrouch,
    MeleeSoccerKick,
    MeleeVaultKick,
    /// TdDmgType_MeleeDisarm (the end of being disarmed).
    MeleeDisarm,
    /// TdDmgType_LowCaliber_Bullet.
    Bullet,
    /// TdDmgType_Fell (TdMove_BotStumbleFalling.Landed).
    Fell,
    /// TdDmgType_MeleeAirAbove (landing on an enemy).
    MeleeAirAbove,
}

/// TdPawn.EMeleeImpactType: which TdPhysicalMaterialMelee sound plays on the victim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeleeImpact {
    Gun,
    Fist,
    Foot,
}

/// TdMove_StumbleBase.EStumbleState.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StumbleState {
    #[default]
    HitNone = 0,
    HitMeleeBack = 1,
    HitMeleeBackHead = 2,
    HitMeleeFrontLeft = 3,
    HitMeleeFrontRight = 4,
    HitMeleeBargeFront = 5,
    HitMeleeCrouchFront = 6,
    HitMeleeSlideFront = 7,
    HitMeleeWallrunRight = 8,
    HitMeleeWallrunLeft = 9,
    HitMeleeAirHeadFront = 10,
    HitMeleeAirBodyFront = 11,
    HitMeleeVaultKick = 12,
    HitMeleeSoccerKick = 13,
}

/// What TdMove_StumbleBase reads off the hit (InstigatorLocation, DamageLocation, ...).
#[derive(Clone, Copy, Debug, Default)]
pub struct StumbleHit {
    pub instigator_location: Vec3,
    pub instigator_rotation: Rotator,
    pub damage_location: Vec3,
    pub momentum: Vec3,
    pub damage_type: Option<DamageType>,
}

/// TdMove_StumbleBase.GetStumbleState for a pawn at `location` facing `rotation` with a
/// collision cylinder `height` half-height tall.
pub fn stumble_state(hit: &StumbleHit, location: Vec3, rotation: Rotator, height: f32) -> StumbleState {
    use DamageType as D;
    use StumbleState as S;
    let hit_height = hit.damage_location - (location - Vec3::new(0.0, 0.0, height));
    let mut to = hit.instigator_location - location;
    to.z = 0.0;
    let side = to.safe_normal().dot(rotation.vector());
    let back = |h: f32| if hit_height.z > h { S::HitMeleeBackHead } else { S::HitMeleeBack };
    match hit.damage_type {
        Some(D::Melee | D::MeleeLeft | D::MeleeRight) => {
            if side > 0.0 {
                if hit.damage_type == Some(D::MeleeRight) { S::HitMeleeFrontRight } else { S::HitMeleeFrontLeft }
            } else {
                back(145.0)
            }
        }
        Some(D::MeleeVaultKick) => S::HitMeleeVaultKick,
        Some(D::MeleeAir) => {
            if side > 0.0 {
                if hit_height.z < 100.0 { S::HitMeleeAirBodyFront } else { S::HitMeleeAirHeadFront }
            } else {
                back(100.0)
            }
        }
        Some(D::MeleeCrouch) => if side > 0.0 { S::HitMeleeCrouchFront } else { back(145.0) },
        Some(D::MeleeSlide) => if side > 0.0 { S::HitMeleeSlideFront } else { back(145.0) },
        Some(D::MeleeWallRun) => {
            if side > 0.0 {
                // which side the instigator is on
                let c = rotation.vector().cross(hit.instigator_location - location);
                if c.z > 0.0 { S::HitMeleeWallrunLeft } else { S::HitMeleeWallrunRight }
            } else {
                back(145.0)
            }
        }
        Some(D::MeleeSoccerKick) => S::HitMeleeSoccerKick,
        Some(D::MeleeDisarm | D::Bullet | D::Fell | D::MeleeAirAbove) | None => S::HitNone,
    }
}

/// TdBotPawn.TakeDamage's MeleeImpactType: kicks (air, slide, wall run) land a foot.
pub fn impact_type(t: DamageType) -> MeleeImpact {
    match t {
        DamageType::MeleeAir | DamageType::MeleeSlide | DamageType::MeleeWallRun => MeleeImpact::Foot,
        _ => MeleeImpact::Fist,
    }
}

impl Sim {
    /// UTdPlayerController::GetMeleeTarget (0x11C30A0): the visible enemy scoring best on
    /// 0.8 * facing + 0.2 * closeness (0x11C2DC0), if any scores above zero.
    pub fn get_melee_target(&self, max_distance: f32) -> Option<usize> {
        let p = &self.pawn;
        let facing = p.rotation.vector();
        let mut best = (0.0f32, None);
        for (i, b) in self.bots.iter().enumerate() {
            if !b.alive() || !self.bot_visible_to_player(i) {
                continue;
            }
            let d = b.location - p.location;
            let dist = d.length();
            let closeness = (max_distance - dist) / max_distance * 0.2;
            let f = d.safe_normal().dot(facing).max(0.0) * 0.8;
            if f <= max_distance && f != 0.0 {
                let score = f + closeness;
                if score > best.0 {
                    best = (score, Some(i));
                }
            }
        }
        best.1
    }

    /// AController::LineOfSightTo between the player and bot i: from the bot's eyes to the
    /// player's eyes, her location, the top of her cylinder, then its two sides (across the
    /// line of sight); any clear trace sees her.
    pub fn bot_visible_to_player(&self, i: usize) -> bool {
        let b = &self.bots[i];
        let p = &self.pawn;
        let their = b.location + Vec3::new(0.0, 0.0, b.base_eye_height);
        let clear = |to: Vec3| !self.world.line_check(to, their, Vec3::ZERO).hit;
        if clear(p.location + Vec3::new(0.0, 0.0, p.base_eye_height)) || clear(p.location) || clear(p.location + Vec3::new(0.0, 0.0, p.collision_height)) {
            return true;
        }
        let d = p.location - their;
        let side = Vec3::new(-d.y, d.x, 0.0).safe_normal() * p.collision_radius;
        clear(p.location + side) || clear(p.location - side)
    }

    /// TdPawn.TakeDamage for the player from a bot: health (dying when it runs out), the
    /// regeneration clock, and the stumble (StumbleDamage -> HandleMoveAction(MA_Stumble)).
    pub fn player_take_melee_damage(&mut self, damage: i32, from: usize, hit_location: Vec3, momentum: Vec3, t: DamageType) {
        if self.pawn.dying {
            return;
        }
        let b = &self.bots[from];
        self.moves.stumble.hit = StumbleHit {
            instigator_location: b.location,
            instigator_rotation: b.rotation,
            damage_location: hit_location,
            momentum,
            damage_type: Some(t),
        };
        self.moves.stumble.instigator = Some(from);
        let mut damage = damage;
        if self.health - damage <= 0 {
            damage = self.move_handle_death(damage);
        }
        self.handle_move_action(MoveAction::Stumble);
        self.pawn.time_since_last_damage = 0.0;
        self.pawn.health_frac = 0.0;
        self.take_damage(damage);
    }

    /// Moves[MovementState].HandleDeath.
    fn move_handle_death(&mut self, damage: i32) -> i32 {
        match self.pawn.movement_state {
            // TdMove_IntoClimb / IntoZipLine.HandleDeath: Health - 1
            Move::IntoClimb | Move::IntoZipLine => self.health - 1,
            _ => damage,
        }
    }

    /// ATdPawn::RegenerateHealth (0x12B18F0): after RegenerateDelay without damage, health
    /// comes back at RegenerateHealthPerSecond.
    pub(crate) fn regenerate_health(&mut self, dt: f32) {
        let p = &mut self.pawn;
        p.time_since_last_damage += dt;
        if p.time_since_last_damage < p.regenerate_delay {
            return;
        }
        if self.health < p.max_health && self.health >= 0 && !p.dying {
            p.health_frac += p.regenerate_health_per_second * dt;
            let whole = p.health_frac as i32;
            if whole >= 1 {
                self.health += whole;
                p.health_frac -= whole as f32;
            }
            self.health = self.health.min(p.max_health);
        }
    }
}
