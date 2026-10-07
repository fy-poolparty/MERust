//! The simulation: one player pawn, its controller, its moves and the world, ticked in the
//! order UE3 ticks them for a local player (controller -> pawn script/natives -> physics).

use crate::anim::{AnimLib, AnimPlayer};
use crate::collision::World;
use crate::config::Config;
use crate::controller::Controller;
use crate::math::{Rotator, Vec3};
use crate::moves::Moves;
use crate::pawn::{Move, MoveAction, Pawn, Physics};

/// Functions UnrealScript schedules with `SetTimer(Time, bLoop, 'Name')`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerFn {
    StopIgnoreMoveInput,
    StopIgnoreLookInput,
    /// `SetMoveTimer` / move `OnTimer` callbacks: (move, id).
    Move(Move, u8),
    /// TdPlayerController PlayerWalking: PlayStop / PlayStopLeft / PlayStopRight.
    PlayStop,
    PlayStopLeft,
    PlayStopRight,
    SetAnimationMovementState,
    ClearAnimationMovementState,
    PlayDeathAnim,
    DestroyPawn,
    TurnOffRootMotion,
    /// TdPlayerPawn.StopAgainstWall.
    StopAgainstWall,
}

#[derive(Clone, Copy, Debug)]
pub struct Timer {
    pub func: TimerFn,
    pub rate: f32,
    pub elapsed: f32,
    pub looping: bool,
}

/// Raw per-frame input (keyboard + mouse), like UE3's bound axes and buttons.
#[derive(Clone, Copy, Debug, Default)]
pub struct InputFrame {
    /// W/S: +1 / -1 (`aBaseY`)
    pub forward: f32,
    /// A/D: -1 / +1 (`aStrafe`)
    pub strafe: f32,
    /// Mouse counts this frame.
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub jump: bool,
    /// Left mouse (GBA_Fire: AttackPress / AttackRelease).
    pub attack: bool,
    /// Right mouse (GBA_SwitchWeapon: drop / disarm / pick up).
    pub switch_weapon: bool,
    pub crouch: bool,
    pub walk: bool,
    /// Q (LookBehind): pressed this frame.
    pub turn: bool,
}

/// Something the presentation layer may want to react to.
#[derive(Clone, Debug)]
pub enum Event {
    MoveChanged { from: Move, to: Move },
    Landed { fall_height: f32 },
    /// TdPlayerPawn.DestroyPawn after a death: the game respawns the player.
    Died,
    /// Something to play (or stop) for the pawn.
    Sound(crate::sound::SoundEvent),
    Footstep,
    /// A gun fired (muzzle flash / tracer).
    Shot(crate::weapons::Shot),
}

pub struct Sim {
    pub world: World,
    pub cfg: Config,
    pub time: f32,
    /// WorldInfo.DeltaSeconds of the current tick.
    pub delta_seconds: f32,
    pub pawn: Pawn,
    pub moves: Moves,
    pub pc: Controller,
    pub anim: AnimPlayer,
    pub timers: Vec<Timer>,
    pub events: Vec<Event>,
    /// Mesh->RootMotionDelta accumulated by the animation since the last physics step.
    pub root_motion_delta: Vec3,
    pub cfg_take_fall_damage: bool,
    pub health: i32,
    /// TdLadderVolumes in the level.
    pub ladders: Vec<crate::ladder::LadderVolume>,
    /// TdSwingVolumes and TdZiplineVolumes in the level.
    pub swings: Vec<crate::volumes::SwingVolume>,
    pub ziplines: Vec<crate::volumes::ZiplineVolume>,
    /// TdBalanceWalkVolumes.
    pub balances: Vec<crate::volumes::SplineVolume>,
    /// Enemy pawns.
    pub bots: Vec<crate::bots::Bot>,
    /// The bot meshes' animations (root motion and notifies).
    pub bot_lib: crate::anim::AnimLib,
    /// The melee hit detection bone's world location from the posed mesh (name, location),
    /// supplied by the presentation layer.
    pub hit_bone_world: Option<(String, Vec3)>,
    /// The player's gun, and guns lying around.
    pub weapon: Option<crate::weapons::Weapon>,
    pub pickups: Vec<crate::weapons::Pickup>,
    /// The anim libraries for unarmed / armed (UpdateAnimSets), and which one is in use.
    pub unarmed_lib: Option<AnimLib>,
    /// Per gun class: AS_C1P_Unarmed with the armed sets over it.
    pub armed_libs: std::collections::HashMap<&'static str, AnimLib>,
    pub anim_armed: bool,
    pub anim_weapon: Option<&'static str>,
    /// TdPawn.WeaponAnimState, BecameReadyTime, AmountTilUnarmed.
    pub weapon_anim_state: crate::weapons::WeaponAnimState,
    pub became_ready_time: f32,
    pub amount_til_unarmed: f32,
    /// TdPlayerPawn.LastEnemyHitTimeOut (the crosshair turns orange until then).
    pub last_enemy_hit_time_out: f32,
    /// Cheat: the player takes no damage.
    pub god_mode: bool,
    /// TdSkelControlRecoil "RightHandRecoil": EffectorLocation.X and RecoverDelay.
    pub recoil_x: f32,
    pub recoil_delay: f32,
    /// The relevant TdAnimNodeTurn's state (it turns LegRotation).
    pub turn_node: crate::body::TurnNode,
    /// [TdGame.TdPlayerPawn] edge probing (CheckForLedges).
    pub edge: crate::moves::vertigo::EdgeConfig,
}

