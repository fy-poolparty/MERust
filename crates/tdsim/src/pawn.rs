//! TdPawn / TdPlayerPawn state (the subset of Actor, Pawn, TdPawn and TdPlayerPawn properties the
//! player's movement reads or writes), with the defaults from their `defaultproperties` and config.

use crate::config::Config;
use crate::math::{InterpCurve, Rotator, Vec3};

/// `Actor.EPhysics` (Mirror's Edge adds WallRunning and WallClimbing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Physics {
    #[default]
    None = 0,
    Walking = 1,
    Falling = 2,
    Swimming = 3,
    Flying = 4,
    Rotating = 5,
    Projectile = 6,
    Interpolating = 7,
    Spider = 8,
    Ladder = 9,
    RigidBody = 10,
    SoftBody = 11,
    WallRunning = 12,
    WallClimbing = 13,
}

macro_rules! movement_enum {
    ($($name:ident = $v:expr),* $(,)?) => {
        /// `TdPawn.EMovement`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
        #[repr(u8)]
        pub enum Move { #[default] $($name = $v),* }
        impl Move {
            pub const COUNT: usize = 94;
            pub fn from_u8(v: u8) -> Move {
                match v { $($v => Move::$name,)* _ => Move::None }
            }
        }
    };
}

movement_enum! {
    None = 0, Walking = 1, Falling = 2, Grabbing = 3, WallRunningRight = 4, WallRunningLeft = 5,
    WallClimbing = 6, SpringBoarding = 7, SpeedVaulting = 8, VaultOver = 9, GrabPullUp = 10,
    Jump = 11, WallRunJump = 12, GrabJump = 13, IntoGrab = 14, Crouch = 15, Slide = 16, Melee = 17,
    Snatch = 18, Barge = 19, Landing = 20, Climb = 21, IntoClimb = 22, WallKick = 23, Turn180 = 24,
    Turn180InAir = 25, LayOnGround = 26, IntoZipLine = 27, ZipLine = 28, Balance = 29,
    LedgeWalk = 30, GrabTransfer = 31, MeleeAir = 32, DodgeJump = 33, WallRunDodgeJump = 34,
    Stumble = 35, Snatched = 36, StepUp = 37, RumpSlide = 38, Interact = 39, WallRun = 40,
    BotStop = 41, BotStartWalking = 42, BotStartRunning = 43, BotTurnRunning = 44,
    BotTurnStanding = 45, ExitCover = 46, Vertigo = 47, MeleeSlide = 48, WallClimbDodgeJump = 49,
    WallClimb180TurnJump = 50, WallClimbDodgeJumpLeft = 51, WallClimbDodgeJumpRight = 52,
    MeleeVault = 53, BotMeleeSecondSwing = 54, StumbleHard = 55, BotRoll = 56, BotFlip = 57,
    BackflipObsolete = 58, BackflipToRunObsolete = 59, Swing = 60, Coil = 61, MeleeWallrun = 62,
    MeleeCrouch = 63, BotJumpShort = 64, BotJumpMedium = 65, BotJumpLong = 66, JumpIntoGrab = 67,
    StandGrabHeaveBot = 68, BotMeleeDodge = 69, FinishAttack = 70, MeleeBarge = 71,
    FallingUncontrolled = 72, SwingJump = 73, AnimationPlayback = 74, EnterCover = 75, Cover = 76,
    StumbleFalling = 77, SoftLanding = 78, HeadButtedByCeleste = 79, MeleeOriginalCelesteObsolete = 80,
    AutoStepUp = 81, MeleeAirAbove = 82, MeleeCounterAttackObsolete = 83, Block = 84, AirBarge = 85,
    RbBullrushObsolete = 86, RbBullrushEndObsolete = 87, RbHitWallObsolete = 88,
    RbHitFenceObsolete = 89, RbLedgeObsolete = 90, SkillRoll = 91, BotGetDistance = 92, Cutscene = 93,
}

/// `TdPawn.EMovementAction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MoveAction {
    None = 0,
    Jump = 1,
    StopJump = 2,
    Melee = 3,
    Snatch = 4,
    Crouch = 5,
    StopCrouch = 6,
    ClimbUp = 7,
    ClimbDown = 8,
    ClimbUpLong = 9,
    ClimbDownLong = 10,
    Abort = 11,
    ShimmyLeft = 12,
    ShimmyLeftLong = 13,
    ShimmyRight = 14,
    ShimmyRightLong = 15,
    Turn = 16,
    Stumble = 17,
    StumbleHard = 18,
}

