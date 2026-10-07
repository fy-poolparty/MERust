//! Guns: TdWeapon (instant-hit firing, ammo, the out-of-ammo auto drop), the TdPickup a dropped
//! gun becomes, and the AI side of shooting: TdBotPawn's burst control (FireWhenReady /
//! CheckFire / PauseFiring), TdBotPawn.SetAIFiringState (0x1236380) and TdAimBot (natives
//! UpdateDispersion 0x12537F0, GetImprovementRate 0x1252610, GetMaxOffset 0x12525C0).
//!
//! Not ported: hit decals, impact particles, tracers, shell ejection, zooming, sticky aim and
//! the AIManager's per-frame bullet control (single-shot pacing).

use crate::bots::BotMove;
use crate::math::{Rotator, UeVec, Vec3};
use crate::pawn::{Move, Slot};
use crate::sim::Sim;
use crate::sound::SoundEvent;

/// AIBurstInfo.
#[derive(Clone, Copy, Debug)]
pub struct Burst {
    pub length_min: i32,
    pub length_max: i32,
    pub pause_min: f32,
    pub pause_max: f32,
}

/// The defaults and [Weapons] config of one TdWeapon class (see weapon_table.rs).
#[derive(Debug)]
pub struct WeaponClass {
    pub name: &'static str,
    /// The weapon mesh (umodel export): package and SkeletalMesh.
    pub package: &'static str,
    pub mesh: &'static str,
    /// AnimationSetCharacter1p (with AS_C1P_OneHanded_Common / TwoHanded_Common under it).
    pub anim_set_1p: &'static str,
    /// TdWeapon_Heavy (EWT_Heavy): two-handed, slower, most parkour off.
    pub heavy: bool,
    /// FiringStatesArray[0]: "WeaponFiring" or "WeaponBursting".
    pub firing_state: &'static str,
    /// bAutomaticReFire[0]: keep firing while the button is held.
    pub automatic_refire: bool,
    /// BurstMax (WeaponBursting's shots per pull).
    pub burst_max: i32,
    /// PelletCount (the shotguns' CustomFire traces).
    pub pellets: i32,
    pub fire_interval: f32,
    pub damage: f32,
    pub momentum: f32,
    /// Weapon.Spread[0] (radians of the cone).
    pub spread: f32,
    pub fall_off_distance: f32,
    pub weapon_range: f32,
    pub death_anim_type: u8,
    pub max_ammo: i32,
    pub reload_time: f32,
    /// Weapon.EquipTime: WeaponEquipping's length (a press meanwhile only sets PendingFire).
    pub equip_time: f32,
    /// TdWeapon.WeaponPoseProfileName: the 1P TdAnimNodeWeaponPoseOffset profile.
    pub pose_profile: &'static str,
    /// InstantHitDamageTypes[0] is TdDmgType_Sniper_Bullet (TdBotPawn.AdjustDamage: 99999).
    pub sniper_bullet: bool,
    pub recoil_amount: f32,
    pub recoil_recover_time: f32,
    pub max_recoil: f32,
    pub kickback_amount: f32,
    pub combat_range_max: f32,
    /// AimedBurst_Near / _Mid / _Far.
    pub bursts: [Burst; 3],
    pub pre_reload_time: f32,
    pub reload_ready_time: (f32, f32),
    pub ai_damage_multiplier: f32,
    pub out_of_ammo_anim: &'static str,
    pub fire_1p: &'static str,
    pub fire_3p: &'static str,
    pub reverb_1p: &'static str,
    pub reverb_3p: &'static str,
    pub click: &'static str,
    pub drop: &'static str,
    pub pickup: &'static str,
}

#[path = "weapon_table.rs"]
mod table;
pub use table::*;
pub use table::PISTOL_GLOCK18C as GLOCK18C;

/// The enemy bodies (AITemplate SkeletalMesh / AdditionalSkeletalMesh).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NpcBody {
    pub package: &'static str,
    pub mesh: &'static str,
    pub head: Option<&'static str>,
}

/// A spawnable enemy: an AITemplate's body, animation sets, gun, drops, DisarmWindow and its
/// melee move's GenericAttackProperties ([AIMeleeAttacks]).
#[derive(Debug)]
pub struct Loadout {
    pub label: &'static str,
    pub weapon: Option<&'static WeaponClass>,
    pub body: NpcBody,
    /// AnimationSets[0] (package, set) and AnimationSets[1] (a set in the same package).
    pub anim_package: &'static str,
    pub anim_set: &'static str,
    pub anim_subset: Option<&'static str>,
    pub ammo_disarmed: i32,
    pub ammo_dropped: i32,
    pub disarm_window: f32,
    /// HitAngle, HitRange, Damage.
    pub melee: (f32, f32, f32),
    /// TdMove_Melee_PatrolCop plays MeleeMiss on a miss; the other melee moves MeleeEnd.
    pub melee_miss_anim: bool,
    /// The AITemplate's PawnClass armor at Medium difficulty (DefaultAI.ini
    /// Armor{Bullets,Melee}{Head,Body,Legs}Settings; the three parts are equal): the share of
    /// bullet and melee damage TdPawn.AdjustDamage takes off.
    pub armor: (f32, f32),
    /// AITemplate MeleeAttackLimit: punches in a row before it blocks (TdBotPawn.ShouldBlock).
    pub melee_attack_limit: i32,
}

const PATROL: NpcBody = NpcBody { package: "CH_TKY_Cop_Patrol", mesh: "SK_TKY_Cop_Patrol", head: Some("SK_TKY_Cop_Patrol_Head_2") };
const PATROL_PK: NpcBody = NpcBody { package: "CH_TKY_Cop_Patrol", mesh: "SK_TKY_Cop_Patrol_PK", head: None };
const SWAT: NpcBody = NpcBody { package: "CH_TKY_Cop_SWAT", mesh: "CH_TKY_Cop_SWAT", head: None };
const SNIPER: NpcBody = NpcBody { package: "CH_TKY_Cop_SWAT", mesh: "SK_TKY_Cop_Swat_Sniper", head: None };
const SUPPORT: NpcBody = NpcBody { package: "CH_TKY_Cop_Support", mesh: "SK_TKY_Cop_Support", head: None };
const PATROL_1H: &str = "AS_AI_PatrolCop_OneHanded";
const PATROL_2H: &str = "AS_AI_PatrolCop_TwoHanded";
const ASSAULT_2H: &str = "AS_AI_Assault_TwoHanded";
const SUPPORT_2H: &str = "AS_AI_Support_TwoHanded";