impl Sim {
    pub fn new(world: World, cfg: Config, lib: AnimLib) -> Self {
        let pawn = Pawn::new(&cfg);
        let moves = Moves::new(&cfg);
        let pc = Controller::new(&cfg);
        let take_fall_damage = cfg.bool(&["TdPawn"], "bTakeFallDamage", true);
        let edge = crate::moves::vertigo::EdgeConfig::new(&cfg);
        Sim {
            world,
            cfg,
            time: 0.0,
            delta_seconds: 0.0,
            pawn,
            moves,
            pc,
            anim: AnimPlayer::new(lib),
            timers: Vec::new(),
            events: Vec::new(),
            root_motion_delta: Vec3::ZERO,
            cfg_take_fall_damage: take_fall_damage,
            health: 100,
            ladders: Vec::new(),
            swings: Vec::new(),
            ziplines: Vec::new(),
            balances: Vec::new(),
            bots: Vec::new(),
            bot_lib: crate::anim::AnimLib::default(),
            hit_bone_world: None,
            weapon: None,
            pickups: Vec::new(),
            unarmed_lib: None,
            armed_libs: Default::default(),
            anim_armed: false,
            anim_weapon: None,
            weapon_anim_state: Default::default(),
            became_ready_time: 0.0,
            amount_til_unarmed: 0.0,
            last_enemy_hit_time_out: 0.0,
            god_mode: false,
            recoil_x: -2.435613,
            recoil_delay: 0.0,
            turn_node: Default::default(),
            edge,
        }
    }

    /// Place the pawn standing at `feet` (bottom of the cylinder), facing `yaw` (UE units).
    pub fn spawn(&mut self, feet: Vec3, yaw: i32) {
        let h = self.pawn.default_collision_height;
        self.pawn.location = feet + Vec3::new(0.0, 0.0, h + 2.15);
        self.pawn.rotation = Rotator::new(0, yaw, 0);
        self.pawn.velocity = Vec3::ZERO;
        self.pawn.acceleration = Vec3::ZERO;
        self.pc.rotation = Rotator::new(0, yaw, 0);
        self.timers.clear();
        self.anim.stop_all();
        // a fresh pawn: alive, out of UncontrolledFall / Dying, input back
        self.health = 100;
        self.pawn.dying = false;
        self.pawn.uncontrolled_fall = false;
        self.stop_ignore_move_input();
        self.stop_ignore_look_input();
        self.pawn.animation_movement_state = Move::None;
        self.pawn.pending_animation_movement_state = Move::None;
        self.pawn.active_movement_volume = None;
        self.pawn.ladder_physics_volume = None;
        self.pawn.leg_rotation = yaw;
        self.pawn.going_forward = true;
        self.pawn.mesh_offset_xy = Vec3::ZERO;
        self.pawn.against_wall_state = crate::body::AgainstWall::None;
        self.turn_node = Default::default();
        for slot in [crate::sound::LoopSlot::Slide, crate::sound::LoopSlot::ClimbDownFast, crate::sound::LoopSlot::Falling] {
            self.sound(crate::sound::SoundEvent::LoopStop { slot, fade_out: 0.0 });
        }
        self.use_root_motion(false);
        self.use_root_rotation(false);
        if self.pawn.movement_state != Move::Walking {
            self.set_move(Move::Walking, false, false);
        }
        self.pc.state = crate::controller::CtrlState::PlayerWalking;
        self.set_physics(Physics::Falling);
        self.pawn.enter_falling_height = self.pawn.location.z;
    }