/// `TdPawn.EMoveActionHint`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum MoveActionHint {
    #[default]
    None = 0,
    Left = 1,
    Right = 2,
    Up = 3,
    Down = 4,
}

/// SkelControlBase.EBoneControlSpace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BoneControlSpace {
    #[default]
    World = 0,
    Actor = 1,
    Component = 2,
    ParentBone = 3,
    Bone = 4,
    OtherBone = 5,
}

/// A skeletal control the sim drives: its strength blend (USkelControlBase) and, for
/// SwingControl, the bone rotation roll and the translation it applies.
#[derive(Clone, Copy, Debug, Default)]
pub struct SkelControl {
    pub strength: f32,
    pub target: f32,
    pub time_to_go: f32,
    pub roll: i32,
    pub translation: Vec3,
}

impl SkelControl {
    /// USkelControlBase::SetSkelControlStrength (0xD10650).
    pub fn set_strength(&mut self, target: f32, time: f32) {
        let (target, time) = (target.clamp(0.0, 1.0), time.max(0.0));
        if self.target != target || self.time_to_go > time {
            self.target = target;
            self.time_to_go = time;
            if time <= 0.0 {
                self.strength = target;
                self.time_to_go = 0.0;
            }
        }
    }

    /// USkelControlBase::TickSkelControl's blend (0xD132A0).
    pub fn tick(&mut self, dt: f32) {
        if self.time_to_go != 0.0 || self.strength != self.target {
            if self.time_to_go <= dt || self.time_to_go == 0.0 {
                self.strength = self.target;
                self.time_to_go = 0.0;
            } else {
                self.strength += (self.target - self.strength) / self.time_to_go * dt;
                self.time_to_go -= dt;
            }
        }
    }
}

/// Scene.ESceneDepthPriorityGroup for the first-person meshes: Foreground draws over the
/// world, Intermediate is depth-tested against it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Dpg {
    World = 1,
    Intermediate = 2,
    #[default]
    Foreground = 3,
}

/// `TdPawn.WalkingState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(u8)]
pub enum WalkingState {
    #[default]
    Idle = 0,
    Sneak = 1,
    Walk = 2,
    Jog = 3,
    Run = 4,
    Sprint = 5,
    None = 6,
}

/// `TdPawn.CustomNodeType`: the AnimTree slots moves play custom animations into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Slot {
    Canned = 0,
    CannedUpperBody = 1,
    FullBody = 2,
    FullBodyDir = 3,
    UpperBody = 4,
    LowerBody = 5,
    Camera = 6,
    Weapon = 7,
    Face = 8,
}

/// The pawn's state. Field names follow the UnrealScript properties.
#[derive(Clone, Debug)]
pub struct Pawn {
    // ---- Actor
    pub location: Vec3,
    pub rotation: Rotator,
    pub velocity: Vec3,
    pub acceleration: Vec3,
    pub physics: Physics,
    /// `Base != None` (standing on world geometry).
    pub base: bool,
    pub collide_world: bool,
    pub just_teleported: bool,
    /// CylinderComponent.CollisionRadius / CollisionHeight (current, may be shrunk by moves).
    pub collision_radius: f32,
    pub collision_height: f32,
    pub default_collision_radius: f32,
    pub default_collision_height: f32,