/// The spawnable enemies: the tutorial's unarmed sparring dummy, then one AITemplate per gun
/// (AITemplate_PatrolCop / _Glock / _SteyrTMP / _Remington, _Assault / _MP5K / _HKG36C /
/// _Neostead, _Support, _SniperCop). The Beretta's only carrier is Celeste, so a patrol cop
/// carries it, dropping one clip.
pub static LOADOUTS: [Loadout; 12] = [
    Loadout { label: "unarmed (sparring dummy)", weapon: None, body: PATROL, anim_package: PATROL_1H, anim_set: PATROL_1H, anim_subset: None, ammo_disarmed: 0, ammo_dropped: 0, disarm_window: 0.0, melee: (140.0, 120.0, 50.0), melee_miss_anim: false, armor: (0.0, 0.0), melee_attack_limit: 3 },
    Loadout { label: "patrol cop, Glock 18c", weapon: Some(&PISTOL_GLOCK18C), body: PATROL, anim_package: PATROL_1H, anim_set: PATROL_1H, anim_subset: Some("AS_AI_PatrolCop_Onehanded_Glock18"), ammo_disarmed: 24, ammo_dropped: 24, disarm_window: 0.1, melee: (75.0, 110.0, 40.0), melee_miss_anim: true, armor: (0.0, 0.0), melee_attack_limit: 3 },
    Loadout { label: "patrol cop, Colt 1911", weapon: Some(&PISTOL_COLT1911), body: PATROL, anim_package: PATROL_1H, anim_set: PATROL_1H, anim_subset: None, ammo_disarmed: 8, ammo_dropped: 8, disarm_window: 0.1, melee: (75.0, 110.0, 40.0), melee_miss_anim: true, armor: (0.0, 0.0), melee_attack_limit: 3 },
    Loadout { label: "patrol cop, Beretta M93R", weapon: Some(&PISTOL_BERETTAM93R), body: PATROL, anim_package: PATROL_1H, anim_set: PATROL_1H, anim_subset: None, ammo_disarmed: 21, ammo_dropped: 21, disarm_window: 0.1, melee: (75.0, 110.0, 40.0), melee_miss_anim: true, armor: (0.0, 0.0), melee_attack_limit: 3 },
    Loadout { label: "patrol cop, Steyr TMP", weapon: Some(&SMG_STEYRTMP), body: PATROL_PK, anim_package: PATROL_1H, anim_set: PATROL_1H, anim_subset: Some("AS_AI_PatrolCop_OneHanded_SteyrTMP"), ammo_disarmed: 30, ammo_dropped: 30, disarm_window: 0.15, melee: (75.0, 110.0, 40.0), melee_miss_anim: true, armor: (0.1, 0.2), melee_attack_limit: 3 },
    Loadout { label: "patrol cop, Remington 870", weapon: Some(&SHOTGUN_REMINGTON870), body: PATROL_PK, anim_package: PATROL_2H, anim_set: PATROL_2H, anim_subset: None, ammo_disarmed: 5, ammo_dropped: 5, disarm_window: 0.1, melee: (75.0, 110.0, 50.0), melee_miss_anim: false, armor: (0.1, 0.2), melee_attack_limit: 4 },
    Loadout { label: "SWAT, MP5K", weapon: Some(&ASSAULTRIFLE_MP5K), body: SWAT, anim_package: ASSAULT_2H, anim_set: ASSAULT_2H, anim_subset: Some("AS_AI_Assault_TwoHanded_MP5K"), ammo_disarmed: 30, ammo_dropped: 30, disarm_window: 0.25, melee: (75.0, 80.0, 50.0), melee_miss_anim: false, armor: (0.3, 0.4), melee_attack_limit: 3 },
    Loadout { label: "SWAT, FN SCAR-L", weapon: Some(&ASSAULTRIFLE_FNSCARL), body: SWAT, anim_package: ASSAULT_2H, anim_set: ASSAULT_2H, anim_subset: None, ammo_disarmed: 24, ammo_dropped: 24, disarm_window: 0.28, melee: (75.0, 80.0, 50.0), melee_miss_anim: false, armor: (0.3, 0.4), melee_attack_limit: 3 },
    Loadout { label: "SWAT, G36C", weapon: Some(&ASSAULTRIFLE_HKG36), body: SWAT, anim_package: ASSAULT_2H, anim_set: ASSAULT_2H, anim_subset: Some("AS_AI_Assault_TwoHanded_G36C"), ammo_disarmed: 27, ammo_dropped: 27, disarm_window: 0.28, melee: (75.0, 80.0, 50.0), melee_miss_anim: false, armor: (0.3, 0.4), melee_attack_limit: 3 },
    Loadout { label: "SWAT, Neostead", weapon: Some(&SHOTGUN_NEOSTEAD), body: SWAT, anim_package: ASSAULT_2H, anim_set: ASSAULT_2H, anim_subset: Some("AS_AI_Assault_TwoHanded_Neostead"), ammo_disarmed: 6, ammo_dropped: 6, disarm_window: 0.12, melee: (75.0, 80.0, 50.0), melee_miss_anim: false, armor: (0.3, 0.4), melee_attack_limit: 3 },
    Loadout { label: "support cop, FN Minimi", weapon: Some(&MACHINEGUN_FNMINIMI), body: SUPPORT, anim_package: SUPPORT_2H, anim_set: SUPPORT_2H, anim_subset: None, ammo_disarmed: 70, ammo_dropped: 70, disarm_window: 0.1, melee: (75.0, 150.0, 60.0), melee_miss_anim: false, armor: (0.5, 0.5), melee_attack_limit: 2 },
    Loadout { label: "sniper, Barrett M95", weapon: Some(&SNIPER_BARRETM95), body: SNIPER, anim_package: ASSAULT_2H, anim_set: ASSAULT_2H, anim_subset: Some("AS_AI_Assault_TwoHanded_M95"), ammo_disarmed: 8, ammo_dropped: 8, disarm_window: 0.01, melee: (75.0, 150.0, 20.0), melee_miss_anim: false, armor: (0.2, 0.3), melee_attack_limit: 3 },
];

/// The loadout that carries a gun (for its drops and disarm window).
pub fn loadout_for(class: &WeaponClass) -> &'static Loadout {
    LOADOUTS.iter().find(|l| l.weapon.is_some_and(|w| std::ptr::eq(w, class))).unwrap_or(&LOADOUTS[1])
}

/// Every sound cue the guns use (for the sound bank).
pub fn weapon_sound_cues() -> Vec<&'static str> {
    let mut v = vec![PLAYER_HIT_SOUND];
    for c in WEAPONS.iter() {
        v.extend([c.fire_1p, c.fire_3p, c.reverb_1p, c.reverb_3p, c.click, c.drop, c.pickup]);
    }
    v.retain(|s| !s.is_empty());
    v
}

/// TdPlayerPawn.TakeDamage's hit grunt.
pub const PLAYER_HIT_SOUND: &str = "A_Character_Female_01.Oral_Impact.Hard";

/// TdWeapon.WeaponDropLinearVelocity (controller axes X, Y, Z).
const WEAPON_DROP_LINEAR_VELOCITY: [f32; 3] = [50.0, 200.0, -80.0];
/// TdPickup's collision cylinder (pickups are touched within it).
const PICKUP_RADIUS: f32 = 300.0;
const PICKUP_HEIGHT: f32 = 64.0;

