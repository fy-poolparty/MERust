//! Enemy pawns (TdBotPawn). Two brains: the tutorial's melee sparring dummy (TdAI_MeleeDummy:
//! face the player, attack when within 200) and an armed patrol cop (TdAI_PatrolCop with
//! AITemplate_PatrolCop_Glock: shoots in bursts through TdAimBot, melees with the gun when the
//! player is within MeleeRange). TdMove_BotMelee for their swing, TdMove_StumbleBot for taking
//! hits, TdMove_DisarmedBot when the player snatches the gun, and TdBotPawn.PlayDying's death
//! animation; the presentation layer runs the ragdoll from the DeathAnimData timings.
//!
//! Not ported: TdAIController's movement (pathing, advancing, cover), blocking and dodging.

use crate::anim::{AnimEvent, AnimLib, AnimPlayer};
use crate::combat::{stumble_state, DamageType, StumbleHit, StumbleState};
use crate::math::{norm_axis, Rotator, UeVec, Vec3};
use crate::pawn::{Move, Slot};
use crate::sim::Sim;

/// TdBotPawn's MovementState as far as the port goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BotMove {
    Walking,
    Melee,
    Stumble,
    /// MOVE_Snatched: TdMove_DisarmedBot.
    Disarmed,
    /// MOVE_BotTurnStanding: a turn-in-place step (StandTurn45/90/135) while the yaw follows.
    TurnStanding,
    /// MOVE_Block: TdMove_BotBlock (HitMeleeBlock, immune to punches, shoves the player).
    Block,
    /// MOVE_StumbleFalling: TdMove_BotStumbleFalling, off a ledge (a sure death on landing).
    StumbleFalling,
    /// MOVE_MeleeAirAbove: TdMove_MeleeAirAboveBot, landed on by the player.
    MeleeAirAbove,
    Dying,
}

/// Which controller runs the bot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Brain {
    MeleeDummy,
    PatrolCop,
}

/// TdAIController states the patrol cop uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CopState {
    /// Advance's Fire label (standing: there's no path network to advance along).
    Combat,
    /// State Melee: SetMove(MOVE_Melee) and wait for MOVE_Walking.
    Melee,
    /// State Stumble.
    Stumble { poll: f32 },
    /// State CannedMove (being disarmed).
    Canned,
}

/// TdAI_MeleeDummy's states.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DummyState {
    /// WaitToMelee: TestMelee every 0.1 s.
    WaitToMelee,
    /// Melee1: Sleep(0.5), then SetMove(MOVE_Melee) and wait it out.
    Melee1 { sleep: f32, started: bool },
    /// TdAIController state Stumble (OnStumble from TdMove_StumbleBot.StartMove): not
    /// interruptable; polls every 0.1 s until the pawn is back in MOVE_Walking, then WhatToDo.
    Stumble { poll: f32 },
}

/// TdMove_BotMelee's MeleeAttackProperties (TdMove_MeleeDummy's GenericAttackProperties).
#[derive(Clone, Copy, Debug)]
pub struct MeleeAttackProperties {
    pub hit_angle: f32,
    pub hit_range: f32,
    pub attack_height_adjustment: f32,
    pub damage: f32,
    pub attack_speed: f32,
}

/// TdBotPawn.DeathAnimData ([TdGame.TdBotPawn] config).
#[derive(Clone, Copy, Debug)]
pub struct DeathAnim {
    pub anim: Option<&'static str>,
    pub speed: f32,
    pub root_motion: bool,
    pub pawn_impulse: f32,
    pub pawn_z_impulse: f32,
    pub bone_impulse: f32,
    pub gravity_modifier: f32,
    pub use_motors: bool,
    pub motor_strength: f32,
    pub time_to_enable_ragdoll: f32,
    pub time_to_blend_out_motors: f32,
    pub time_to_disable_motors: f32,
    pub time_to_bone_impulse: f32,
    pub time_to_full_ragdoll: f32,
}

#[allow(clippy::too_many_arguments)]
fn d(anim: Option<&'static str>, speed: f32, root_motion: bool, pawn_impulse: f32, pawn_z_impulse: f32, bone_impulse: f32, gravity_modifier: f32, use_motors: bool, motor_strength: f32, enable: f32, blend_out: f32, disable: f32, bone: f32, full: f32) -> DeathAnim {
    DeathAnim {
        anim,
        speed,
        root_motion,
        pawn_impulse,
        pawn_z_impulse,
        bone_impulse,
        gravity_modifier,
        use_motors,
        motor_strength,
        time_to_enable_ragdoll: enable,
        time_to_blend_out_motors: blend_out,
        time_to_disable_motors: disable,
        time_to_bone_impulse: bone,
        time_to_full_ragdoll: full,
    }
}