    // ---- Pawn
    pub max_step_height: f32,
    pub max_jump_height: f32,
    pub walkable_floor_z: f32,
    pub ground_speed: f32,
    pub air_speed: f32,
    pub accel_rate: f32,
    pub jump_z: f32,
    pub air_control: f32,
    pub base_eye_height: f32,
    pub floor: Vec3,
    pub rm_velocity: Vec3,
    pub can_jump: bool,
    pub can_walk_off_ledges: bool,
    pub is_walking: bool,
    pub is_crouched: bool,
    pub avoid_ledges: bool,
    pub force_floor_check: bool,
    pub force_max_accel: bool,
    pub force_rm_velocity: bool,
    pub force_regular_velocity: bool,
    /// PhysicsVolume.GroundFriction / FluidFriction / TerminalVelocity / ZoneVelocity.
    pub ground_friction: f32,
    pub fluid_friction: f32,
    pub terminal_velocity: f32,
    /// WorldInfo.DefaultGravityZ.
    pub world_gravity_z: f32,

    // ---- TdPawn
    pub movement_state: Move,
    pub old_movement_state: Move,
    pub pending_movement_state: Move,
    pub allow_move_change: bool,
    pub gravity_modifier: f32,
    /// TdPawn.GravityModifierTimer: GravityModifier goes back to 1 when it runs out.
    pub gravity_modifier_timer: f32,
    /// TdPawn health regeneration (RegenerateHealth).
    pub max_health: i32,
    pub regenerate_delay: f32,
    pub regenerate_health_per_second: f32,
    pub time_since_last_damage: f32,
    pub health_frac: f32,
    /// TdPawn.CustomSoundInput (TdSoundNodeVelocity's Custom speed).
    pub custom_sound_input: f32,
    /// SwingControl1p / SwingControl3p on the root bone (TdMove_Swing.SetPawnRotation).
    pub swing_control: SkelControl,
    pub enter_falling_height: f32,
    pub max_wall_step_height: f32,
    pub new_floor_smooth: f32,
    pub target_mesh_translation_z: f32,
    /// Mesh1p.Translation.Z (step smoothing), relative to the default.
    pub mesh_translation_z: f32,
    /// Mesh1p/Mesh3p.Translation X/Y (actor space): OffsetMeshXY, decaying in ATdPawn::Tick.
    pub mesh_offset_xy: Vec3,
    pub is_wall_walking: bool,
    pub force_max_accel_one_frame: bool,
    pub uncontrolled_slide: bool,
    pub uncontrolled_slide_normal: Vec3,
    pub is_using_root_motion: bool,
    pub is_using_root_rotation: bool,
    pub can_uncrouch: bool,
    pub found_ledge: bool,
    pub found_ledge_excludes_hand_moves: bool,
    pub found_ledge_excludes_foot_moves: bool,
    pub move_ledge_location: Vec3,
    pub move_ledge_normal: Vec3,
    pub move_normal: Vec3,
    pub move_location: Vec3,
    pub move_ledge_result: i32,
    pub illegal_ledge_normal: Vec3,
    pub illegal_ledge_timer: f32,
    pub ledge_find_extent: Vec3,
    pub ledge_find_distance: f32,
    pub ledge_find_depth: f32,
    pub evade_timer: f32,
    /// Average-speed ring buffer (ASFilterTime / ASPollSlots, filled in ATdPawn::Tick).
    pub as_filter_time: f32,
    pub as_poll_slots: usize,
    pub as_poll_interval: f32,
    pub as_poll_timer: f32,
    pub as_distance_accum: f32,
    pub as_slot_pointer: usize,
    pub as_time_data: Vec<f32>,
    pub as_distance_data: Vec<f32>,
    pub current_grab_turn_type: crate::moves::grab::GrabTurn,
    pub move_action_hint: MoveActionHint,
    pub move_action_max: bool,
    pub override_walking_state: WalkingState,
    pub current_walking_state: WalkingState,
    pub velocity_magnitude_2d: f32,
    pub velocity_magnitude: f32,
    pub velocity_dir_2d: Vec3,
    pub velocity_dir: Vec3,
    pub face_rotation_time_left: f32,
    pub slide_stopped_time_stamp: f32,
    pub roll_trigger_time: f32,
    pub roll_trigger_pressed: bool,
    pub last_jump_location: Vec3,
    pub constrain_look: bool,
    pub min_look_constraint: Rotator,
    pub max_look_constraint: Rotator,
    /// RootControl1p.BoneTranslation (actor space); the visible offset is this times
    /// `root_offset_strength` (SetSkelControlStrength blending).
    pub root_offset: Vec3,
    pub root_offset_strength: f32,
    pub root_offset_strength_target: f32,
    pub root_offset_blend_time_to_go: f32,
    /// RootControl's BoneTranslationSpace (SetRootOffset's TranslationSpace).
    pub root_offset_space: BoneControlSpace,
    pub leg_rotation: i32,
    /// TdPawn.bGoingForward (legs follow the velocity, or its reverse when running backwards),
    /// LegRotationSlowTimer and the [TdGame.TdPawn] leg angle limits.
    pub going_forward: bool,
    /// TdPawn.bDisableCharacterSounds / bCharacterInhaling (GetCharacterSoundCue).
    pub disable_character_sounds: bool,
    pub character_inhaling: bool,
    pub leg_rotation_slow_timer: f32,
    pub go_back_leg_angle_limit_min: i32,
    pub go_back_leg_angle_limit_max: i32,
    pub leg_angle_limit_fudge: i32,
    /// TdPawn.AgainstWallState and the hand spots / wall normal CheckAgainstWall found.
    pub against_wall_state: crate::body::AgainstWall,
    pub against_wall_left_hand: Vec3,
    pub against_wall_right_hand: Vec3,
    pub against_wall_normal: Vec3,
    /// TdPawn.AnimationMovementState: forces the anim tree's movement-state switch (None = use
    /// MovementState); PendingAnimationMovementState waits for its SetTimer.
    pub animation_movement_state: Move,
    /// TdPlayerPawn states: UncontrolledFall (only Landing / SoftLanding may follow) and Dying.
    pub uncontrolled_fall: bool,
    pub dying: bool,
    /// TdPawn.bClimbLeftHand / bClimbDownFast / LadderType (TdAnimNodeClimb reads them).
    pub climb_left_hand: bool,
    pub climb_down_fast: bool,
    pub ladder_type: crate::ladder::LadderType,
    /// TdPawn.ActiveMovementVolume and the ladder volume the pawn's location is in (index into
    /// Sim::ladders).
    pub active_movement_volume: Option<crate::volumes::VolumeRef>,
    /// The movement volume the pawn is in (its PhysicsVolume, when that is one).
    pub ladder_physics_volume: Option<crate::volumes::VolumeRef>,
    pub pending_animation_movement_state: Move,
    /// TdPlayerPawn.FirstPersonDPG / FirstPersonLowerBodyDPG (Mesh1p / Mesh1pLowerBody).
    pub first_person_dpg: Dpg,
    pub first_person_lower_body_dpg: Dpg,