/// TdPawn.EWeaponAnimState.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WeaponAnimState {
    #[default]
    Unarmed = 0,
    Relaxed = 1,
    Ready = 2,
    Reload = 3,
    Throwing = 4,
}

/// The player's gun (TdWeapon in TdInventoryManager).
#[derive(Clone, Debug)]
pub struct Weapon {
    pub class: &'static WeaponClass,
    pub ammo: i32,
    /// WeaponFiring's refire timer.
    pub refire: f32,
    pub firing: bool,
    /// WeaponBursting: shots left in this pull (BurstMax - BurstCnt).
    pub burst_left: i32,
    /// State OutOfAmmo: the DropWeaponTimer.
    pub out_of_ammo: Option<f32>,
    /// TdPawn.DropWeapon's RemoveWeaponAfterDrop timer (the throwaway anim).
    pub dropping: Option<f32>,
    /// RecoilControl1p's offset (view pitch kick, recovering over RecoilRecoverTime).
    pub recoil: f32,
    /// Pawn.Weapon: false while it's only in the inventory (a disarm's
    /// CreateInventory(bDoNotActivate) until TdMOVE_Disarm.StopMove's SetCurrentWeapon), so
    /// it can't fire and the HUD keeps the unarmed crosshair.
    pub current: bool,
    /// State WeaponEquipping: EquipTime left before Active (BeginFire only sets PendingFire).
    pub equipping: f32,
}

impl Weapon {
    pub fn new(class: &'static WeaponClass, ammo: i32) -> Self {
        Weapon { class, ammo, refire: 0.0, firing: false, burst_left: 0, out_of_ammo: None, dropping: None, recoil: 0.0, current: true, equipping: class.equip_time }
    }
}

/// A gun lying in the world (TdPickup / DroppedPickup).
#[derive(Clone, Debug)]
pub struct Pickup {
    pub class: &'static WeaponClass,
    pub ammo: i32,
    pub location: Vec3,
    pub velocity: Vec3,
    pub rotation: Rotator,
    pub resting: bool,
}

/// One shot, for the presentation layer (muzzle flash, tracer).
#[derive(Clone, Copy, Debug)]
pub struct Shot {
    pub start: Vec3,
    pub end: Vec3,
    /// Fired by bot i (None = the player).
    pub bot: Option<usize>,
}

/// A bot's gun and its firing state (TdBotPawn burst variables, TdAimBot).
#[derive(Clone, Debug)]
pub struct BotWeapon {
    pub class: &'static WeaponClass,
    pub ammo: i32,
    pub refire: f32,
    /// IsPressingWeaponTrigger.
    pub pressing: bool,
    pub wants_to_fire: bool,
    pub burst_wait: Option<f32>,
    pub new_burst: bool,
    pub ok_to_burst: bool,
    pub shot_count: i32,
    pub current_burst_length: i32,
    pub burst_min: i32,
    pub burst_max: i32,
    pub burst_delay_min: f32,
    pub burst_delay_max: f32,
    /// SetAIFiringState: (mood, range) last set; GetFiringRange returns the range.
    pub firing_range: u8,
    /// TdAIController.bFirstShot: only the controller's very first shot starts a burst
    /// at full dispersion (StartBurst); it's never set again.
    pub first_shot: bool,
    /// The controller's reload: PreReloadTime, then the reload until ReloadReadyTime.
    pub reloading: Option<f32>,
    pub aim: AimBot,
}

/// TdAimBot (AITemplate_Default accuracy).
#[derive(Clone, Debug)]
pub struct AimBot {
    pub max_dispersion: f32,
    pub min_dispersion: f32,
    pub min_offset: f32,
    pub max_y_offset: f32,
    pub min_y_offset: f32,
    /// ImprovementRates_Far / _Medium / _Near (Medium difficulty).
    pub improvement_rate_far: f32,
    pub improvement_rate_medium: f32,
    pub improvement_rate_near: f32,
    pub movement_rate_x: f32,
    pub movement_rate_y: f32,
    pub move_sideways_multiplier: f32,
    pub move_away_multiplier: f32,
    pub move_toward_multiplier: f32,
    /// HorizontalOffset_Max Near / Medium / Far.
    pub horizontal_offset_max: [f32; 3],
    /// OffsetThreshold Near / Medium / Far.
    pub offset_threshold: [f32; 3],
    pub improvement_rate: f32,
    pub base_dispersion: f32,
    pub base_offset: f32,
    pub base_y_offset: f32,
    pub current_location: Vec3,
    pub time_of_last_shot: f32,
    pub current_x: f32,
    pub current_y: f32,
    pub target_x: f32,
    pub diff_x: f32,
    /// ATdPawn.AIAimOldMovementState: the player's move the one-shot penalty was taken for.
    pub old_movement_state: Move,
}

impl AimBot {
    pub fn new() -> Self {
        AimBot {
            max_dispersion: 35.0,
            min_dispersion: 10.0,
            min_offset: 5.0,
            max_y_offset: 40.0,
            min_y_offset: 10.0,
            improvement_rate_far: 35.0,
            improvement_rate_medium: 35.0,
            improvement_rate_near: 35.0,
            movement_rate_x: 0.9,
            movement_rate_y: 25.0,
            move_sideways_multiplier: 0.8,
            move_away_multiplier: 0.15,
            move_toward_multiplier: 1.3,
            horizontal_offset_max: [50.0, 90.0, 200.0],
            offset_threshold: [40.0, 70.0, 130.0],
            improvement_rate: 0.0,
            base_dispersion: 35.0,
            base_offset: 200.0,
            base_y_offset: 40.0,
            current_location: Vec3::ZERO,
            time_of_last_shot: 0.0,
            current_x: 0.0,
            current_y: 0.0,
            target_x: 0.0,
            diff_x: 0.0,
            old_movement_state: Move::None,
        }
    }
}

impl BotWeapon {
    pub fn new(class: &'static WeaponClass) -> Self {
        BotWeapon {
            class,
            ammo: class.max_ammo,
            refire: 0.0,
            pressing: false,
            wants_to_fire: false,
            burst_wait: None,
            new_burst: false,
            ok_to_burst: false,
            shot_count: 0,
            current_burst_length: 0,
            burst_min: class.bursts[1].length_min,
            burst_max: class.bursts[1].length_max,
            burst_delay_min: class.bursts[1].pause_min,
            burst_delay_max: class.bursts[1].pause_max,
            firing_range: 1,
            first_shot: true,
            reloading: None,
            aim: AimBot::new(),
        }
    }
}