/// TdBotPawn.GetDeathAnim (DeathAnimType -> DeathAnimRagdoll ... DeathAnimDeathByAuto).
pub fn death_anim(t: u8) -> DeathAnim {
    match t {
        1 => d(Some("HitMeleeInAir_High"), 0.8, false, 500.0, 275.0, 0.0, 0.9, true, 2000.0, 0.1, 0.2, 0.3, 0.0, 0.3),
        2 => d(Some("HitMeleeOverEdge"), 0.75, false, 325.0, 125.0, 150.0, 0.5, true, 2000.0, 0.25, 0.3, 0.5, 0.26, 0.5),
        3 => d(Some("DeathByShotgun"), 0.9, false, 500.0, 350.0, 500.0, 0.9, true, 1000.0, 0.2, 0.45, 0.5, 0.0, 0.5),
        4 => d(Some("DeathByHeadShot"), 0.8, true, 50.0, 1.0, 500.0, 1.0, true, 200.0, 0.2, 0.25, 0.4, 0.21, 0.4),
        5 => d(Some("HitMeleeOverEdge"), 0.75, false, 550.0, 250.0, 400.0, 0.85, true, 2000.0, 0.35, 0.45, 0.5, 0.37, 0.5),
        6 => d(Some("HitMeleeRight"), 0.8, false, 400.0, 200.0, 0.0, 0.8, true, 20000.0, 0.2, 0.3, 0.6, 0.0, 0.2),
        7 => d(Some("HitMeleeWallrunRight"), 1.0, true, 0.0, 0.0, 0.0, 0.8, true, 2000.0, 0.0, 0.3, 0.75, 0.0, 0.75),
        8 => d(Some("HitMeleeLeft"), 0.8, false, 400.0, 200.0, 0.0, 0.8, true, 20000.0, 0.2, 0.3, 0.6, 0.0, 0.2),
        9 => d(Some("HitMeleeWallrunLeft"), 1.0, true, 0.0, 0.0, 0.0, 0.8, true, 2000.0, 0.0, 0.3, 0.75, 0.0, 0.75),
        10 => d(Some("DeathByAuto"), 0.8, false, 600.0, 175.0, 50.0, 0.9, true, 20000.0, 0.1, 0.4, 0.35, 0.15, 0.36),
        _ => d(None, 1.0, false, 0.0, 0.0, 0.0, 1.0, false, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
    }
}

/// GetDeathAnim for a bot's pawn class: DefaultAI.ini overrides some types for
/// [TdSpContent.TdBotPawn_PatrolCop_Remington] and [TdSpContent.TdBotPawn_Support].
pub fn death_anim_for(l: &crate::weapons::Loadout, t: u8) -> DeathAnim {
    let remington = l.weapon.is_some_and(|w| w.name == "TdWeapon_Shotgun_Remington870");
    let support = l.body.package == "CH_TKY_Cop_Support";
    match (remington, support, t) {
        (true, _, 3) => d(Some("DeathByShotgun"), 1.0, false, 500.0, 350.0, 500.0, 0.9, true, 1000.0, 0.2, 0.45, 0.5, 0.0, 0.5),
        (true, _, 5) => d(Some("HitMeleeOverEdge"), 0.75, false, 450.0, 300.0, 800.0, 0.82, true, 2000.0, 0.25, 0.4, 0.5, 0.27, 0.5),
        (true, _, 10) => d(Some("DeathByAuto"), 0.7, false, 500.0, 75.0, 150.0, 0.9, true, 20000.0, 0.1, 0.4, 0.35, 0.15, 0.36),
        (_, true, 2) => d(Some("HitMeleeOverEdge"), 0.5, false, 250.0, 75.0, 50.0, 1.0, true, 2000.0, 0.25, 0.3, 0.4, 0.0, 0.4),
        (_, true, 3) => d(Some("DeathByShotgun"), 0.7, false, 50.0, 50.0, 50.0, 1.0, true, 1000.0, 0.1, 0.15, 0.3, 0.0, 0.3),
        (_, true, 5) => d(Some("HitMeleeOverEdge"), 1.0, false, 300.0, 175.0, 50.0, 1.0, true, 2000.0, 0.1, 0.2, 0.15, 0.12, 0.15),
        (_, true, 10) => d(Some("DeathByAuto"), 0.8, false, 50.0, 100.0, 50.0, 1.0, true, 20000.0, 0.4, 0.3, 0.5, 0.45, 0.5),
        _ => death_anim(t),
    }
}


pub struct Bot {
    pub location: Vec3,
    pub rotation: Rotator,
    pub velocity: Vec3,
    pub falling: bool,
    pub gravity_modifier: f32,
    pub collision_radius: f32,
    pub collision_height: f32,
    pub base_eye_height: f32,
    pub health: i32,
    pub max_health: i32,
    pub movement_state: BotMove,
    pub anim: AnimPlayer,
    pub use_root_motion: bool,
    pub use_root_rotation: bool,
    /// The custom anim the current move played (TdMove.CurrentCustomAnimName).
    current_anim: Option<(Slot, String)>,
    /// TdMove.LastStopMoveTime per move, for RedoMoveTime.
    last_stop_move_time: f32,
    move_time: f32,
    // TdAI_MeleeDummy
    ai: DummyState,
    ai_test_timer: f32,
    /// Controller.RotationRate.Yaw while focused on the enemy.
    pub rotation_rate_yaw: f32,
    // TdMove_BotMelee
    pub attack: MeleeAttackProperties,
    melee_stage: u8,
    // TdMove_StumbleBot
    pub stumble_hit: StumbleHit,
    pub stumble_state: StumbleState,
    stand_melee_to_stand: bool,
    /// TdBotPawn.ActiveDeathAnimType: set by the attack that's about to land.
    pub active_death_anim_type: u8,
    pub consecutive_hit_count: i32,
    /// TdBotPawn.MeleeAttackTime (the last counted hit) and the block's 'bLock' invulnerability.
    melee_attack_time: f32,
    block_invulnerable: bool,
    block_timer: f32,
    /// A precise rotation in progress: (start yaw, delta, time) over 0.1 s.
    look_at: Option<(i32, i32, f32)>,
    pub dead_time: f32,
    /// The death's DeathAnimData and the hit that killed it (LastPhysHitInfo: location,
    /// momentum), for the ragdoll.
    pub death: Option<DeathAnim>,
    pub death_hit: (Vec3, Vec3),
    /// Where it started (respawned there after dying).
    pub home: Vec3,
    pub home_yaw: i32,
    pub brain: Brain,
    /// The AITemplate it was spawned from (gun, drops, disarm window, melee).
    pub loadout: &'static crate::weapons::Loadout,
    pub cop_state: CopState,
    /// The gun (TdBotPawn.Weapon) and its firing state.
    pub weapon: Option<crate::weapons::BotWeapon>,
    /// bForceWaitForDamage (DamageTime after a hit, 0.75 s after a stumble).
    pub force_wait_for_damage: f32,
    /// TdAIController.MeleePredictionTime.
    pub melee_prediction_time: f32,
    /// AITemplate MeleeRange / DisarmWindow.
    pub melee_range: f32,
    pub disarm_window: f32,
    /// TdMove_BotMelee.MoveActiveTime (QueryDisarmState reads it).
    pub melee_active_time: f32,
    /// EnemyVisible.
    pub enemy_visible: bool,
    /// TdMove_BotTurnStanding: InitialPawnRotationYaw, DeltaRotationYaw, TimeIntoRotation,
    /// RotationTime.
    turn_initial_yaw: i32,
    turn_delta_yaw: i32,
    turn_time: f32,
    turn_length: f32,
    /// TdMove_BotTurnStanding.bRotatePawn and the body's start / delta when it turns too.
    turn_rotate_pawn: bool,
    turn_initial_body_yaw: i32,
    turn_delta_body_yaw: i32,
    /// TdPawn.LegRotation: where the legs face. A standing bot's body (Rotation) follows the
    /// focus; the legs step round with BotTurnStanding (bUseLegRotationHack2), the upper body
    /// twisting between them.
    pub leg_yaw: i32,
}

impl Bot {
    pub fn new(lib: AnimLib, feet: Vec3, yaw: i32) -> Self {
        let height = 90.0;
        Bot {
            location: feet + Vec3::new(0.0, 0.0, height),
            rotation: Rotator::new(0, yaw, 0),
            velocity: Vec3::ZERO,
            falling: false,
            gravity_modifier: 1.0,
            collision_radius: 30.0,
            collision_height: height,
            base_eye_height: 60.0,
            health: 100,
            max_health: 100,
            movement_state: BotMove::Walking,
            anim: AnimPlayer::new(lib),
            use_root_motion: false,
            use_root_rotation: false,
            current_anim: None,
            last_stop_move_time: -10.0,
            move_time: 0.0,
            ai: DummyState::WaitToMelee,
            ai_test_timer: 0.0,
            rotation_rate_yaw: 15000.0,
            attack: MeleeAttackProperties { hit_angle: 140.0, hit_range: 120.0, attack_height_adjustment: 0.0, damage: 50.0, attack_speed: 1.0 },
            melee_stage: 0,
            stumble_hit: StumbleHit::default(),
            stumble_state: StumbleState::HitNone,
            stand_melee_to_stand: false,
            active_death_anim_type: 0,
            consecutive_hit_count: 0,
            melee_attack_time: -10.0,
            block_invulnerable: false,
            block_timer: 0.0,
            look_at: None,
            dead_time: 0.0,
            death: None,
            death_hit: (Vec3::ZERO, Vec3::ZERO),
            home: feet,
            home_yaw: yaw,
            brain: Brain::MeleeDummy,
            loadout: &crate::weapons::LOADOUTS[0],
            cop_state: CopState::Combat,
            weapon: None,
            force_wait_for_damage: 0.0,
            melee_prediction_time: 0.15,
            melee_range: 150.0,
            disarm_window: 0.1,
            melee_active_time: 0.0,
            enemy_visible: false,
            turn_initial_yaw: 0,
            turn_delta_yaw: 0,
            turn_time: 0.0,
            turn_length: 0.0,
            turn_rotate_pawn: false,
            turn_initial_body_yaw: 0,
            turn_delta_body_yaw: 0,
            leg_yaw: yaw,
        }
    }

    /// An armed patrol cop with the Glock (AITemplate_PatrolCop_Glock).
    pub fn new_patrol_cop(lib: AnimLib, feet: Vec3, yaw: i32) -> Self {
        Bot::with_loadout(lib, feet, yaw, &crate::weapons::LOADOUTS[1])
    }

    /// A bot from one of the spawnable AITemplates (unarmed: the sparring dummy).
    pub fn with_loadout(lib: AnimLib, feet: Vec3, yaw: i32, loadout: &'static crate::weapons::Loadout) -> Self {
        let mut b = Bot::new(lib, feet, yaw);
        b.apply_loadout(loadout);
        b
    }

    fn apply_loadout(&mut self, l: &'static crate::weapons::Loadout) {
        self.loadout = l;
        let (hit_angle, hit_range, damage) = l.melee;
        self.attack = MeleeAttackProperties { hit_angle, hit_range, attack_height_adjustment: 0.0, damage, attack_speed: 1.0 };
        let Some(w) = l.weapon else { return };
        self.brain = Brain::PatrolCop;
        self.weapon = Some(crate::weapons::BotWeapon::new(w));
        // TdBotPawn MaxRotationSpeed caps the controller's RotationRate
        self.rotation_rate_yaw = 25000.0;
        self.disarm_window = l.disarm_window;
        self.melee_range = 150.0;
    }

    pub fn alive(&self) -> bool {
        self.health > 0
    }

    fn play(&mut self, slot: Slot, name: &str, rate: f32, blend_in: f32, blend_out: f32, root_motion: bool, root_rotation: bool) {
        self.anim.play(slot, name, rate, blend_in, blend_out, false, root_motion, root_rotation);
        self.current_anim = self.anim.current(slot).map(|_| (slot, name.to_string()));
    }

    /// Respawn at its start.
    pub fn reset(&mut self) {
        let lib = self.anim.lib.clone();
        let (home, yaw, l) = (self.home, self.home_yaw, self.loadout);
        *self = Bot::with_loadout(lib, home, yaw, l);
    }
}

impl Sim {
    /// All the bots' ticks: brain, moves, animation, then movement.
    pub(crate) fn tick_bots(&mut self, dt: f32) {
        for i in 0..self.bots.len() {
            self.tick_bot(i, dt);
        }
    }

    fn tick_bot(&mut self, i: usize, dt: f32) {
        self.bots[i].move_time += dt;
        {
            let b = &mut self.bots[i];
            b.force_wait_for_damage = (b.force_wait_for_damage - dt).max(0.0);
            if b.movement_state == BotMove::Melee {
                b.melee_active_time += dt;
            }
        }
        if self.bots[i].movement_state == BotMove::TurnStanding {
            let b = &mut self.bots[i];
            b.turn_time = (b.turn_time + dt).min(b.turn_length);
            let f = if b.turn_length > 0.0 { b.turn_time / b.turn_length } else { 1.0 };
            b.leg_yaw = norm_axis(b.turn_initial_yaw + (b.turn_delta_yaw as f32 * f) as i32);
            if b.turn_rotate_pawn {
                b.rotation.yaw = norm_axis(b.turn_initial_body_yaw + (b.turn_delta_body_yaw as f32 * f) as i32);
            }
        }
        if self.bots[i].movement_state == BotMove::Dying {
            self.bots[i].dead_time += dt;
        } else if matches!(self.bots[i].movement_state, BotMove::StumbleFalling | BotMove::MeleeAirAbove | BotMove::Block) {
            if self.bots[i].movement_state == BotMove::Block {
                self.bot_block_tick(i, dt);
            }
            // OnStumble / StartCannedMove: the controller waits for the move
            self.tick_bot_weapon(i, dt, false, false);
        } else {
            match self.bots[i].brain {
                Brain::MeleeDummy => self.bot_ai(i, dt),
                Brain::PatrolCop => self.cop_ai(i, dt),
            }
        }
        if let Some((from, delta, t)) = self.bots[i].look_at {
            let t = (t + dt).min(0.1);
            let b = &mut self.bots[i];
            b.rotation.yaw = norm_axis(from + (delta as f32 * t / 0.1) as i32);
            b.look_at = if t >= 0.1 { None } else { Some((from, delta, t)) };
        }
        // UpdateLegRotation: outside a standing idle / turn the legs go with the body
        if !matches!(self.bots[i].movement_state, BotMove::Walking | BotMove::TurnStanding) {
            self.bots[i].leg_yaw = self.bots[i].rotation.yaw;
        }
        // animation, root motion and its events
        let (rm, yaw) = {
            let b = &mut self.bots[i];
            let rm = b.anim.tick_root(dt, b.use_root_motion, b.rotation.yaw);
            (rm, b.anim.root_rotation_delta)
        };
        if self.bots[i].use_root_rotation && yaw != 0 {
            self.bots[i].rotation.yaw += yaw;
        }
        let events = self.bots[i].anim.take_events();
        for e in events {
            match e {
                AnimEvent::End { slot, name, .. } => {
                    let current = self.bots[i].current_anim.as_ref().is_some_and(|(s, n)| *s == slot && n.eq_ignore_ascii_case(&name));
                    if current {
                        self.bot_on_custom_anim_end(i);
                    }
                }
                AnimEvent::CeaseRelevantRootMotion { .. } => {
                    if self.bots[i].movement_state == BotMove::Stumble {
                        // TdMove_StumbleBot.OnCeaseRelevantRootMotion
                        let b = &mut self.bots[i];
                        b.use_root_motion = false;
                        b.use_root_rotation = false;
                        b.velocity = Vec3::ZERO;
                    }
                }
                AnimEvent::Notify(_) => {}
            }
        }
        self.bot_physics(i, dt, rm);
    }

    /// TdAI_MeleeDummy: SetFocus(Enemy) while it's visible, WaitToMelee / Melee1.
    fn bot_ai(&mut self, i: usize, dt: f32) {
        if self.pawn.dying {
            return;
        }
        let enemy = self.pawn.location;
        let visible = self.bot_visible_to_player(i);
        // the controller's focus turns the pawn (in place: BotTurnStanding past 22.5 degrees)
        if visible && matches!(self.bots[i].movement_state, BotMove::Walking | BotMove::TurnStanding) {
            self.bot_face(i, enemy, dt);
        }
        let b = &mut self.bots[i];
        let dist = (enemy - b.location).length();
        match b.ai {
            DummyState::WaitToMelee => {
                b.ai_test_timer += dt;
                if b.ai_test_timer >= 0.1 {
                    b.ai_test_timer -= 0.1;
                    // TestMelee
                    if visible && dist < 200.0 {
                        b.ai = DummyState::Melee1 { sleep: 0.5, started: false };
                    }
                }
            }
            DummyState::Stumble { poll } => {
                let poll = poll + dt;
                if poll >= 0.1 {
                    // WhatToDo -> TestCombatTransitions -> WaitToMelee
                    b.ai = if b.movement_state == BotMove::Walking { DummyState::WaitToMelee } else { DummyState::Stumble { poll: 0.0 } };
                } else {
                    b.ai = DummyState::Stumble { poll };
                }
            }
            DummyState::Melee1 { sleep, started } => {
                if !started {
                    let sleep = sleep - dt;
                    if sleep > 0.0 {
                        b.ai = DummyState::Melee1 { sleep, started: false };
                    } else {
                        b.ai = DummyState::Melee1 { sleep: 0.0, started: true };
                        self.bot_set_move(i, BotMove::Melee);
                    }
                } else if self.bots[i].movement_state != BotMove::Melee {
                    // PopState
                    self.bots[i].ai = DummyState::WaitToMelee;
                }
            }
        }
    }

    fn bot_set_move(&mut self, i: usize, m: BotMove) {
        let old = self.bots[i].movement_state;
        if old == m && m != BotMove::Stumble {
            return;
        }
        if old != m {
            // StopMove
            let t = self.time;
            let b = &mut self.bots[i];
            b.last_stop_move_time = t;
            b.use_root_motion = false;
            b.use_root_rotation = false;
            b.current_anim = None;
            if old == BotMove::Stumble {
                b.velocity = Vec3::ZERO;
                b.anim.stop(Slot::Canned, 0.2);
            }
            if old == BotMove::TurnStanding {
                // StopMove: the rotation is wherever the turn got to
                b.anim.stop(Slot::FullBody, 0.0);
            }
        }
        self.bots[i].movement_state = m;
        self.bots[i].move_time = 0.0;
        match m {
            BotMove::Melee => {
                self.bots[i].melee_active_time = 0.0;
                self.bot_melee_start(i)
            }
            BotMove::Stumble => self.bot_stumble_start(i),
            BotMove::Disarmed => self.bot_disarmed_start(i),
            BotMove::TurnStanding => self.bot_turn_standing_start(i),
            BotMove::StumbleFalling => self.bot_stumble_falling_start(i),
            BotMove::Block => self.bot_block_start(i),
            BotMove::Walking | BotMove::Dying | BotMove::MeleeAirAbove => {}
        }
    }

    // ------------------------------------------------------------------ TdMove_BotMelee

    /// TdMove_BotMelee.StartMove + TriggerMove (full-body: MeleeStart with root motion).
    fn bot_melee_start(&mut self, i: usize) {
        let b = &mut self.bots[i];
        b.melee_stage = 0;
        b.use_root_motion = true;
        let speed = b.attack.attack_speed;
        b.play(Slot::Canned, "MeleeStart", speed, 0.1, 0.0, true, false);
        if b.current_anim.is_none() {
            self.bot_set_move(i, BotMove::Walking);
        }
    }

    /// TdMove_BotMelee.TestHit: the player inside HitAngle and HitRange, not sliding.
    fn bot_melee_test_hit(&self, i: usize) -> bool {
        let b = &self.bots[i];
        let p = &self.pawn;
        if b.attack.attack_height_adjustment >= 0.0 && (matches!(p.movement_state, Move::Slide | Move::MeleeSlide) || self.time - p.slide_stopped_time_stamp < 0.35) {
            return false;
        }
        let cos_half = (6.28 * b.attack.hit_angle / 720.0).cos();
        let mut to = p.location - b.location;
        let height = to.z - b.attack.attack_height_adjustment;
        to.z = 0.0;
        let dist = to.length();
        let mut facing = b.rotation.vector();
        facing.z = 0.0;
        facing.safe_normal().dot(to.safe_normal()) > cos_half && dist < b.attack.hit_range && height.abs() < 150.0
    }

    /// TdMove_BotMelee.OnCustomAnimEnd and TdMove_StumbleBot.OnCustomAnimEnd.
    fn bot_on_custom_anim_end(&mut self, i: usize) {
        match self.bots[i].movement_state {
            BotMove::Melee => {
                if self.bots[i].melee_stage == 0 {
                    let hit = self.bot_melee_test_hit(i);
                    // TriggerHit / TriggerMiss (no second swing for the dummy): MeleeEnd
                    let b = &mut self.bots[i];
                    b.use_root_motion = true;
                    // TdMove_Melee_PatrolCop.TriggerMiss plays MeleeMiss
                    let anim = if !hit && b.loadout.melee_miss_anim { "MeleeMiss" } else { "MeleeEnd" };
                    b.play(Slot::Canned, anim, 1.0, 0.0, 0.1, true, false);
                    if hit {
                        self.bot_trigger_hit_player(i);
                    }
                    // OnAfterFirstAnimation
                    self.bots[i].melee_stage = 1;
                } else {
                    self.bot_set_move(i, BotMove::Walking);
                }
            }
            BotMove::TurnStanding => {
                // OnCustomAnimEnd: SetMove(MOVE_Walking)
                let b = &mut self.bots[i];
                b.leg_yaw = norm_axis(b.turn_initial_yaw + b.turn_delta_yaw);
                if b.turn_rotate_pawn {
                    b.rotation.yaw = norm_axis(b.turn_initial_body_yaw + b.turn_delta_body_yaw);
                }
                self.bot_set_move(i, BotMove::Walking);
            }
            BotMove::Disarmed => {
                // TdMove_Disarmed.OnCustomAnimEnd: drop the inventory, die (DeathAnimType 0: ragdoll)
                self.bots[i].weapon = None;
                self.bots[i].active_death_anim_type = 0;
                let h = self.bots[i].health;
                let neck = self.bots[i].location + Vec3::new(0.0, 0.0, 60.0);
                self.bot_take_damage(i, h, neck, Vec3::ZERO, DamageType::MeleeDisarm);
            }
            BotMove::Block => {
                // TdMove_BotBlock.OnCustomAnimEnd: SetMove(MOVE_Walking); StopMove: OnStoppedBlocking
                self.bots[i].block_invulnerable = false;
                self.bots[i].cop_state = CopState::Combat;
                self.bot_set_move(i, BotMove::Walking);
            }
            BotMove::StumbleFalling => {
                // OnCustomAnimEnd: UseRootMotion(false); SetPhysics(PHYS_Falling); the tree's
                // falling branch loops HitMeleeOverEdgeLoop until it lands
                let b = &mut self.bots[i];
                b.use_root_motion = false;
                b.falling = true;
                b.anim.play(Slot::Canned, "HitMeleeOverEdgeLoop", 1.0, 0.1, 0.1, true, false, false);
                b.current_anim = None;
            }
            BotMove::MeleeAirAbove => {
                // TdMove_MeleeAirAboveBot.OnCustomAnimEnd: SetMove(MOVE_Walking); OnMeleedFromAir
                self.bots[i].cop_state = CopState::Combat;
                self.bot_set_move(i, BotMove::Walking);
            }
            BotMove::Stumble => {
                let b = &mut self.bots[i];
                b.velocity = Vec3::ZERO;
                if b.stand_melee_to_stand {
                    b.stand_melee_to_stand = false;
                    b.anim.stop(Slot::Canned, 0.2);
                    b.play(Slot::UpperBody, "StandMeleeToStand", 1.0, 0.2, 0.2, false, false);
                } else {
                    self.bot_set_move(i, BotMove::Walking);
                }
            }
            _ => {}
        }
    }

    /// TdMove_BotMelee.TriggerHitPlayer.
    fn bot_trigger_hit_player(&mut self, i: usize) {
        let b = &self.bots[i];
        let damage = b.attack.damage as i32;
        let momentum = b.rotation.vector() * 600.0;
        // HitLocation: the bot's right hand (its mesh isn't in the sim: roughly shoulder height)
        let hit = b.location + b.rotation.vector() * 60.0 + Vec3::new(0.0, 0.0, 50.0);
        self.player_take_melee_damage(damage, i, hit, momentum, DamageType::MeleeLeft);
        self.sound(crate::sound::SoundEvent::MeleeImpact { impact: crate::combat::MeleeImpact::Fist, head: true });
    }

    // ------------------------------------------------------------------ taking hits

    /// TdBotPawn.TakeDamage from the player's melee: health, the impact sound, the hit
    /// reaction (TdBotPawn.StumbleDamage -> MA_Stumble) or dying.
    pub fn bot_take_damage(&mut self, i: usize, damage: i32, hit_location: Vec3, momentum: Vec3, t: DamageType) {
        if !self.bots[i].alive() {
            return;
        }
        // IsInvulnerableToThisDamageType: TdMove_BotBlock's 'bLock' set
        if self.bots[i].block_invulnerable && matches!(t, DamageType::MeleeAir | DamageType::MeleeCrouch | DamageType::MeleeLeft | DamageType::MeleeRight | DamageType::MeleeSoccerKick) {
            return;
        }
        let p_loc = self.pawn.location;
        let p_rot = self.pawn.rotation;
        let b = &mut self.bots[i];
        // TdPawn.AdjustDamage: the melee armor (only for hits on the mesh: not the disarm's
        // finishing damage or a fall, which pass no HitComponent)
        let damage = if matches!(t, DamageType::MeleeDisarm | DamageType::Fell) { damage } else { 0.max((damage as f32 * (1.0 - b.loadout.armor.1)) as i32) };
        b.health -= damage;
        b.stumble_hit = StumbleHit { instigator_location: p_loc, instigator_rotation: p_rot, damage_location: hit_location, momentum, damage_type: Some(t) };
        // the hit bone's material: the leg kicks (slide, crouch) hit the body, the rest the neck
        let head = !matches!(t, DamageType::MeleeSlide | DamageType::MeleeCrouch);
        // TdBotPawn.PlayMeleeImpact: none for the disarm, a fall, or MeleeAirAbove
        if !matches!(t, DamageType::MeleeDisarm | DamageType::Fell | DamageType::MeleeAirAbove) {
            self.sound(crate::sound::SoundEvent::MeleeImpact { impact: crate::combat::impact_type(t), head });
        }
        self.bots[i].force_wait_for_damage = 0.4;
        if self.bots[i].health <= 0 {
            self.bot_play_dying(i);
            return;
        }
        if matches!(t, DamageType::MeleeDisarm | DamageType::Fell | DamageType::MeleeAirAbove) {
            return;
        }
        // TdBotPawn.StumbleDamage: counts the combo, then HandleMoveAction(MA_Stumble)
        if matches!(t, DamageType::Melee | DamageType::MeleeLeft | DamageType::MeleeRight | DamageType::MeleeSoccerKick) && self.bot_block_test_hit(i) {
            let now = self.time;
            let b = &mut self.bots[i];
            b.consecutive_hit_count += 1;
            b.melee_attack_time = now;
        }
        // TdMove_StumbleBot.CanDoMove: RedoMoveTime (none) only
        self.bot_set_move(i, BotMove::Stumble);
    }

    /// TdMove_StumbleBot.StartMove + PlayStumbleAnimation.
    fn bot_stumble_start(&mut self, i: usize) {
        let b = &mut self.bots[i];
        // BotOwner.OnStumble -> the controller's Stumble state
        b.ai = DummyState::Stumble { poll: 0.0 };
        b.cop_state = CopState::Stumble { poll: 0.0 };
        b.stumble_state = stumble_state(&b.stumble_hit, b.location, b.rotation, b.collision_height);
        b.stand_melee_to_stand = false;
        b.use_root_motion = true;
        b.anim.stop(Slot::UpperBody, 0.1);
        use StumbleState as S;
        let (anim, smts, root_rot) = match b.stumble_state {
            S::HitMeleeFrontLeft => (Some("HitMeleeLeft"), true, false),
            S::HitMeleeFrontRight => (Some("HitMeleeRight"), true, false),
            S::HitMeleeCrouchFront => (Some("HitMeleeCrouchSweep"), true, false),
            S::HitMeleeSlideFront => (Some("HitMeleeSlide"), true, false),
            S::HitMeleeWallrunRight => (Some("HitMeleeWallrunRight"), false, true),
            S::HitMeleeWallrunLeft => (Some("HitMeleeWallrunLeft"), false, true),
            S::HitMeleeAirHeadFront => (Some("HitMeleeInAir_High"), true, false),
            S::HitMeleeAirBodyFront | S::HitMeleeVaultKick => (Some("HitMeleeInAir_Low"), true, false),
            S::HitMeleeSoccerKick => (Some("HitMeleeSoccerKick"), true, false),
            S::HitMeleeBack | S::HitMeleeBackHead => (Some("HitMeleeBack"), true, true),
            _ => (None, false, false),
        };
        b.use_root_rotation = root_rot;
        match anim {
            Some(a) => {
                b.stand_melee_to_stand = smts;
                b.play(Slot::Canned, a, 1.0, 0.1, -1.0, true, root_rot);
                if b.current_anim.is_none() {
                    self.bot_set_move(i, BotMove::Walking);
                }
            }
            None => self.bot_set_move(i, BotMove::Walking),
        }
    }

    /// TdBotPawn.PlayDying + PlayDeathAnim (the ragdoll that takes over isn't ported: the
    /// death animation plays out and the body stays where it ends).
    fn bot_play_dying(&mut self, i: usize) {
        self.bot_set_move(i, BotMove::Dying);
        // TdPawn.TossInventory: the gun drops (MainWeaponAmmoDrops_Dropped)
        if let Some(w) = self.bots[i].weapon.take() {
            let b = &self.bots[i];
            let (x, y, z) = b.rotation.axes();
            let velocity = b.velocity * 2.0 + (x + y + z) * 25.0;
            let location = b.location + x * 30.0 + y * 20.0 + Vec3::new(0.0, 0.0, 20.0);
            let ammo = b.loadout.ammo_dropped;
            self.pickups.push(crate::weapons::Pickup { class: w.class, ammo, location, velocity, rotation: b.rotation, resting: false });
        }
        let b = &mut self.bots[i];
        let d = death_anim_for(b.loadout, b.active_death_anim_type);
        // StopAllCustomAnimations blends out; a pure ragdoll (no death anim) takes over from the
        // pose the bot is in, so that one keeps its last frame
        if d.anim.is_some() {
            b.anim.stop_all();
        }
        b.death = Some(d);
        b.death_hit = (b.stumble_hit.damage_location, b.stumble_hit.momentum);
        let dir = b.stumble_hit.momentum.safe_normal();
        // ImpulseHisAss: Impulse = Normal(Momentum) * PawnImpulse with Z replaced by
        // PawnZImpulse, added to the velocity
        if d.pawn_impulse > 0.0 {
            let mut impulse = dir * d.pawn_impulse;
            impulse.z = d.pawn_z_impulse;
            b.velocity += impulse;
            b.falling = true;
            b.gravity_modifier = d.gravity_modifier;
        }
        if let Some(a) = d.anim {
            b.use_root_motion = d.root_motion;
            b.play(Slot::Canned, a, d.speed, 0.2, -1.0, d.root_motion, d.root_motion);
        }
    }

    // ------------------------------------------------------------------ movement

    /// PHYS_Walking / PHYS_Falling for a bot: root motion or velocity, blocked by the world
    /// and the player's cylinder, standing on the floor below it.
    fn bot_physics(&mut self, i: usize, dt: f32, root_motion: Vec3) {
        let g = self.pawn.world_gravity_z;
        let (player, pr, ph) = (self.pawn.location, self.pawn.collision_radius, self.pawn.collision_height);
        let b = &mut self.bots[i];
        let ext = Vec3::new(b.collision_radius, b.collision_radius, b.collision_height);
        if b.falling {
            b.velocity.z += g * b.gravity_modifier * dt;
        }
        let mut delta = if b.use_root_motion && !b.falling { root_motion } else { b.velocity * dt };
        if b.use_root_motion && b.falling {
            delta += root_motion;
        }
        // horizontal: blocked by the player (cylinders)
        let to_player = Vec3::new(player.x - (b.location.x + delta.x), player.y - (b.location.y + delta.y), 0.0);
        let min = b.collision_radius + pr;
        if to_player.length() < min && (player.z - b.location.z).abs() < b.collision_height + ph && b.movement_state != BotMove::Dying {
            let push = to_player.safe_normal();
            let into = Vec3::new(delta.x, delta.y, 0.0).dot(push);
            if into > 0.0 {
                delta -= push * into;
            }
        }
        let start = b.location;
        let hit = self.world.line_check(start + delta, start, ext);
        let b = &mut self.bots[i];
        b.location = start + delta * hit.time;
        // bCanWalkOffLedges is false: a standing bot doesn't leave an edge (walking, stumbling,
        // shoved) unless the drop is deadly, twice its height (by request: the game lets a
        // stumble carry it over any drop and the landing always kills)
        if !b.falling && b.alive() && !matches!(b.movement_state, BotMove::StumbleFalling | BotMove::Dying) {
            let (h, r) = (b.collision_height, b.collision_radius);
            let at = b.location;
            let probe = Vec3::new(r * 0.5, r * 0.5, h);
            let floor = self.world.line_check(at - Vec3::new(0.0, 0.0, h + 40.0), at, probe);
            if !floor.hit {
                let down = self.world.line_check(at - Vec3::new(0.0, 0.0, 4000.0), at, Vec3::ZERO);
                let drop = if down.hit { (at.z - h) - down.location.z } else { f32::MAX };
                if drop < 4.0 * h {
                    let b = &mut self.bots[i];
                    b.location = Vec3::new(start.x, start.y, b.location.z);
                    b.velocity.x = 0.0;
                    b.velocity.y = 0.0;
                }
            }
        }
        let b = &mut self.bots[i];
        if hit.hit && hit.normal.z < 0.7 {
            // slide along walls
            let rest = delta * (1.0 - hit.time);
            let slide = rest - hit.normal * rest.dot(hit.normal);
            let s = b.location;
            let h2 = self.world.line_check(s + slide, s, ext);
            self.bots[i].location = s + slide * h2.time;
        }
        // floor
        let b = &mut self.bots[i];
        let down = self.world.line_check(b.location - Vec3::new(0.0, 0.0, 4.0), b.location, ext);
        let b = &mut self.bots[i];
        if down.hit && down.normal.z > 0.7 {
            if b.falling && b.velocity.z <= 0.0 {
                let land_vz = b.velocity.z;
                b.falling = false;
                b.velocity = Vec3::ZERO;
                self.bot_landed(i, down.normal, land_vz);
            }
        } else if !b.falling {
            b.falling = true;
            self.bot_falling(i);
        }
    }

    /// Moves[MOVE_MeleeAirAbove].CanDoMove for a bot (TdMove.CanDoMove: not dying; not in
    /// another canned move).
    pub(crate) fn bot_can_do_air_above(&self, i: usize) -> bool {
        let b = &self.bots[i];
        b.alive() && !matches!(b.movement_state, BotMove::Disarmed | BotMove::StumbleFalling | BotMove::MeleeAirAbove | BotMove::Dying)
    }

    /// TdAIController.StartCannedMove(MOVE_MeleeAirAbove): bDisableCollision, waits for
    /// TriggerCannedAnim.
    pub(crate) fn bot_start_canned_air_above(&mut self, i: usize) -> bool {
        if !self.bot_can_do_air_above(i) {
            return false;
        }
        if let Some(w) = self.bots[i].weapon.as_mut() {
            w.pressing = false;
        }
        self.bot_set_move(i, BotMove::MeleeAirAbove);
        self.bots[i].cop_state = CopState::Canned;
        true
    }

    /// TdAIController.TriggerCannedAnim(MOVE_MeleeAirAbove, 'MarioMove') ->
    /// TdMove_MeleeAirAboveBot.PlayCannedAnim.
    pub(crate) fn bot_trigger_canned_air_above(&mut self, i: usize) {
        if self.bots[i].movement_state != BotMove::MeleeAirAbove {
            return;
        }
        let b = &mut self.bots[i];
        b.use_root_motion = true;
        b.use_root_rotation = true;
        b.play(Slot::Canned, "MarioMove", 1.0, 0.0, 0.2, true, true);
    }

    /// TdMove_BotBlock.TestHit (GenericAttackProperties HitAngle 75, HitRange 260): the
    /// player in front and in reach.
    fn bot_block_test_hit(&self, i: usize) -> bool {
        let b = &self.bots[i];
        let p = &self.pawn;
        if matches!(p.movement_state, Move::Slide | Move::MeleeSlide) || self.time - p.slide_stopped_time_stamp < 0.35 {
            return false;
        }
        let cos_half = (6.28 * 75.0f32 / 720.0).cos();
        let mut to = p.location - b.location;
        let height = to.z;
        to.z = 0.0;
        let mut facing = b.rotation.vector();
        facing.z = 0.0;
        facing.safe_normal().dot(to.safe_normal()) > cos_half && to.length() < 260.0 && height.abs() < 150.0
    }

    /// TdBotPawn.PrepareForMeleeAttack (TdMove_MeleeBase.UpdateTargetPawn): CanBlock and
    /// ShouldBlock (MeleeAttackLimit punches in a row within BlockResetTime 1.8 s) -> block.
    pub(crate) fn bot_prepare_for_melee_attack(&mut self, i: usize, t: DamageType) {
        let b = &self.bots[i];
        if !b.alive() || b.loadout.melee_attack_limit < 0 || matches!(b.movement_state, BotMove::Disarmed | BotMove::StumbleFalling | BotMove::MeleeAirAbove | BotMove::Dying | BotMove::Block) {
            return;
        }
        if !self.bot_block_test_hit(i) {
            return;
        }
        if self.time - b.melee_attack_time > 1.8 {
            self.bots[i].consecutive_hit_count = 0;
        }
        let should = matches!(t, DamageType::Melee | DamageType::MeleeLeft | DamageType::MeleeRight) && self.bots[i].consecutive_hit_count >= self.bots[i].loadout.melee_attack_limit;
        if should {
            // StartBlocking: the controller's Blocking state, counters reset
            self.bots[i].consecutive_hit_count = 0;
            self.bot_set_move(i, BotMove::Block);
        }
    }

    /// TdMove_BotBlock.StartMove + TriggerMove.
    fn bot_block_start(&mut self, i: usize) {
        if let Some(w) = self.bots[i].weapon.as_mut() {
            w.pressing = false;
        }
        let b = &mut self.bots[i];
        b.cop_state = CopState::Canned;
        b.velocity = Vec3::ZERO;
        b.block_invulnerable = true;
        b.block_timer = 0.2;
        b.anim.stop(Slot::CannedUpperBody, 0.1);
        b.use_root_motion = true;
        b.play(Slot::Canned, "HitMeleeBlock", 1.0, 0.1, 0.1, true, false);
        if b.current_anim.is_none() {
            b.block_invulnerable = false;
            self.bot_set_move(i, BotMove::Walking);
        }
    }

    /// TdMove_BotBlock's 0.2 s timer: OnStartPlayerStumble (TestHit -> TriggerHitPlayer, a
    /// 50 damage TdDmgType_Shove), then the punches hurt again.
    fn bot_block_tick(&mut self, i: usize, dt: f32) {
        let b = &mut self.bots[i];
        if b.block_timer <= 0.0 {
            return;
        }
        b.block_timer -= dt;
        if b.block_timer > 0.0 {
            return;
        }
        if self.bot_block_test_hit(i) {
            let b = &self.bots[i];
            let momentum = b.rotation.vector() * 600.0;
            let hit = b.location + b.rotation.vector() * 60.0 + Vec3::new(0.0, 0.0, 50.0);
            self.player_take_melee_damage(50, i, hit, momentum, DamageType::Melee);
            self.sound(crate::sound::SoundEvent::MeleeImpact { impact: crate::combat::MeleeImpact::Fist, head: false });
        }
        self.bots[i].block_invulnerable = false;
    }

    /// TdBotPawn.Falling: off a ledge with nothing within 300 below, MOVE_StumbleFalling.
    fn bot_falling(&mut self, i: usize) {
        let b = &self.bots[i];
        if !b.alive() || matches!(b.movement_state, BotMove::StumbleFalling | BotMove::Dying) {
            return;
        }
        let clear = !self.world.line_check(b.location - Vec3::new(0.0, 0.0, 300.0), b.location, Vec3::ZERO).hit;
        if clear {
            self.bot_set_move(i, BotMove::StumbleFalling);
        }
    }

    /// TdMove_BotStumbleFalling.StartMove + StartAnimation.
    fn bot_stumble_falling_start(&mut self, i: usize) {
        let b = &mut self.bots[i];
        b.anim.stop_all();
        b.use_root_motion = true;
        b.play(Slot::Canned, "HitMeleeOverEdge", 1.0, 0.5, 0.1, true, false);
    }

    /// TdBotPawn.Landed: TdMove_BotStumbleFalling.Landed takes 100 TdDmgType_Fell.
    fn bot_landed(&mut self, i: usize, normal: Vec3, vz: f32) {
        if self.bots[i].movement_state != BotMove::StumbleFalling || !self.bots[i].alive() {
            return;
        }
        let momentum = normal * (-0.5 * vz);
        let loc = self.bots[i].location;
        self.bots[i].active_death_anim_type = 0;
        self.bots[i].stumble_hit.momentum = momentum;
        self.bot_take_damage(i, 100, loc, momentum, DamageType::Fell);
    }
}

/// TdAIController.EDisarmState.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisarmState {
    NotPossible,
    Miss,
    Stage1,
}