    /// One frame. UE3 order for a local player: PlayerController.PlayerTick (input, PlayerMove,
    /// rotation), then the pawn (TdPawn native Tick, script Tick, timers, TickSpecial, physics),
    /// then animation.
    pub fn tick(&mut self, dt: f32, input: InputFrame) {
        self.time += dt;
        self.delta_seconds = dt;
        self.player_tick(dt, input);
        self.pawn_tick(dt);
    }

    fn pawn_tick(&mut self, dt: f32) {
        // ATdPawn::Tick (vt110): mesh translation decay first (0x12BA2E0)
        self.update_mesh_translation(dt);
        if self.pawn.evade_timer > 0.0 {
            self.pawn.evade_timer -= self.pawn.evade_timer.min(dt);
        }
        if self.pawn.illegal_ledge_timer > 0.0 {
            self.pawn.illegal_ledge_timer -= self.pawn.illegal_ledge_timer.min(dt);
        }
        let ms = self.pawn.movement_state;
        if ms != Move::None {
            self.move_tick(ms, dt);
        }
        self.update_leg_rotation(dt);
        self.update_average_speed(dt);
        // TdPlayerPawn.Tick / TdPawn.Tick (script)
        self.tick_against_wall();
        self.player_pawn_script_tick(dt);
        // TdPawn.Tick: RegenerateHealth, UpdateWeaponAnimState
        self.regenerate_health(dt);
        self.update_weapon_anim_state(dt);
        // ATdPlayerPawn::Tick: GravityModifierTimer
        if self.pawn.gravity_modifier_timer > 0.0 {
            self.pawn.gravity_modifier_timer -= dt;
            if self.pawn.gravity_modifier_timer < 0.0 {
                self.pawn.gravity_modifier = 1.0;
            }
        }
        self.update_velocity_variables();
        self.update_walking_state();
        // TdPawn.Tick: ActiveMovementVolume.PawnUpdate
        self.update_movement_volumes();
        match self.pawn.active_movement_volume {
            Some(crate::volumes::VolumeRef::Ladder(i)) => self.ladder_pawn_update(i),
            Some(crate::volumes::VolumeRef::Swing(i)) => self.swing_volume_pawn_update(i),
            Some(crate::volumes::VolumeRef::Zipline(i)) => self.zipline_volume_pawn_update(i),
            Some(crate::volumes::VolumeRef::Balance(i)) => self.balance_volume_pawn_update(i),
            None => {}
        }
        // AActor::UpdateTimers
        self.update_timers(dt);
        // ATdPawn::TickSpecial (vt121)
        let ms = self.pawn.movement_state;
        if ms != Move::None {
            self.move_pre_physics(ms);
            self.move_precise_physics(ms, dt);
        }
        // Mesh: step smoothing decays towards the target, mesh XY offset fades.
        self.update_mesh_offsets(dt);
        // Animation advances before physics so root motion is available (UE3 ticks the skeletal
        // mesh component before the actor's physics for root motion actors).
        let rm = self.anim.tick(dt, &self.pawn);
        self.root_motion_delta += rm;
        self.tick_turn_node(dt);
        // Mesh.RootMotionRotationMode = RMRM_RotateActor: the actor turns by the root yaw delta
        // and ATdPawn vt138 (0x12B25A0) carries LegRotation and the controller along.
        let dyaw = self.anim.root_rotation_delta;
        if self.pawn.is_using_root_rotation && dyaw != 0 {
            self.pawn.rotation.yaw += dyaw;
            self.pawn.leg_rotation += dyaw;
            self.pc.rotation.yaw += dyaw;
        }
        self.dispatch_anim_events();
        self.perform_physics(dt);
        // the enemies (their controllers and pawns tick after the player's)
        self.tick_bots(dt);
        self.tick_pickups(dt);
    }