/// Per-move AiAimPenalties / AiAimOneShotPenalties at Medium ([TdGame.TdMove_*] config and the
/// move classes' defaults): (penalty multiplier on aim improvement, one-shot dispersion add).
pub fn ai_aim_penalties(m: Move) -> (f32, f32) {
    match m {
        Move::Jump => (0.8, 0.0),
        Move::Slide => (0.8, 0.0),
        Move::SpringBoarding => (0.4, 75.0),
        Move::WallRunningLeft | Move::WallRunningRight | Move::WallClimbing => (0.5, 0.0),
        Move::WallClimb180TurnJump => (0.4, 100.0),
        Move::Swing => (0.2, 200.0),
        Move::SwingJump => (0.3, 200.0),
        Move::ZipLine => (0.1, 200.0),
        Move::Snatch => (0.08, 200.0),
        Move::Balance => (1.0, 50.0),
        Move::DodgeJump | Move::GrabTransfer | Move::WallRunJump | Move::WallRunDodgeJump | Move::WallClimbDodgeJump => (1.0, 75.0),
        _ => (1.0, 0.0),
    }
}

/// Weapon.AddSpread.
fn add_spread(base: Rotator, spread: f32, r1: f32, r2: f32, r3: f32) -> Vec3 {
    let (x, y, z) = base.axes();
    let rand_y = r1 - 0.5;
    let rand_z = (0.5 - rand_y * rand_y).sqrt() * (r2 - 0.5);
    let _ = r3;
    (x + y * (rand_y * spread) + z * (rand_z * spread)).safe_normal()
}

/// FPctByRange.
fn pct_by_range(v: f32, lo: f32, hi: f32) -> f32 {
    (v - lo) / (hi - lo)
}

impl WeaponClass {
    /// TdWeapon.GetInstantHitDamage (EWFOT_Linear fall-off; AIDamageMultiplier 1).
    pub fn instant_hit_damage(&self, distance: f32) -> f32 {
        let mut d = self.damage;
        if distance > self.fall_off_distance && distance < self.weapon_range {
            d *= 1.0 - pct_by_range(distance, self.fall_off_distance, self.weapon_range);
        } else if distance >= self.weapon_range {
            d = 0.0;
        }
        d
    }

    /// TdWeapon.GetInstantHitMomentum.
    pub fn instant_hit_momentum(&self, distance: f32) -> f32 {
        if distance < self.fall_off_distance {
            self.momentum
        } else if distance < self.weapon_range {
            self.momentum * (1.0 - pct_by_range(distance, self.fall_off_distance, self.weapon_range))
        } else {
            0.0
        }
    }
}

/// Where a segment first enters a vertical cylinder (centre `c`, radius, half height), as a
/// fraction of the segment.
pub fn segment_cylinder(start: Vec3, end: Vec3, c: Vec3, radius: f32, half_height: f32) -> Option<f32> {
    let d = end - start;
    let (ox, oy) = (start.x - c.x, start.y - c.y);
    let a = d.x * d.x + d.y * d.y;
    let b = 2.0 * (ox * d.x + oy * d.y);
    let cc = ox * ox + oy * oy - radius * radius;
    let mut t_in = 0.0f32;
    let mut t_out = 1.0f32;
    if a < 1e-8 {
        if cc > 0.0 {
            return None;
        }
    } else {
        let disc = b * b - 4.0 * a * cc;
        if disc < 0.0 {
            return None;
        }
        let s = disc.sqrt();
        t_in = t_in.max((-b - s) / (2.0 * a));
        t_out = t_out.min((-b + s) / (2.0 * a));
    }
    // the caps
    let (zlo, zhi) = (c.z - half_height, c.z + half_height);
    if d.z.abs() < 1e-8 {
        if start.z < zlo || start.z > zhi {
            return None;
        }
    } else {
        let (t1, t2) = ((zlo - start.z) / d.z, (zhi - start.z) / d.z);
        t_in = t_in.max(t1.min(t2));
        t_out = t_out.min(t1.max(t2));
    }
    (t_in <= t_out && t_out >= 0.0 && t_in <= 1.0).then_some(t_in.max(0.0))
}

impl Sim {
    /// A uniform random number in [0, 1) (FRand).
    pub fn frand(&mut self) -> f32 {
        self.moves.walking.frand()
    }

    // ------------------------------------------------------------------ the player's gun

    /// Pawn.Weapon != None (the current weapon).
    pub fn has_weapon(&self) -> bool {
        self.weapon.as_ref().is_some_and(|w| w.current)
    }

    /// UpdateAnimSets for callers outside the sim (respawn).
    pub fn update_anim_sets_pub(&mut self) {
        self.update_anim_sets();
    }

    /// TdPawn.UpdateAnimSets: the armed sets (AS_C1P_OneHanded_Common + the gun's) over
    /// AS_C1P_Unarmed, or back to unarmed.
    pub(crate) fn update_anim_sets(&mut self) {
        let class = self.weapon.as_ref().map(|w| w.class.name);
        if class != self.anim_weapon {
            self.anim_weapon = class;
            self.anim_armed = class.is_some();
            let lib = match class {
                Some(c) => self.armed_libs.get(c).cloned(),
                None => self.unarmed_lib.clone(),
            };
            if let Some(lib) = lib {
                self.anim.lib = lib;
            }
        }
    }

    /// TdPawn.GetWeaponType: the gun in hand is heavy.
    pub fn heavy_weapon(&self) -> bool {
        self.weapon.as_ref().is_some_and(|w| w.class.heavy)
    }

    /// Give the player a gun (TdInventoryManager.CreateInventory): the anim sets change; the
    /// caller deploys it (or not yet, during a disarm).
    pub fn give_weapon(&mut self, class: &'static WeaponClass, ammo: i32) {
        self.weapon = Some(Weapon::new(class, ammo));
        self.update_anim_sets();
    }

    /// TdPawn.PlayWeaponSwitch -> PlayWeaponDeploy: unholster on the canned upper body, and
    /// the gun comes up ready.
    pub fn play_weapon_deploy(&mut self) {
        self.anim.play(Slot::CannedUpperBody, "unholster", 1.0, 0.0, 0.2, false, false, false);
        self.set_weapon_anim_state(WeaponAnimState::Ready);
    }

    /// TdPawn.SetWeaponAnimState.
    pub fn set_weapon_anim_state(&mut self, s: WeaponAnimState) {
        if s != WeaponAnimState::Unarmed && self.weapon.is_none() {
            self.weapon_anim_state = WeaponAnimState::Unarmed;
            return;
        }
        if self.weapon_anim_state == s {
            return;
        }
        if s == WeaponAnimState::Ready {
            // BecameReadyTime; AmountTilUnarmed (light weapons: 1000 uu of movement)
            self.became_ready_time = self.time;
            self.amount_til_unarmed = if self.heavy_weapon() { 0.0 } else { 1000.0 };
        }
        self.weapon_anim_state = s;
    }