    // speed / acceleration model
    pub speed_curve_light_weapon: InterpCurve,
    pub accel_curve_light_weapon: InterpCurve,
    /// TdPawn.SpeedCurve_HeavyWeapon (0 -> 400 uu/s over 1 s) as its AccelCurve.
    pub accel_curve_heavy_weapon: InterpCurve,
    pub speed_max_base_velocity: f32,
    pub speed_min_base_velocity: f32,
    pub speed_strafe_velocity_acceleration_factor: f32,
    pub speed_walk_velocity_acceleration_factor: f32,
    pub speed_sprint_velocity_acceleration_factor: f32,
    pub speed_energy_deceleration_time: f32,
    pub speed_energy_deceleration_exponent: f32,
    pub speed_turn_deceleration_factor: f32,
    pub speed_sprint_energy: f32,
    pub upward_walk_friction_scale: f32,
    pub downward_walk_friction_scale: f32,
    pub min_walk_friction_modify: f32,
    pub max_walk_friction_modify: f32,
    pub upward_slide_friction_scale: f32,
    pub downward_slide_friction_scale: f32,
    pub braking_friction_strength: f32,
    pub falling_uncontrolled_height: f32,
    pub sneak_velocity: f32,
    pub walk_velocity: f32,
    pub jog_velocity: f32,
    pub run_velocity: f32,
    pub sprint_velocity: f32,
    pub average_speed: f32,