impl Sim {
    /// TdAI_PatrolCop (TdAIController): focus on the enemy, TestCombatTransitions (melee when
    /// ShouldEnterMelee, otherwise stand and fire), Stumble, CannedMove.
    fn cop_ai(&mut self, i: usize, dt: f32) {
        if self.pawn.dying {
            self.tick_bot_weapon(i, dt, false, false);
            return;
        }
        let visible = self.bot_visible_to_player(i);
        self.bots[i].enemy_visible = visible;
        let enemy = self.pawn.location;
        // the controller's focus turns the pawn (in place: BotTurnStanding past 22.5 degrees)
        {
            let b = &self.bots[i];
            let turn = matches!(b.cop_state, CopState::Combat | CopState::Melee) && matches!(b.movement_state, BotMove::Walking | BotMove::TurnStanding);
            if visible && turn {
                self.bot_face(i, enemy, dt);
            }
        }
        let mut fire = false;
        match self.bots[i].cop_state {
            CopState::Combat => {
                if self.bots[i].movement_state == BotMove::Walking && self.should_enter_melee(i, self.bots[i].melee_range) {
                    // state Melee: StopFiring, SetMove(MOVE_Melee)
                    self.bots[i].cop_state = CopState::Melee;
                    self.bot_set_move(i, BotMove::Melee);
                } else {
                    // TdMove_BotTurnStanding only Freeze()s the movement: the cop keeps firing
                    fire = visible && matches!(self.bots[i].movement_state, BotMove::Walking | BotMove::TurnStanding);
                }
            }
            CopState::Melee => {
                if self.bots[i].movement_state != BotMove::Melee {
                    // EndState: MeleePredictionTime = GetRandomMeleePredictionTime
                    let r = self.frand();
                    let b = &mut self.bots[i];
                    b.melee_prediction_time = 0.1 + 0.1 * r;
                    b.cop_state = CopState::Combat;
                }
            }
            CopState::Stumble { poll } => {
                let poll = poll + dt;
                let b = &mut self.bots[i];
                if poll >= 0.1 {
                    if b.movement_state == BotMove::Walking {
                        // EndState: SetForceWaitForDamageTimer(0.75); WhatToDo
                        b.force_wait_for_damage = b.force_wait_for_damage.max(0.75);
                        b.cop_state = CopState::Combat;
                    } else {
                        b.cop_state = CopState::Stumble { poll: 0.0 };
                    }
                } else {
                    b.cop_state = CopState::Stumble { poll };
                }
            }
            CopState::Canned => {}
        }
        self.tick_bot_weapon(i, dt, fire, visible);
    }