    /// TdPawn.UpdateWeaponAnimState: a light gun drops back to relaxed once the pawn has moved
    /// AmountTilUnarmed and TimeToStayReady (5 s) has passed since it became ready.
    pub(crate) fn update_weapon_anim_state(&mut self, dt: f32) {
        // TdSkelControlRecoil tick (0x1222B60): after RecoverDelay, EffectorLocation.X comes
        // back at InterpFactor 15 per second (at least MinInterpValue 0.05 a tick)
        if self.recoil_delay > 0.0 {
            self.recoil_delay -= dt;
        } else if self.recoil_x > 0.0 {
            let step = (15.0 * self.recoil_x * dt).max(0.05);
            self.recoil_x -= step;
        }
        self.amount_til_unarmed -= self.pawn.velocity.length() * dt;
        // TimeToStayReady: 5 s light, 1 s heavy; a heavy gun goes back to ready (its moves
        // here have no bTwoHandedFullBodyAnimations), a light one relaxes
        let heavy = self.heavy_weapon();
        let stay = if heavy { 1.0 } else { 5.0 };
        if self.amount_til_unarmed <= 0.0 && self.weapon_anim_state == WeaponAnimState::Ready && self.became_ready_time < self.time - stay {
            if !heavy {
                self.set_weapon_anim_state(WeaponAnimState::Relaxed);
            }
            self.amount_til_unarmed = 0.0;
        }
    }

    /// TdPlayerPawn.TossWeapon on death: the gun leaves the hand (Velocity * 2 plus 25 along
    /// each view axis).
    pub(crate) fn toss_weapon_on_death(&mut self) {
        let Some(w) = self.weapon.take() else { return };
        let (v1, v2, v3) = self.pc.rotation.axes();
        let velocity = self.pawn.velocity * 2.0 + (v1 + v2 + v3) * 25.0;
        let eye = self.pawn.location + Vec3::new(0.0, 0.0, self.pawn.base_eye_height);
        let location = eye + v1 * 30.0 + v2 * 15.0 - v3 * 20.0;
        self.pickups.push(Pickup { class: w.class, ammo: w.ammo, location, velocity, rotation: self.pawn.rotation, resting: false });
        self.anim.stop(Slot::Weapon, 0.0);
        self.anim.stop(Slot::CannedUpperBody, 0.0);
        self.update_anim_sets();
        self.weapon_anim_state = WeaponAnimState::Unarmed;
    }

    /// TdPlayerController.AttackPress with a gun: StartFire.
    pub(crate) fn player_start_fire(&mut self) {
        let Some(w) = self.weapon.as_mut() else { return };
        if w.dropping.is_some() || !w.current || w.equipping > 0.0 {
            return;
        }
        // TdWeapon.StartFire: not while pressed against a wall
        if self.pawn.against_wall_state != crate::body::AgainstWall::None {
            return;
        }
        if w.out_of_ammo.is_some() {
            // state OutOfAmmo: StartFire just clicks
            let click = w.class.click;
            self.sound(SoundEvent::Cue(click.to_string()));
            return;
        }
        if !w.firing {
            w.firing = true;
            w.refire = 0.0;
            // BeginFire -> FiringStatesArray[0]: the first shot right away (WeaponBursting
            // counts BurstCnt from 1)
            w.burst_left = if w.class.firing_state == "WeaponBursting" { w.class.burst_max - 1 } else { 0 };
            self.player_fire_ammunition();
        }
    }

    /// Each tick: refire while the button is held (WeaponFiring.RefireCheckTimer), recoil
    /// recovery, the out-of-ammo and throw-away timers.
    pub(crate) fn tick_player_weapon(&mut self, dt: f32, holding: bool) {
        let Some(w) = self.weapon.as_mut() else { return };
        // RecoilControl1p recovers over RecoilRecoverTime
        if w.recoil > 0.0 {
            let rate = w.class.max_recoil / w.class.recoil_recover_time.max(1e-3);
            w.recoil = (w.recoil - rate * dt * 0.1).max(0.0);
        }
        if let Some(t) = w.dropping.as_mut() {
            *t -= dt;
            if *t <= 0.0 {
                self.remove_weapon_after_drop();
            }
            return;
        }
        // WeaponEquipping -> Active: Active.BeginState fires if the trigger is still held
        if w.current && w.equipping > 0.0 {
            w.equipping -= dt;
            if w.equipping <= 0.0 && holding {
                self.player_start_fire();
            }
            return;
        }
        if let Some(t) = w.out_of_ammo.as_mut() {
            *t -= dt;
            if *t <= 0.0 {
                w.out_of_ammo = None;
                // DropWeaponTimer
                self.drop_weapon();
            }
            return;
        }
        if w.firing {
            w.refire += dt;
            let interval = w.class.fire_interval;
            if w.refire >= interval {
                w.refire -= interval;
                let bursting = w.class.firing_state == "WeaponBursting";
                if bursting {
                    // WeaponBursting.RefireCheckTimer: until BurstCnt reaches BurstMax
                    if w.burst_left > 0 && w.ammo > 0 {
                        w.burst_left -= 1;
                        self.player_fire_ammunition();
                    } else {
                        w.firing = false;
                    }
                } else if holding && w.ammo > 0 && w.class.automatic_refire {
                    // ShouldRefire: bAutomaticReFire, still pressed, has ammo
                    self.player_fire_ammunition();
                } else {
                    w.firing = false;
                }
            }
        }
    }

    /// TdWeapon.FireAmmunition for the player: ammo, sound, the fire anim and kickback, then
    /// InstantFire from the camera along the (spread) view.
    fn player_fire_ammunition(&mut self) {
        let Some(w) = self.weapon.as_mut() else { return };
        if w.ammo <= 0 {
            return;
        }
        w.ammo -= 1;
        let class = w.class;
        w.recoil = (w.recoil + class.recoil_amount).min(class.max_recoil);
        self.sound(SoundEvent::Cue(class.fire_1p.to_string()));
        self.sound(SoundEvent::Cue(class.reverb_1p.to_string()));
        // PlayFiringAnimation: TdMove.PlayFireAnimation (FireAnimSeqName "standfire" on the
        // weapon slot) and the view kickback
        self.anim.stop(Slot::Weapon, 0.0);
        self.anim.play(Slot::Weapon, "standfire", 1.0, 0.1, 0.1, false, false, false);
        self.set_weapon_anim_state(WeaponAnimState::Ready);
        // RecoilControl1p.AddImpulse(RecoilAmount, RecoilRecoverTime, MinRecoil, MaxRecoil)
        self.recoil_x = (self.recoil_x + class.recoil_amount).clamp(0.0, class.max_recoil);
        self.recoil_delay = class.recoil_recover_time;
        self.pc.rotation.pitch += class.kickback_amount as i32;
        // InstantFire (the shotguns' CustomFire: PelletCount traces) from the camera along
        // GetAdjustedAim + AddSpread
        let start = self.pawn.location + Vec3::new(0.0, 0.0, self.pawn.base_eye_height);
        for k in 0..class.pellets.max(1) {
            let (r1, r2) = (self.frand(), self.frand());
            let dir = add_spread(self.pc.rotation, class.spread, r1, r2, 0.0);
            let end = start + dir * class.weapon_range;
            let (t, bot) = self.trace_shot(start, end, None);
            let hit = start + (end - start) * t;
            if k == 0 {
                self.events.push(crate::sim::Event::Shot(Shot { start, end: hit, bot: None }));
            }
            if let Some(i) = bot {
                // TdBotPawn.TakeDamage: LastEnemyHitTimeOut (the crosshair flashes)
                self.last_enemy_hit_time_out = self.time + 0.35;
                let dist = (hit - start).length();
                let damage = class.instant_hit_damage(dist);
                let momentum = dir * class.instant_hit_momentum(dist);
                self.bot_take_bullet_damage(i, damage, hit, momentum, class);
            }
        }
        // out of ammo after this shot: WeaponEmpty -> bAutoDrop -> state OutOfAmmo
        let w = self.weapon.as_mut().unwrap();
        if w.ammo == 0 {
            w.firing = false;
            w.out_of_ammo = Some(1.5);
            if !class.out_of_ammo_anim.is_empty() {
                self.anim.play(Slot::Weapon, class.out_of_ammo_anim, 1.0, 0.0, -1.0, true, false, false);
            }
        }
    }