    // ---- TdPlayerPawn
    pub edge_stop_min_height: f32,
    pub edge_check_max_speed: f32,
    pub edge_check_distance: f32,
}

impl Pawn {
    /// The mesh offset the RootControl skel control applies this frame.
    pub fn root_offset_effective(&self) -> Vec3 {
        self.root_offset * self.root_offset_strength
    }

    pub fn new(cfg: &Config) -> Self {
        let pawn = &["TdPlayerPawn", "TdPawn"];
        let speed_curve =
            InterpCurve::linear(&[(0.0, 0.0), (0.4, 400.0), (1.0, 520.0), (3.5, 650.0), (7.0, 720.0)]);
        let accel_curve = build_accel_curve(&speed_curve, 10);
        Pawn {
            location: Vec3::ZERO,
            rotation: Rotator::ZERO,
            velocity: Vec3::ZERO,
            acceleration: Vec3::ZERO,
            physics: Physics::Walking,
            base: false,
            collide_world: true,
            just_teleported: false,
            // TdPawn CollisionCylinder
            collision_radius: 30.0,
            collision_height: 90.0,
            default_collision_radius: 30.0,
            default_collision_height: 90.0,

            max_step_height: 35.0,
            max_jump_height: 96.0,
            walkable_floor_z: 0.71,
            ground_speed: 720.0,
            air_speed: 2400.0,
            accel_rate: 6144.0,
            jump_z: 420.0,
            air_control: 0.025,
            base_eye_height: 76.0,
            floor: Vec3::new(0.0, 0.0, 1.0),
            rm_velocity: Vec3::ZERO,
            can_jump: true,
            can_walk_off_ledges: false,
            is_walking: false,
            is_crouched: false,
            avoid_ledges: true,
            force_floor_check: false,
            force_max_accel: false,
            force_rm_velocity: false,
            force_regular_velocity: false,
            ground_friction: 8.0,
            fluid_friction: 0.3,
            terminal_velocity: 3500.0,
            world_gravity_z: crate::config::atof(cfg.raw_in("Engine.WorldInfo", "DefaultGravityZ").unwrap_or("-800")),

            movement_state: Move::Walking,
            old_movement_state: Move::Walking,
            pending_movement_state: Move::Walking,
            allow_move_change: true,
            gravity_modifier: 1.0,
            gravity_modifier_timer: 0.0,
            max_health: 100,
            regenerate_delay: cfg.f32(&["TdPlayerPawn", "TdPawn"], "RegenerateDelay", 5.0),
            regenerate_health_per_second: cfg.f32(&["TdPlayerPawn", "TdPawn"], "RegenerateHealthPerSecond", 25.0),
            time_since_last_damage: 0.0,
            health_frac: 0.0,
            custom_sound_input: 0.0,
            swing_control: SkelControl::default(),
            enter_falling_height: 0.0,
            max_wall_step_height: 35.0,
            new_floor_smooth: 0.0,
            target_mesh_translation_z: 0.0,
            mesh_translation_z: 0.0,
            mesh_offset_xy: Vec3::ZERO,
            is_wall_walking: false,
            force_max_accel_one_frame: false,
            uncontrolled_slide: false,
            uncontrolled_slide_normal: Vec3::ZERO,
            is_using_root_motion: false,
            is_using_root_rotation: false,
            can_uncrouch: true,
            found_ledge: false,
            found_ledge_excludes_hand_moves: false,
            found_ledge_excludes_foot_moves: false,
            move_ledge_location: Vec3::ZERO,
            move_ledge_normal: Vec3::ZERO,
            move_normal: Vec3::ZERO,
            move_location: Vec3::ZERO,
            move_ledge_result: 0,
            illegal_ledge_normal: Vec3::ZERO,
            illegal_ledge_timer: 0.0,
            ledge_find_extent: Vec3::new(10.0, 10.0, 86.0),
            ledge_find_distance: 350.0,
            ledge_find_depth: 4.0,
            evade_timer: 0.0,
            as_filter_time: cfg.f32(pawn, "ASFilterTime", 3.0),
            as_poll_slots: 40,
            as_poll_interval: 0.0,
            as_poll_timer: 0.0,
            as_distance_accum: 0.0,
            as_slot_pointer: 0,
            as_time_data: Vec::new(),
            as_distance_data: Vec::new(),
            current_grab_turn_type: Default::default(),
            move_action_hint: MoveActionHint::None,
            move_action_max: false,
            override_walking_state: WalkingState::None,
            current_walking_state: WalkingState::Idle,
            velocity_magnitude_2d: 0.0,
            velocity_magnitude: 0.0,
            velocity_dir_2d: Vec3::ZERO,
            velocity_dir: Vec3::ZERO,
            face_rotation_time_left: 0.0,
            slide_stopped_time_stamp: -10.0,
            roll_trigger_time: -10.0,
            roll_trigger_pressed: false,
            last_jump_location: Vec3::ZERO,
            constrain_look: false,
            min_look_constraint: Rotator::new(-32768, -32768, -32768),
            max_look_constraint: Rotator::new(32768, 32768, 32768),
            root_offset: Vec3::ZERO,
            root_offset_strength: 0.0,
            root_offset_strength_target: 0.0,
            root_offset_blend_time_to_go: 0.0,
            root_offset_space: BoneControlSpace::World,
            leg_rotation: 0,
            going_forward: true,
            disable_character_sounds: false,
            character_inhaling: false,
            leg_rotation_slow_timer: 0.0,
            go_back_leg_angle_limit_min: cfg.i32(pawn, "GoBackLegAngleLimitMin", -16384),
            go_back_leg_angle_limit_max: cfg.i32(pawn, "GoBackLegAngleLimitMax", 16384),
            leg_angle_limit_fudge: cfg.i32(pawn, "LegAngleLimitFudge", 2000),
            against_wall_state: crate::body::AgainstWall::None,
            against_wall_left_hand: Vec3::ZERO,
            against_wall_right_hand: Vec3::ZERO,
            against_wall_normal: Vec3::ZERO,
            animation_movement_state: Move::None,
            uncontrolled_fall: false,
            dying: false,
            climb_left_hand: false,
            climb_down_fast: false,
            ladder_type: crate::ladder::LadderType::Ladder,
            active_movement_volume: None,
            ladder_physics_volume: None,
            pending_animation_movement_state: Move::None,
            first_person_dpg: Dpg::Foreground,
            first_person_lower_body_dpg: Dpg::Intermediate,

            speed_curve_light_weapon: speed_curve,
            accel_curve_light_weapon: accel_curve,
            accel_curve_heavy_weapon: build_accel_curve(&InterpCurve::linear(&[(0.0, 0.0), (1.0, 400.0)]), 10),
            speed_max_base_velocity: cfg.f32(pawn, "SpeedMaxBaseVelocity", 400.0),
            speed_min_base_velocity: cfg.f32(pawn, "SpeedMinBaseVelocity", 10.0),
            speed_strafe_velocity_acceleration_factor: cfg.f32(pawn, "SpeedStrafeVelocityAccelerationFactor", 10.0),
            speed_walk_velocity_acceleration_factor: cfg.f32(pawn, "SpeedWalkVelocityAccelerationFactor", 7.0),
            speed_sprint_velocity_acceleration_factor: cfg.f32(pawn, "SpeedSprintVelocityAccelerationFactor", 30.0),
            speed_energy_deceleration_time: cfg.f32(pawn, "SpeedEnergyDecelerationTime", 3.0),
            speed_energy_deceleration_exponent: cfg.f32(pawn, "SpeedEnergyDecelerationExponent", 0.5),
            speed_turn_deceleration_factor: cfg.f32(pawn, "SpeedTurnDecelerationFactor", 10.0),
            speed_sprint_energy: 0.0,
            upward_walk_friction_scale: cfg.f32(pawn, "UpwardWalkFrictionScale", 1.1),
            downward_walk_friction_scale: cfg.f32(pawn, "DownwardWalkFrictionScale", 0.8),
            min_walk_friction_modify: cfg.f32(pawn, "MinWalkFrictionModify", 0.4),
            max_walk_friction_modify: cfg.f32(pawn, "MaxWalkFrictionModify", 2.0),
            upward_slide_friction_scale: cfg.f32(pawn, "UpwardSlideFrictionScale", 5.0),
            downward_slide_friction_scale: cfg.f32(pawn, "DownwardSlideFrictionScale", 1.8),
            braking_friction_strength: cfg.f32(pawn, "BrakingFrictionStrength", 1.0),
            falling_uncontrolled_height: cfg.f32(pawn, "FallingUncontrolledHeight", 1000.0),
            sneak_velocity: cfg.f32(pawn, "SneakVelocity", 5.0),
            walk_velocity: cfg.f32(pawn, "WalkVelocity", 50.0),
            jog_velocity: cfg.f32(pawn, "JogVelocity", 260.0),
            run_velocity: cfg.f32(pawn, "RunVelocity", 400.0),
            sprint_velocity: cfg.f32(pawn, "SprintVelocity", 630.0),
            average_speed: 0.0,

            edge_stop_min_height: cfg.f32(pawn, "EdgeStopMinHeight", 36.0),
            edge_check_max_speed: cfg.f32(pawn, "EdgeCheckMaxSpeed", 300.0),
            edge_check_distance: cfg.f32(pawn, "EdgeCheckDistance", 20.0),
        }
    }