    /// TdAIController.ShouldEnterMelee.
    fn should_enter_melee(&self, i: usize, range: f32) -> bool {
        let b = &self.bots[i];
        let p = &self.pawn;
        if !b.enemy_visible {
            return false;
        }
        // IsPredictedPositionWithinMeleeRange
        let predicted = p.location + p.velocity * b.melee_prediction_time;
        let d2 = Vec3::new(predicted.x - b.location.x, predicted.y - b.location.y, 0.0).length();
        if d2 >= range {
            return false;
        }
        if (b.location.z - p.location.z).abs() >= 50.0 && p.movement_state != Move::Jump {
            return false;
        }
        // IsEnemyBehindMe
        if b.rotation.vector().dot((p.location - b.location).safe_normal()) < 0.0 {
            return false;
        }
        // EnemyHarmless: the player lying down, stumbling or stepping up
        if matches!(p.movement_state, Move::LayOnGround | Move::Stumble | Move::StepUp) {
            return false;
        }
        // PlayerLookingTowardsMeDot > 0.8, or nobody else in close combat
        let looking = self.pc.rotation.vector().dot((b.location - p.location).safe_normal()) > 0.8;
        let others = self.bots.iter().enumerate().any(|(j, o)| j != i && o.alive() && o.movement_state == BotMove::Melee);
        looking || !others
    }