    /// The first thing an instant-hit trace from `start` to `end` hits: the world, or a live
    /// bot's cylinder (skipping `ignore`). Returns the fraction and the bot.
    pub fn trace_shot(&self, start: Vec3, end: Vec3, ignore: Option<usize>) -> (f32, Option<usize>) {
        let w = self.world.line_check(end, start, Vec3::ZERO);
        let mut best = (if w.hit { w.time } else { 1.0 }, None);
        for (i, b) in self.bots.iter().enumerate() {
            if Some(i) == ignore || !b.alive() {
                continue;
            }
            if let Some(t) = segment_cylinder(start, end, b.location, b.collision_radius, b.collision_height) {
                if t < best.0 {
                    best = (t, Some(i));
                }
            }
        }
        best
    }

    /// TdPlayerController.SwitchWeapon (GBA_SwitchWeapon, RMB) with a gun: TdPawn.DropWeapon
    /// (the throwaway anim, then RemoveWeaponAfterDrop tosses it).
    pub fn drop_weapon(&mut self) {
        let Some(w) = self.weapon.as_mut() else { return };
        if w.dropping.is_some() {
            return;
        }
        let len = self.anim.lib.get("throwaway").map(|s| s.length).unwrap_or(0.73);
        w.dropping = Some(len);
        w.firing = false;
        // SetWeaponAnimState(WS_Throwing): throwaway on CannedUpperBody, weaponpose on the gun
        self.set_weapon_anim_state(WeaponAnimState::Throwing);
        self.anim.play(Slot::CannedUpperBody, "throwaway", 1.0, 0.1, 0.0, false, false, false);
    }

    /// TdPawn.RemoveWeaponAfterDrop -> TossWeapon: the gun leaves the hand as a pickup.
    fn remove_weapon_after_drop(&mut self) {
        let Some(w) = self.weapon.take() else { return };
        let (v1, v2, v3) = self.pc.rotation.axes();
        let v = WEAPON_DROP_LINEAR_VELOCITY;
        let velocity = v1 * v[0] + v2 * v[1] + v3 * v[2];
        let eye = self.pawn.location + Vec3::new(0.0, 0.0, self.pawn.base_eye_height);
        let location = eye + v1 * 30.0 + v2 * 15.0 - v3 * 20.0;
        {
            self.pickups.push(Pickup { class: w.class, ammo: w.ammo, location, velocity, rotation: self.pawn.rotation, resting: false });
        }
        self.anim.stop(Slot::Weapon, 0.1);
        self.anim.stop(Slot::CannedUpperBody, 0.1);
        self.update_anim_sets();
        self.weapon_anim_state = WeaponAnimState::Unarmed;
    }

    /// TdInventoryManager.TryToPickUpWeapon: the first pickup touching the pawn with ammo.
    pub(crate) fn try_pick_up_weapon(&mut self) -> bool {
        let p = self.pawn.location;
        let Some(i) = self.pickups.iter().position(|k| {
            let d = Vec3::new(k.location.x - p.x, k.location.y - p.y, 0.0).length();
            k.ammo > 0 && d < PICKUP_RADIUS + self.pawn.collision_radius && (k.location.z - p.z).abs() < PICKUP_HEIGHT + self.pawn.collision_height
        }) else {
            return false;
        };
        let k = self.pickups.remove(i);
        self.sound(SoundEvent::Cue(k.class.pickup.to_string()));
        self.give_weapon(k.class, k.ammo);
        self.play_weapon_deploy();
        true
    }

    /// Pickups fall and come to rest on the floor (PHYS_Falling until they land).
    pub(crate) fn tick_pickups(&mut self, dt: f32) {
        let g = self.pawn.world_gravity_z;
        for i in 0..self.pickups.len() {
            if self.pickups[i].resting {
                continue;
            }
            let k = &mut self.pickups[i];
            k.velocity.z += g * dt;
            let start = k.location;
            let end = start + k.velocity * dt;
            let ext = Vec3::new(4.0, 4.0, 4.0);
            let hit = self.world.line_check(end, start, ext);
            let k = &mut self.pickups[i];
            if hit.hit {
                k.location = start + (end - start) * hit.time;
                if hit.normal.z > 0.7 {
                    k.resting = true;
                    k.velocity = Vec3::ZERO;
                    let snd = k.class.drop;
                    self.sound(SoundEvent::Cue(snd.to_string()));
                } else {
                    k.velocity -= hit.normal * (k.velocity.dot(hit.normal) * 1.3);
                }
            } else {
                k.location = end;
            }
        }
    }

    // ------------------------------------------------------------------ the bots' guns

    /// TdBotPawn.SetAIFiringState(Mood 0, EnemyDistanceSq) (0x1236380).
    fn bot_set_ai_firing_state(&mut self, i: usize, dist_sq: f32) {
        let Some(w) = self.bots[i].weapon.as_mut() else { return };
        let max = w.class.combat_range_max;
        let range = if dist_sq > max * max { 2 } else if dist_sq >= 240000.0 { 1 } else { 0 };
        if range != w.firing_range {
            let b = w.class.bursts[range as usize];
            w.burst_min = b.length_min;
            w.burst_max = b.length_max;
            w.burst_delay_min = b.pause_min;
            w.burst_delay_max = b.pause_max;
            w.firing_range = range;
        }
    }