    fn update_mesh_offsets(&mut self, dt: f32) {
        let p = &mut self.pawn;
        p.swing_control.tick(dt);
        // USkelControlBase::TickSkelControl strength blend (RootControl)
        if p.root_offset_blend_time_to_go != 0.0 {
            if p.root_offset_blend_time_to_go < dt {
                p.root_offset_strength = p.root_offset_strength_target;
                p.root_offset_blend_time_to_go = 0.0;
            } else {
                p.root_offset_strength += (p.root_offset_strength_target - p.root_offset_strength) * (dt / p.root_offset_blend_time_to_go);
                p.root_offset_blend_time_to_go -= dt;
            }
        }
    }

    fn update_timers(&mut self, dt: f32) {
        let mut fired = Vec::new();
        let mut i = 0;
        while i < self.timers.len() {
            let t = &mut self.timers[i];
            t.elapsed += dt;
            if t.elapsed >= t.rate {
                fired.push(t.func);
                if t.looping {
                    t.elapsed -= t.rate;
                } else {
                    self.timers.remove(i);
                    continue;
                }
            }
            i += 1;
        }
        for f in fired {
            self.fire_timer(f);
        }
    }

    /// `SetTimer(Time, bLoop, Func)`: replaces an existing timer of the same function.
    pub fn set_timer(&mut self, func: TimerFn, time: f32, looping: bool) {
        self.timers.retain(|t| t.func != func);
        if time > 0.0 {
            self.timers.push(Timer { func, rate: time, elapsed: 0.0, looping });
        }
    }

    pub fn clear_timer(&mut self, func: TimerFn) {
        self.timers.retain(|t| t.func != func);
    }

    pub fn is_timer_active(&self, func: TimerFn) -> bool {
        self.timers.iter().any(|t| t.func == func)
    }

    fn fire_timer(&mut self, f: TimerFn) {
        match f {
            TimerFn::StopIgnoreMoveInput => self.stop_ignore_move_input(),
            TimerFn::StopIgnoreLookInput => self.stop_ignore_look_input(),
            TimerFn::Move(m, id) => self.move_on_move_timer(m, id),
            TimerFn::PlayStop => self.pc.is_stopping = false,
            TimerFn::PlayStopLeft => self.play_stop_anim(true),
            TimerFn::PlayStopRight => self.play_stop_anim(false),
            TimerFn::SetAnimationMovementState => self.pawn.animation_movement_state = self.pawn.pending_animation_movement_state,
            TimerFn::PlayDeathAnim => self.play_death_anim(),
            TimerFn::DestroyPawn => self.events.push(Event::Died),
            TimerFn::TurnOffRootMotion => self.pawn.velocity = Vec3::ZERO,
            TimerFn::StopAgainstWall => self.stop_against_wall(),
            TimerFn::ClearAnimationMovementState => {
                self.pawn.animation_movement_state = Move::None;
                self.pawn.pending_animation_movement_state = Move::None;
            }
        }
    }

    // ------------------------------------------------------------------ TdPawn script API

    /// ATdPawn::SetRootOffset (0x12B2430): a non-zero offset becomes RootControl's translation
    /// and the control blends in; a zero offset only blends the control out.
    pub fn set_root_offset(&mut self, offset: Vec3, blend_time: f32) {
        self.set_root_offset_space(offset, blend_time, crate::pawn::BoneControlSpace::World);
    }

    /// SetRootOffset with its TranslationSpace (omitted: BCS_WorldSpace).
    pub fn set_root_offset_space(&mut self, offset: Vec3, blend_time: f32, space: crate::pawn::BoneControlSpace) {
        let target = if offset.length() <= 0.1 {
            0.0
        } else {
            self.pawn.root_offset = offset;
            self.pawn.root_offset_space = space;
            1.0
        };
        // USkelControlBase::SetSkelControlStrength (0xD10650)
        let p = &mut self.pawn;
        let time = blend_time.max(0.0);
        if p.root_offset_strength_target != target || p.root_offset_blend_time_to_go > time {
            p.root_offset_strength_target = target;
            p.root_offset_blend_time_to_go = time;
            if time <= 0.0 {
                p.root_offset_strength = target;
                p.root_offset_blend_time_to_go = 0.0;
            }
        }
    }

    /// TdPawn.SetAnimationMovementState.
    pub fn set_animation_movement_state(&mut self, state: Move, delay: f32) {
        self.pawn.pending_animation_movement_state = state;
        if delay > 0.0 {
            self.set_timer(TimerFn::SetAnimationMovementState, delay, false);
        } else {
            self.pawn.animation_movement_state = state;
        }
    }