    /// TdAIController.QueryDisarmState: from behind always works; from the front only while
    /// the bot's swing has been going for DisarmWindow.
    pub fn query_disarm_state(&self, i: usize) -> DisarmState {
        let b = &self.bots[i];
        if b.brain != Brain::PatrolCop || !b.alive() || b.weapon.is_none() {
            return DisarmState::NotPossible;
        }
        let dot = self.pawn.rotation.vector().dot(b.rotation.vector());
        if dot > 0.8 {
            return DisarmState::Stage1;
        }
        let in_melee = b.movement_state == BotMove::Melee;
        if (!in_melee && dot < 0.8) || b.melee_active_time < b.disarm_window {
            return DisarmState::Miss;
        }
        DisarmState::Stage1
    }

    /// TdAIController.StartCannedMove(MOVE_Snatched).
    pub fn bot_start_canned_disarm(&mut self, i: usize) -> bool {
        if !self.bots[i].alive() {
            return false;
        }
        self.bots[i].cop_state = CopState::Canned;
        if let Some(w) = self.bots[i].weapon.as_mut() {
            w.pressing = false;
            w.wants_to_fire = false;
        }
        self.bot_set_move(i, BotMove::Disarmed);
        true
    }

    /// TdMove_Disarmed.StartMove: the other custom anims stop.
    fn bot_disarmed_start(&mut self, i: usize) {
        let b = &mut self.bots[i];
        b.velocity = Vec3::ZERO;
        b.use_root_motion = false;
        b.use_root_rotation = false;
        for s in [Slot::FullBody, Slot::UpperBody, Slot::LowerBody, Slot::FullBodyDir, Slot::Weapon, Slot::Canned] {
            b.anim.stop(s, 0.1);
        }
    }