    /// TdAimBot.UpdateDispersion (0x12537F0) and GetImprovementRate (0x1252610).
    fn bot_update_dispersion(&mut self, i: usize, dt: f32, enemy_visible: bool) {
        let (penalty, one_shot) = ai_aim_penalties(self.pawn.movement_state);
        let ms = self.pawn.movement_state;
        let enemy_vel = self.pawn.velocity;
        let enemy_loc = self.pawn.location;
        let b = &mut self.bots[i];
        let loc = b.location;
        let Some(w) = b.weapon.as_mut() else { return };
        let a = &mut w.aim;
        let range = w.firing_range as usize;
        // GetAIAimingOneShotPenalty: once per new player move
        let shot = if a.old_movement_state != ms { one_shot } else { 0.0 };
        a.old_movement_state = ms;
        a.base_dispersion += shot;
        a.base_offset += shot;
        a.base_y_offset += shot;
        if enemy_visible {
            // GetImprovementRate
            let mut rate = match range {
                2 => a.improvement_rate_far,
                1 => a.improvement_rate_medium,
                _ => a.improvement_rate_near,
            };
            let speed2d = (enemy_vel.x * enemy_vel.x + enemy_vel.y * enemy_vel.y).sqrt();
            let mult = if speed2d <= 200.0 {
                a.move_toward_multiplier
            } else {
                let vdir = Vec3::new(enemy_vel.x, enemy_vel.y, 0.0).safe_normal();
                let to_me = Vec3::new(loc.x - enemy_loc.x, loc.y - enemy_loc.y, 0.0).safe_normal();
                let d = vdir.dot(to_me);
                if d.abs() >= 0.7 {
                    if d < 0.0 { a.move_away_multiplier } else { a.move_toward_multiplier }
                } else {
                    a.move_sideways_multiplier
                }
            };
            rate *= mult;
            if rate > 0.0 {
                rate *= penalty;
            }
            a.improvement_rate = rate;
            a.base_dispersion -= rate * dt;
            a.base_offset -= rate * dt;
            a.base_y_offset -= rate * dt;
        }
        a.base_dispersion = a.base_dispersion.clamp(a.min_dispersion, a.max_dispersion);
        a.base_offset = a.base_offset.max(a.min_offset).min(a.horizontal_offset_max[range]);
        a.base_y_offset = a.base_y_offset.max(a.min_y_offset).min(a.max_y_offset);
    }

    /// TdAimBot.GetAimLocation (StartBurst on the burst's first shot, else GetNextLocation).
    fn bot_aim_location(&mut self, i: usize) -> Vec3 {
        let first = self.bots[i].weapon.as_ref().is_some_and(|w| w.first_shot);
        let target = self.pawn.location;
        let viewpoint = self.pawn.location + Vec3::new(0.0, 0.0, self.pawn.base_eye_height);
        let now = self.time;
        if first {
            // StartBurst: full dispersion, then PickPoint
            let w = self.bots[i].weapon.as_mut().unwrap();
            w.aim.time_of_last_shot = now;
            w.aim.base_dispersion = w.aim.max_dispersion;
            w.aim.base_offset = w.aim.horizontal_offset_max[w.firing_range as usize];
            w.aim.base_y_offset = w.aim.max_y_offset;
            w.first_shot = false;
            let p = self.bot_pick_point(i, target, viewpoint);
            self.bots[i].weapon.as_mut().unwrap().aim.current_location = p;
            return p;
        }
        // GetNextLocation
        let (r1, r2, r3, r4) = (self.frand(), self.frand(), self.frand(), self.frand());
        let bl = self.bots[i].location;
        let w = self.bots[i].weapon.as_mut().unwrap();
        let a = &mut w.aim;
        let sign = |x: f32| if x < 0.0 { -1.0 } else { 1.0 };
        let new_aim_y = a.movement_rate_y * (now - a.time_of_last_shot);
        let err_z = viewpoint.z - a.current_location.z;
        let mut dy = if err_z > 1.0 { new_aim_y * sign(a.current_y) } else { -10.0 };
        let new_aim_x = a.movement_rate_x * a.diff_x * (now - a.time_of_last_shot);
        let mut dx = 0.0;
        if (a.target_x - a.current_x).abs() > 1.0 {
            dx = -new_aim_x;
        }
        dx += r1 * dx * if r2 < 0.5 { -0.25 } else { 0.25 };
        dy += r3 * dy * if r4 < 0.5 { -1.0 } else { 1.0 };
        a.current_x += dx;
        a.current_y += dy;
        let disp_offset = a.base_offset.abs();
        a.time_of_last_shot = now;
        if sign(a.target_x) == sign(a.current_x) && a.current_x.abs() >= a.target_x.abs().max(disp_offset) {
            let p = self.bot_pick_point(i, target, viewpoint);
            self.bots[i].weapon.as_mut().unwrap().aim.current_location = p;
            p
        } else {
            let (cx, cy) = (a.current_x, a.current_y);
            let p = target + offset_point(bl, target, cx, cy);
            a.current_location = p;
            p
        }
    }

    /// TdAimBot.PickPoint.
    fn bot_pick_point(&mut self, i: usize, target: Vec3, viewpoint: Vec3) -> Vec3 {
        let (r1, r2, r3) = (self.frand(), self.frand(), self.frand());
        let moving = self.pawn.velocity.length() > 10.0;
        let target_rot_yaw = self.pawn.rotation.yaw;
        let bl = self.bots[i].location;
        let w = self.bots[i].weapon.as_mut().unwrap();
        let a = &mut w.aim;
        let height = a.base_y_offset;
        let width = a.base_dispersion;
        // GetDispersionOffset (the speed term needs an orthogonal speed over 200; the speed
        // curve's last point is the player's max speed)
        let mut offset = a.base_offset;
        let _ = target_rot_yaw;
        let sign = |x: f32| if x < 0.0 { -1.0 } else { 1.0 };
        let enemy_facing_left = {
            let to = Rotator::from_vector(target - bl).yaw;
            crate::math::norm_axis(target_rot_yaw - to) > 0
        };
        if enemy_facing_left {
            offset = -offset;
        }
        a.current_x = offset + sign(offset) * width * r1;
        a.current_y = (viewpoint.z - target.z) - r2 * height;
        let mut direction = 1.0;
        if !moving && r3 < 0.5 {
            direction = -1.0;
        }
        a.current_x *= direction;
        a.target_x = -a.current_x;
        a.diff_x = a.current_x - a.target_x;
        target + offset_point(bl, target, a.current_x, a.current_y)
    }