    /// TdPawn.ClearAnimationMovementState.
    pub fn clear_animation_movement_state(&mut self, delay: f32) {
        if delay > 0.0 {
            self.set_timer(TimerFn::ClearAnimationMovementState, delay, false);
        } else {
            self.pawn.animation_movement_state = Move::None;
            self.pawn.pending_animation_movement_state = Move::None;
        }
    }

    /// TdPawn.SetMove.
    pub fn set_move(&mut self, new: Move, _via_replication: bool, check_can_do: bool) -> bool {
        // TdPlayerPawn state UncontrolledFall.SetMove
        if self.pawn.uncontrolled_fall && !matches!(new, Move::Landing | Move::SoftLanding) {
            return false;
        }
        if new == self.pawn.movement_state || !self.pawn.allow_move_change {
            return false;
        }
        self.pawn.pending_movement_state = new;
        if check_can_do && !self.can_do_move(new) {
            return false;
        }
        let old = self.pawn.movement_state;
        if old != Move::None {
            self.stop_move(old);
        }
        self.pawn.old_movement_state = old;
        self.pawn.movement_state = new;
        if old != Move::None {
            self.post_stop_move(old);
        }
        self.start_move(new);
        // NotifyNewMove
        if matches!(self.pawn.old_movement_state, Move::Slide | Move::MeleeSlide) {
            self.pawn.slide_stopped_time_stamp = self.time;
        }
        self.events.push(Event::MoveChanged { from: old, to: new });
        true
    }

    /// TdPawn.CanDoMove (event): gated by bAllowMoveChange and a pending change.
    pub fn pawn_can_do_move(&mut self, m: Move) -> bool {
        if !self.pawn.allow_move_change || self.pawn.pending_movement_state != self.pawn.movement_state {
            return false;
        }
        self.can_do_move(m)
    }

    pub fn is_in_move(&self, m: Move) -> bool {
        self.pawn.movement_state == m
    }

    /// TdPawn.HandleMoveAction -> TdPlayerMoveManager.HandleMoveAction.
    pub fn handle_move_action(&mut self, action: MoveAction) {
        self.move_manager_handle_action(action);
    }

    /// TdPawn.SetIgnoreMoveInput.
    pub fn set_ignore_move_input(&mut self, time: f32) {
        if time == 0.0 {
            return;
        }
        self.clear_timer(TimerFn::StopIgnoreMoveInput);
        if time > 0.0 && !self.pc.ignore_move_input {
            self.set_timer(TimerFn::StopIgnoreMoveInput, time, false);
        }
        self.pc.ignore_move_input = true;
    }

    pub fn stop_ignore_move_input(&mut self) {
        self.pc.ignore_move_input = false;
        self.clear_timer(TimerFn::StopIgnoreMoveInput);
    }

    /// TdPawn.SetIgnoreLookInput.
    pub fn set_ignore_look_input(&mut self, time: f32) {
        if time == 0.0 {
            return;
        }
        self.clear_timer(TimerFn::StopIgnoreLookInput);
        if time > 0.0 && !self.pc.ignore_look_input {
            self.set_timer(TimerFn::StopIgnoreLookInput, time, false);
        }
        self.pc.ignore_look_input = true;
    }

    pub fn stop_ignore_look_input(&mut self) {
        self.pc.ignore_look_input = false;
        self.clear_timer(TimerFn::StopIgnoreLookInput);
    }

    /// TdPawn.UseRootMotion / UseRootRotation.
    pub fn use_root_motion(&mut self, on: bool) {
        self.pawn.is_using_root_motion = on;
        if !on {
            self.root_motion_delta = Vec3::ZERO;
        }
    }

    pub fn use_root_rotation(&mut self, on: bool) {
        self.pawn.is_using_root_rotation = on;
    }

    /// TdPawn::GetMobilityMultiplier (vt264, TdPlayerPawn override without weapons): 1.
    pub fn mobility_multiplier(&self) -> f32 {
        1.0
    }

    /// TdMove vt72: the move's SpeedModifier.
    pub fn move_speed_modifier(&self, m: Move) -> f32 {
        self.moves.base(m).speed_modifier
    }

    /// ATdPawn::UpdateVelocityVariables (vt276).
    fn update_velocity_variables(&mut self) {
        let p = &mut self.pawn;
        p.velocity_magnitude_2d = crate::math::vsize2d(p.velocity);
        p.velocity_magnitude = p.velocity.length();
        p.velocity_dir_2d = crate::math::UeVec::safe_normal_2d(p.velocity);
        p.velocity_dir = crate::math::UeVec::safe_normal(p.velocity);
    }