    /// TdMove_Disarmed.PlayDisarmStart: the snatch anim matching the player's.
    pub fn bot_play_disarm_start(&mut self, i: usize, anim: &str) {
        let b = &mut self.bots[i];
        // (held on its last frame: the ragdoll that follows starts from it)
        b.play(Slot::Canned, anim, 1.0, 0.1, -1.0, false, false);
        if b.current_anim.is_none() {
            let h = b.health;
            let loc = b.location;
            b.active_death_anim_type = 0;
            self.bot_take_damage(i, h, loc, Vec3::ZERO, DamageType::MeleeDisarm);
        }
    }

    /// TdMove_Disarmed.SetLookAtDirection (SetPreciseRotation over 0.1 s).
    /// TdMove_Disarmed.SetLookAtDirection: SetPreciseRotation(LookAtDirection, 0.1).
    pub fn bot_set_look_at_direction(&mut self, i: usize, rot: Rotator) {
        let b = &mut self.bots[i];
        b.look_at = Some((b.rotation.yaw, norm_axis(rot.yaw - b.rotation.yaw), 0.0));
    }

    /// TdBotPawn.TakeDamage from the player's gun: DamageMultiplier_Head (2) for the head,
    /// the weapon's DeathAnimType if it kills. Bots don't stumble from bullets
    /// (TdBotPawn.BulletDamage is empty).
    pub fn bot_take_bullet_damage(&mut self, i: usize, damage: f32, hit_location: Vec3, momentum: Vec3, class: &crate::weapons::WeaponClass) {
        let b = &self.bots[i];
        if !b.alive() {
            return;
        }
        // the head body: above the shoulders
        let head = hit_location.z > b.location.z + b.collision_height - 28.0;
        let damage = if head { damage * 2.0 } else { damage } as i32;
        // TdBotPawn.AdjustDamage: TdDmgType_Sniper_Bullet (the M95's InstantHitDamageTypes)
        // kills outright; otherwise TdPawn.AdjustDamage's bullet armor
        let damage = if class.sniper_bullet { 99999 } else { 0.max((damage as f32 * (1.0 - b.loadout.armor.0)) as i32) };
        let prev = b.active_death_anim_type;
        let p_loc = self.pawn.location;
        let p_rot = self.pawn.rotation;
        let b = &mut self.bots[i];
        b.active_death_anim_type = class.death_anim_type;
        b.health -= damage;
        b.stumble_hit = StumbleHit { instigator_location: p_loc, instigator_rotation: p_rot, damage_location: hit_location, momentum, damage_type: Some(DamageType::Bullet) };
        b.force_wait_for_damage = 0.4;
        if b.health <= 0 {
            self.bot_play_dying(i);
        } else {
            b.active_death_anim_type = prev;
        }
    }
}