    /// `GetCylinderExtent()`.
    pub fn extent(&self) -> Vec3 {
        Vec3::new(self.collision_radius, self.collision_radius, self.collision_height)
    }

    /// TdPawn::GetGravityZ (vt77): WorldInfo gravity scaled by GravityModifier.
    pub fn gravity_z(&self) -> f32 {
        self.world_gravity_z * self.gravity_modifier
    }
}

/// TdPawn's curve init (vt119 -> 0x12C2DB0): AccelCurve(speed) = d(SpeedCurve)/dt, sampled at
/// `n + 1` speeds, using a numerically inverted speed curve and a forward difference of 0.5/n s.
pub fn build_accel_curve(speed: &InterpCurve, n: i32) -> InterpCurve {
    let mut out = InterpCurve::default();
    let pts = &speed.points;
    if pts.is_empty() {
        return out;
    }
    let t_end = pts[pts.len() - 1].in_val;
    let v_end = pts[pts.len() - 1].out_val;
    // speed -> time, 10 samples per key
    let samples = 10 * pts.len() as i32;
    let mut inv = InterpCurve::default();
    for i in 0..=samples {
        let t = i as f32 * t_end / samples as f32;
        inv.add_point(speed.eval(t, 0.0), t);
    }
    let dt = 0.5 / n as f32;
    for j in 0..=n {
        let v = j as f32 * v_end / n as f32;
        let t = inv.eval(v, 0.0);
        let accel = (speed.eval(t + dt, 0.0) - speed.eval(t, 0.0)) * (1.0 / dt);
        out.add_point(v, accel);
    }
    out
}