    /// ATdPawn::UpdateWalkingState (vt273).
    fn update_walking_state(&mut self) {
        use crate::pawn::WalkingState as W;
        let p = &mut self.pawn;
        if p.override_walking_state != W::None {
            p.current_walking_state = p.override_walking_state;
            return;
        }
        let v = p.velocity_magnitude_2d;
        p.current_walking_state = if v < p.sneak_velocity {
            W::Idle
        } else if v < p.walk_velocity {
            W::Sneak
        } else if v < p.jog_velocity {
            W::Walk
        } else if v < p.run_velocity {
            W::Jog
        } else if v < p.sprint_velocity {
            W::Run
        } else {
            W::Sprint
        };
    }

    /// TdPlayerPawn.TakeFallingDamage. Health only matters for deaths here: RegenerateHealth
    /// isn't ported, so the 15 of a hard landing is not kept.
    pub fn take_falling_damage(&mut self) {
        let h = self.pawn.enter_falling_height - self.pawn.location.z;
        self.events.push(Event::Landed { fall_height: h });
        let soft = self.landing_is_landing_on_soft_object();
        let death = (self.pawn.movement_state == Move::FallingUncontrolled && self.pawn.uncontrolled_fall) || h >= self.pawn.falling_uncontrolled_height;
        if soft || (!death && h < self.moves.landing.hard_landing_height) || self.can_skill_roll() {
            return;
        }
        if death {
            self.take_damage(100);
        }
    }

    /// Pawn.TakeDamage -> Died for the player.
    pub fn take_damage(&mut self, damage: i32) {
        if self.pawn.dying || self.god_mode {
            return;
        }
        self.health -= damage;
        if self.health <= 0 {
            self.play_dying();
        }
    }

    /// TdPlayerPawn.PlayDying, then state Dying.BeginState.
    fn play_dying(&mut self) {
        self.set_ignore_move_input(-1.0);
        self.set_ignore_look_input(-1.0);
        // TdPawn.PlayDying -> TossInventory: the gun drops
        self.toss_weapon_on_death();
        let fast = matches!(self.pawn.movement_state, Move::FallingUncontrolled | Move::Turn180InAir);
        self.set_timer(TimerFn::PlayDeathAnim, if fast { 0.01 } else { 0.5 }, false);
        self.pc.state = crate::controller::CtrlState::PlayerDying;
        let from_uncontrolled = self.pawn.uncontrolled_fall;
        if from_uncontrolled {
            // UncontrolledFall.EndState
            self.sound(crate::sound::SoundEvent::LoopStop { slot: crate::sound::LoopSlot::Falling, fade_out: 0.1 });
        }
        self.pawn.uncontrolled_fall = false;
        self.pawn.dying = true;
        self.set_timer(TimerFn::DestroyPawn, if from_uncontrolled { 0.4 } else { 2.5 }, false);
    }

    /// TdPlayerPawn.PlayDeathAnim.
    fn play_death_anim(&mut self) {
        let death_move = self.pawn.movement_state;
        if self.pawn.movement_state != Move::LayOnGround {
            self.set_move(Move::Walking, false, false);
        }
        let m = self.pawn.movement_state;
        self.reset_camera_look(m, 0.2);
        let mut root_motion = false;
        match death_move {
            Move::Turn180InAir => {
                self.anim.play(crate::pawn::Slot::Canned, "FallingLandDieBwd", 1.0, 0.3, -1.0, false, true, false);
                self.set_physics(Physics::Falling);
            }
            Move::FallingUncontrolled => {
                self.anim.play(crate::pawn::Slot::Canned, "FallingLandDie", 1.0, 0.3, -1.0, false, true, false);
                self.set_physics(Physics::Falling);
            }
            Move::Crouch | Move::Slide => {
                self.anim.play(crate::pawn::Slot::Canned, "diecrouch", 1.0, 0.3, -1.0, false, true, true);
                root_motion = true;
            }
            Move::LayOnGround => {
                self.anim.play(crate::pawn::Slot::Canned, "DiePursuitFinish", 1.0, 0.3, -1.0, false, true, false);
                self.set_physics(Physics::Flying);
                root_motion = true;
            }
            Move::Falling => self.set_animation_movement_state(Move::FallingUncontrolled, -1.0),
            _ => {
                self.anim.play(crate::pawn::Slot::Canned, "die", 1.0, 0.3, -1.0, false, true, true);
                root_motion = true;
            }
        }
        if root_motion {
            self.pawn.velocity = Vec3::ZERO;
            self.set_physics(Physics::Flying);
            self.use_root_motion(true);
            self.set_timer(TimerFn::TurnOffRootMotion, 2.0, false);
        }
    }