impl Sim {
    /// Facing the focus: small differences turn the pawn at the controller's RotationRate; at
    /// 22.5 degrees or more (UTdMove_BotTurnStanding::CalculateLegTurnAngle, 0x11F8B90) a
    /// standing bot steps round with BotTurnStanding.
    fn bot_face(&mut self, i: usize, focus: Vec3, dt: f32) {
        let b = &self.bots[i];
        let want = Rotator::from_vector(focus - b.location).yaw;
        let delta = norm_axis(want - b.rotation.yaw);
        if b.movement_state == BotMove::Walking {
            // CalculateLegTurnAngle (0x11F8B90): DeltaRotationYaw = focus - Rotation,
            // DeltaLegRotationYaw = focus - LegRotation; bRotatePawn past 20 degrees, else
            // the legs overshoot by 30% (at most 3458); a turn at 22.5 degrees or more
            let leg = norm_axis(want - b.leg_yaw);
            if leg as f32 * 360.0 / 65536.0 >= 22.5 || (leg as f32 * 360.0 / 65536.0) <= -22.5 {
                let rotate_pawn = (delta as f32 * 360.0 / 65536.0).abs() > 20.0;
                let leg_delta = if rotate_pawn { leg } else { norm_axis(leg + ((leg as f32 * 0.3).clamp(-3458.8445, 3458.8445)) as i32) };
                let b = &mut self.bots[i];
                b.turn_initial_yaw = b.leg_yaw;
                b.turn_delta_yaw = leg_delta;
                b.turn_rotate_pawn = rotate_pawn;
                b.turn_initial_body_yaw = b.rotation.yaw;
                b.turn_delta_body_yaw = delta;
                self.bot_set_move(i, BotMove::TurnStanding);
                return;
            }
        }
        // the body follows the focus at the controller's RotationRate (not while the turn
        // carries it)
        let b = &mut self.bots[i];
        if b.movement_state == BotMove::TurnStanding && b.turn_rotate_pawn {
            return;
        }
        let step = (b.rotation_rate_yaw * dt) as i32;
        b.rotation.yaw = norm_axis(b.rotation.yaw + delta.clamp(-step, step));
    }