    /// The controller's firing: aim, bursts, reload. Called each tick for an armed bot that's
    /// in its combat state (FireWeapon set) or not.
    pub(crate) fn tick_bot_weapon(&mut self, i: usize, dt: f32, fire: bool, enemy_visible: bool) {
        if self.bots[i].weapon.is_none() {
            return;
        }
        let d2 = (self.pawn.location - self.bots[i].location).length_squared();
        self.bot_set_ai_firing_state(i, d2);
        self.bot_update_dispersion(i, dt, enemy_visible);
        // reload (Advance's Reload label: PreReloadTime, then ReloadReadyTime)
        if let Some(t) = self.bots[i].weapon.as_mut().unwrap().reloading.as_mut() {
            *t -= dt;
            if *t <= 0.0 {
                let w = self.bots[i].weapon.as_mut().unwrap();
                w.reloading = None;
                w.ammo = w.class.max_ammo;
            }
            return;
        }
        let in_melee = matches!(self.bots[i].movement_state, BotMove::Melee | BotMove::Stumble | BotMove::Disarmed | BotMove::Dying);
        let force_wait = self.bots[i].force_wait_for_damage > 0.0;
        {
            let w = self.bots[i].weapon.as_mut().unwrap();
            if fire && !w.wants_to_fire {
                // FireWhenReady
                w.wants_to_fire = true;
                w.burst_wait = None;
                w.new_burst = true;
                w.shot_count = 0;
            } else if !fire && w.wants_to_fire {
                // CeaseFire
                w.wants_to_fire = false;
                w.pressing = false;
            }
            if !w.wants_to_fire {
                return;
            }
        }
        // PauseFiring's refire delay -> StartNewBurst
        let w = self.bots[i].weapon.as_mut().unwrap();
        if let Some(t) = w.burst_wait.as_mut() {
            *t -= dt;
            if *t > 0.0 {
                return;
            }
            w.burst_wait = None;
            w.new_burst = true;
            w.shot_count = 0;
        }
        // CheckFire
        if w.new_burst {
            let (lo, hi) = (w.burst_min, w.burst_max);
            let r = self.frand();
            let w = self.bots[i].weapon.as_mut().unwrap();
            w.current_burst_length = (lo as f32 + (hi - lo) as f32 * r).round() as i32;
            w.current_burst_length = w.current_burst_length.max(1);
            // AIManager.AskToBurst: MaxSimultaneousBursts 2
            let active = self.bots.iter().filter(|b| b.weapon.as_ref().is_some_and(|w| w.pressing)).count();
            let w = self.bots[i].weapon.as_mut().unwrap();
            w.ok_to_burst = active < 2;
            w.new_burst = false;
            return;
        }
        let can_fire = w.ok_to_burst && !force_wait && !in_melee && w.ammo > 0;
        if can_fire && !w.pressing {
            w.pressing = true;
            w.refire = w.class.fire_interval;
        } else if !can_fire && w.pressing {
            w.pressing = false;
        }
        if !w.ok_to_burst {
            // wait for a slot: ask again next tick
            w.new_burst = true;
            return;
        }
        if !w.pressing {
            if w.ammo <= 0 && w.reloading.is_none() {
                // NotifyWeaponEmpty -> Reload
                let (lo, hi) = w.class.reload_ready_time;
                let pre = w.class.pre_reload_time;
                let r = self.frand();
                self.bots[i].weapon.as_mut().unwrap().reloading = Some(pre + lo + (hi - lo) * r);
            }
            return;
        }
        w.refire += dt;
        if w.refire >= w.class.fire_interval {
            w.refire -= w.class.fire_interval;
            self.bot_fire_ammunition(i);
            let w = self.bots[i].weapon.as_mut().unwrap();
            // WeaponFired: ShotCount, BurstOver -> PauseFiring
            w.shot_count += 1;
            if w.shot_count >= w.current_burst_length || w.ammo <= 0 {
                w.pressing = false;
                let (lo, hi) = (w.burst_delay_min, w.burst_delay_max);
                let r = self.frand();
                let w = self.bots[i].weapon.as_mut().unwrap();
                w.burst_wait = Some(0.03 + lo + (hi - lo) * r);
            }
        }
    }

    /// TdWeapon.FireAmmunition for a bot: GetAdjustedAimFor (the aim bot), the trace, and
    /// TdPlayerPawn.TakeDamage when it hits and the aim bot calls the hit relevant.
    fn bot_fire_ammunition(&mut self, i: usize) {
        let class = self.bots[i].weapon.as_ref().unwrap().class;
        self.bots[i].weapon.as_mut().unwrap().ammo -= 1;
        let aim = self.bot_aim_location(i);
        let b = &self.bots[i];
        // GetWeaponStartTraceLocation: the weapon hand, roughly eye height in front
        let start = b.location + b.rotation.vector() * 40.0 + Vec3::new(0.0, 0.0, b.base_eye_height - 15.0);
        let aim_dir = (aim - start).safe_normal();
        // the bot's fire anim (WeaponAnimationNode3p) and sound
        self.bots[i].anim.play(Slot::Weapon, "standfire", 1.0, 0.1, 0.2, false, false, false);
        self.sound(SoundEvent::CueAt { name: class.fire_3p.to_string(), location: start });
        // InstantFire, or the shotguns' PelletCount traces (an AI's first 3 impacts count)
        let mut stop = 1.0;
        let mut end = start + aim_dir * class.weapon_range;
        let mut hits = 0;
        for k in 0..class.pellets.max(1) {
            let (r1, r2) = (self.frand(), self.frand());
            let dir = add_spread(Rotator::from_vector(aim_dir), class.spread, r1, r2, 0.0);
            let e = start + dir * class.weapon_range;
            let (t, _) = self.trace_shot(start, e, Some(i));
            // the player's cylinder
            let p = &self.pawn;
            let pt = segment_cylinder(start, e, p.location, p.collision_radius, p.collision_height);
            let mut st = t;
            if let Some(pt) = pt.filter(|&pt| pt < t && !self.pawn.dying) {
                st = pt;
                let w = self.bots[i].weapon.as_ref().unwrap();
                // TdAimBot.IsHitRelevant: ShouldMiss when BaseOffset > OffsetThreshold
                let relevant = w.aim.base_offset <= w.aim.offset_threshold[w.firing_range as usize];
                if relevant && hits < 3 {
                    hits += 1;
                    let dist = (e - start).length() * pt;
                    // GetInstantHitDamage: AIDamageMultiplier for an AI instigator
                    let damage = class.instant_hit_damage(dist) * class.ai_damage_multiplier;
                    self.player_take_bullet_damage(damage as i32, i);
                }
            }
            if k == 0 {
                stop = st;
                end = e;
            }
        }
        let hit = start + (end - start) * stop;
        self.events.push(crate::sim::Event::Shot(Shot { start, end: hit, bot: Some(i) }));
    }

    /// TdPlayerPawn.TakeDamage from a bullet (PlayerBulletDamageMultiplier 1): the grunt,
    /// health and the regeneration clock. Bullets don't make the player stumble
    /// (TdPawn.BulletDamage is empty).
    fn player_take_bullet_damage(&mut self, damage: i32, _from: usize) {
        if self.pawn.dying || damage <= 0 {
            return;
        }
        self.sound(SoundEvent::Cue(PLAYER_HIT_SOUND.to_string()));
        self.pawn.time_since_last_damage = 0.0;
        self.pawn.health_frac = 0.0;
        self.take_damage(damage);
    }
}

/// TdAimBot.GetOffsetPoint: X along the sideways axis (TargetDir x Up(X)), Y up.
fn offset_point(start: Vec3, target: Vec3, x: f32, y: f32) -> Vec3 {
    let dir = (target - start).safe_normal();
    let up = Vec3::new(0.0, 0.0, x);
    let mut side = dir.cross(up);
    side.z += y;
    side
}