    /// UTdMove_Landing::IsLandingOnSoftObject (0x11F9FC0): a soft-landing surface right under
    /// the feet; remembers it for the landing move.
    pub fn landing_is_landing_on_soft_object(&mut self) -> bool {
        let start = self.pawn.location;
        let mut end = start;
        end.z = start.z - self.pawn.collision_height - 20.0;
        let mut ext = self.pawn.extent();
        ext.z = 5.0;
        let h = self.world.line_check(end, start, ext);
        if !h.hit || h.normal.z <= 0.9 || !h.surface.soft_landing {
            return false;
        }
        self.moves.landing.last_landing_was_on_soft_object = true;
        true
    }

    /// ATdPawn::Tick average-speed sampling.
    fn update_average_speed(&mut self, dt: f32) {
        let p = &mut self.pawn;
        if p.as_time_data.is_empty() {
            // ATdPawn::PostInitCurves
            p.as_poll_interval = p.as_filter_time / p.as_poll_slots as f32;
            p.as_time_data = vec![p.as_poll_interval; p.as_poll_slots];
            p.as_distance_data = vec![0.0; p.as_poll_slots];
        }
        p.as_poll_timer += dt;
        // running backwards (bGoingForward off) counts as no distance
        let sign = if p.going_forward { 1.0 } else { -1.0 };
        let d = (p.velocity.x * p.velocity.x + p.velocity.y * p.velocity.y).sqrt().abs() * sign * dt;
        p.as_distance_accum += d.max(0.0);
        if p.as_poll_timer > p.as_poll_interval {
            let i = p.as_slot_pointer;
            p.as_distance_data[i] = p.as_distance_accum;
            p.as_time_data[i] = p.as_poll_timer;
            p.as_slot_pointer += 1;
            p.as_distance_accum = 0.0;
            p.as_poll_timer = 0.0;
            if p.as_slot_pointer == p.as_poll_slots {
                p.as_slot_pointer = 0;
            }
            let dist: f32 = p.as_distance_data.iter().sum();
            let time: f32 = p.as_time_data.iter().sum();
            p.average_speed = dist / time;
        }
    }

    /// ATdPawn::GetAverageSpeed (0x12BA620): mean speed over the last `time` seconds of slots,
    /// starting at ASSlotPointer and walking backwards.
    pub fn get_average_speed(&self, time: f32) -> f32 {
        let p = &self.pawn;
        if p.as_time_data.is_empty() {
            return 0.0;
        }
        let t = if time >= p.as_filter_time { p.as_filter_time } else { time };
        let n = (p.as_poll_slots as f32 * t / p.as_filter_time) as i32;
        if n <= 0 {
            return 0.0;
        }
        let (mut tsum, mut dsum) = (0.0f32, 0.0f32);
        for k in 0..n {
            let mut idx = p.as_slot_pointer as i32 - k;
            if idx < 0 {
                idx += p.as_poll_slots as i32;
            }
            tsum += p.as_time_data[idx as usize];
            dsum += p.as_distance_data[idx as usize];
        }
        if dsum == 0.0 { 0.0 } else { dsum / tsum }
    }

    /// TdPawn.IsLeftLegForward: MasterSync group 0 past half its sequence.
    pub fn is_left_leg_forward(&self) -> bool {
        self.anim.locomotion_phase > 0.5
    }

    /// TdPawn.CanSkillRoll.
    /// TdPawn.CanSkillRoll (no weapons/melee in this port).
    pub fn can_skill_roll(&self) -> bool {
        if self.heavy_weapon() || matches!(self.pawn.movement_state, Move::Turn180InAir | Move::FallingUncontrolled | Move::MeleeAir) {
            return false;
        }
        // Landing out of a melee move
        if self.pawn.movement_state == Move::Landing && crate::moves::melee::is_melee_move(self.pawn.old_movement_state) {
            return false;
        }
        self.pawn.roll_trigger_time + 0.2 > self.time
    }
}