    /// TdMove_BotTurnStanding.StartAnimation: StandTurn45 under 67.5 degrees, 90 under 112.5,
    /// else 135 (blend in 0.05, out 0); the pawn turns over the anim's length.
    fn bot_turn_standing_start(&mut self, i: usize) {
        let b = &mut self.bots[i];
        // StartAnimation picks the anim by LegTurnAngle (overshoot included)
        let angle = b.turn_delta_yaw as f32 * 360.0 / 65536.0;
        // Unreal yaw grows clockwise seen from above: a positive delta turns right
        let side = if angle < 0.0 { "Left" } else { "Right" };
        let name = if angle.abs() < 67.5 {
            format!("StandTurn45{side}")
        } else if angle.abs() < 112.5 {
            format!("StandTurn90{side}")
        } else {
            format!("StandTurn135{side}")
        };
        b.use_root_motion = false;
        b.use_root_rotation = false;
        b.velocity = Vec3::ZERO;
        b.turn_time = 0.0;
        b.play(Slot::FullBody, &name, 1.0, 0.05, 0.0, false, false);
        match b.current_anim.as_ref().and_then(|(s, n)| b.anim.lib.get(n).map(|q| (*s, q.length))) {
            Some((_, len)) => b.turn_length = len,
            None => {
                b.leg_yaw = norm_axis(b.turn_initial_yaw + b.turn_delta_yaw);
                self.bot_set_move(i, BotMove::Walking);
            }
        }
    }
}
