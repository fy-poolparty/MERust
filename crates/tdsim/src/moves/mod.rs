//! The TdMove state machine: one move object per `TdPawn.EMovement` (built from
//! `TdPawn.MoveClasses`), the TdMove / TdPhysicsMove base behaviour, and per-class dispatch.
//!
//! Each move class lives in its own module as `impl Sim` blocks named after the UnrealScript
//! functions (`walking_start_move` = TdMove_Walking.StartMove).

pub mod aim;
pub mod balance;
pub mod climb;
pub mod air;
pub mod crouch;
pub mod falling;
pub mod grab;
pub mod jump;
pub mod landing;
pub mod melee;
pub mod disarm;
pub mod precise;
pub mod transfer;
pub mod vertigo;
pub mod springboard;
pub mod swing;
pub mod vault;
pub mod walking;
pub mod wallclimb;
pub mod wallrun;
pub mod zipline;

use crate::config::Config;
use crate::controller::CtrlState;
use crate::math::{Rotator, Vec3, UeVec};
use crate::pawn::{Move, MoveAction, Physics, Slot};
use crate::sim::{Sim, TimerFn};

/// `TdMove.EPreciseLocationMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PreciseMode {
    #[default]
    Fly = 0,
    Walk = 1,
    Jump = 2,
    SimJump = 3,
    Fall = 4,
}

/// The UnrealScript class implementing each move (TdPawn.MoveClasses).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    None,
    Walking,
    Falling,
    Grab,
    WallRun,
    WallClimb,
    SpringBoard,
    SpeedVault,
    VaultOver,
    GrabPullUp,
    Jump,
    WallrunJump,
    GrabJump,
    IntoGrab,
    Crouch,
    Slide,
    Landing,
    Turn180,
    Turn180InAir,
    GrabTransfer,
    DodgeJump,
    WallrunDodgeJump,
    StepUp,
    WallClimbDodgeJump,
    WallClimb180TurnJump,
    Coil,
    SoftLanding,
    AutoStepUp,
    SkillRoll,
    LayOnGround,
    FallingUncontrolled,
    Vertigo,
    IntoClimb,
    Climb,
    Swing,
    SwingJump,
    IntoZipLine,
    ZipLine,
    Balance,
    Melee,
    MeleeAir,
    MeleeAirAbove,
    MeleeSlide,
    MeleeWallrun,
    MeleeCrouch,
    Stumble,
    Disarm,
    /// Classes not part of this port yet (melee, zipline, swing...): never allowed.
    Unported,
}

pub fn class_of(m: Move) -> Class {
    use Move as M;
    match m {
        M::None => Class::None,
        M::Walking => Class::Walking,
        M::Falling => Class::Falling,
        M::Grabbing => Class::Grab,
        M::WallRunningRight | M::WallRunningLeft | M::WallRun => Class::WallRun,
        M::WallClimbing => Class::WallClimb,
        M::SpringBoarding => Class::SpringBoard,
        M::SpeedVaulting => Class::SpeedVault,
        M::VaultOver => Class::VaultOver,
        M::GrabPullUp => Class::GrabPullUp,
        M::Jump => Class::Jump,
        M::WallRunJump => Class::WallrunJump,
        M::GrabJump => Class::GrabJump,
        M::IntoGrab => Class::IntoGrab,
        M::Crouch => Class::Crouch,
        M::Slide => Class::Slide,
        M::Landing => Class::Landing,
        M::Turn180 => Class::Turn180,
        M::Turn180InAir => Class::Turn180InAir,
        M::LayOnGround => Class::LayOnGround,
        M::GrabTransfer => Class::GrabTransfer,
        M::DodgeJump => Class::DodgeJump,
        M::WallRunDodgeJump => Class::WallrunDodgeJump,
        M::StepUp => Class::StepUp,
        M::WallClimbDodgeJump | M::WallClimbDodgeJumpLeft | M::WallClimbDodgeJumpRight => Class::WallClimbDodgeJump,
        M::WallClimb180TurnJump => Class::WallClimb180TurnJump,
        M::Coil => Class::Coil,
        M::SoftLanding => Class::SoftLanding,
        M::AutoStepUp => Class::AutoStepUp,
        M::SkillRoll => Class::SkillRoll,
        M::FallingUncontrolled => Class::FallingUncontrolled,
        M::Vertigo => Class::Vertigo,
        M::IntoClimb => Class::IntoClimb,
        M::Climb => Class::Climb,
        M::Swing => Class::Swing,
        M::SwingJump => Class::SwingJump,
        M::IntoZipLine => Class::IntoZipLine,
        M::ZipLine => Class::ZipLine,
        M::Balance => Class::Balance,
        M::Melee => Class::Melee,
        M::MeleeAir => Class::MeleeAir,
        M::MeleeAirAbove => Class::MeleeAirAbove,
        M::MeleeSlide => Class::MeleeSlide,
        M::MeleeWallrun => Class::MeleeWallrun,
        M::MeleeCrouch => Class::MeleeCrouch,
        M::Stumble => Class::Stumble,
        M::Snatch => Class::Disarm,
        _ => Class::Unported,
    }
}

/// UnrealScript class chain for config lookup (most derived first).
pub fn config_chain(c: Class) -> &'static [&'static str] {
    match c {
        Class::Walking => &["TdMove_Walking", "TdPhysicsMove", "TdMove"],
        Class::Falling => &["TdMove_Falling", "TdPhysicsMove", "TdMove"],
        Class::Jump => &["TdMove_Jump", "TdPhysicsMove", "TdMove"],
        Class::Landing => &["TdMove_Landing", "TdMove"],
        Class::Crouch => &["TdMove_Crouch", "TdPhysicsMove", "TdMove"],
        Class::Slide => &["TdMove_Slide", "TdMove"],
        Class::WallRun => &["TdMove_WallRun", "TdPhysicsMove", "TdMove"],
        Class::WallClimb => &["TdMove_WallClimb", "TdPhysicsMove", "TdMove"],
        Class::Grab => &["TdMove_Grab", "TdPhysicsMove", "TdMove"],
        Class::IntoGrab => &["TdMove_IntoGrab", "TdPhysicsMove", "TdMove"],
        Class::GrabPullUp => &["TdMove_GrabPullUp", "TdPhysicsMove", "TdMove"],
        Class::GrabJump => &["TdMove_GrabJump", "TdPhysicsMove", "TdMove"],
        Class::SpeedVault => &["TdMove_SpeedVault", "TdPhysicsMove", "TdMove"],
        Class::WallrunJump => &["TdMove_WallrunJump", "TdPhysicsMove", "TdMove"],
        Class::WallrunDodgeJump => &["TdMove_WallrunDodgeJump", "TdPhysicsMove", "TdMove"],
        Class::WallClimbDodgeJump => &["TdMove_WallClimbDodgeJump", "TdPhysicsMove", "TdMove"],
        Class::WallClimb180TurnJump => &["TdMove_WallClimb180TurnJump", "TdPhysicsMove", "TdMove"],
        Class::DodgeJump => &["TdMove_DodgeJump", "TdPhysicsMove", "TdMove"],
        Class::Turn180 => &["TdMove_180Turn", "TdMove"],
        Class::Turn180InAir => &["TdMove_180TurnInAir", "TdPhysicsMove", "TdMove"],
        Class::Coil => &["TdMove_Coil", "TdPhysicsMove", "TdMove"],
        Class::SoftLanding => &["TdMove_SoftLanding", "TdPhysicsMove", "TdMove"],
        Class::SkillRoll => &["TdMove_SkillRoll", "TdPhysicsMove", "TdMove"],
        Class::FallingUncontrolled => &["TdMove_FallingUncontrolled", "TdPhysicsMove", "TdMove"],
        Class::LayOnGround => &["TdMove_LayOnGround", "TdPhysicsMove", "TdMove"],
        Class::SpringBoard => &["TdMove_SpringBoard", "TdPhysicsMove", "TdMove"],
        Class::VaultOver => &["TdMove_VaultOver", "TdMove_SpeedVault", "TdPhysicsMove", "TdMove"],
        Class::IntoClimb => &["TdMove_IntoClimb", "TdPhysicsMove", "TdMove"],
        Class::Climb => &["TdMove_Climb", "TdPhysicsMove", "TdMove"],
        Class::GrabTransfer => &["TdMove_GrabTransfer", "TdPhysicsMove", "TdMove"],
        Class::Vertigo => &["TdMove_Vertigo", "TdPhysicsMove", "TdMove"],
        Class::Swing => &["TdMove_Swing", "TdPhysicsMove", "TdMove"],
        Class::SwingJump => &["TdMove_SwingJump", "TdPhysicsMove", "TdMove"],
        Class::IntoZipLine => &["TdMove_IntoZipLine", "TdPhysicsMove", "TdMove"],
        Class::ZipLine => &["TdMove_ZipLine", "TdPhysicsMove", "TdMove"],
        Class::Balance => &["TdMove_Balance", "TdPhysicsMove", "TdMove"],
        Class::Melee => &["TdMove_Melee", "TdMove_MeleeBase", "TdPhysicsMove", "TdMove"],
        Class::MeleeAir => &["TdMove_MeleeAir", "TdMove_MeleeBase", "TdPhysicsMove", "TdMove"],
        Class::MeleeAirAbove => &["TdMove_MeleeAirAbove", "TdMove_MeleeBase", "TdPhysicsMove", "TdMove"],
        Class::MeleeSlide => &["TdMove_MeleeSlide", "TdMove_MeleeBase", "TdPhysicsMove", "TdMove"],
        Class::MeleeWallrun => &["TdMove_MeleeWallrun", "TdMove_MeleeBase", "TdPhysicsMove", "TdMove"],
        Class::MeleeCrouch => &["TdMove_MeleeCrouch", "TdMove_MeleeBase", "TdPhysicsMove", "TdMove"],
        Class::Stumble => &["TdMove_Stumble", "TdMove_StumbleBase", "TdPhysicsMove", "TdMove"],
        Class::Disarm => &["TdMOVE_Disarm", "TdPhysicsMove", "TdMove"],
        _ => &["TdMove"],
    }
}

/// TdMove + TdPhysicsMove instance variables.
#[derive(Clone, Debug)]
pub struct MoveBase {
    pub class: Class,
    pub speed_modifier: f32,
    pub friction_modifier: f32,
    pub redo_move_time: f32,
    pub disable_collision: bool,
    pub constrain_look: bool,
    pub use_absolute_yaw_constraint: bool,
    pub look_at_target_location: bool,
    pub look_at_target_angle: bool,
    pub disable_face_rotation: bool,
    pub disable_controller_facing_pawn_yaw_rotation: bool,
    pub avoid_ledges: bool,
    pub use_precise_location: bool,
    pub reached_precise_location: bool,
    pub use_precise_rotation: bool,
    pub reached_precise_rotation: bool,
    pub delay_rotation_and_location_callback: bool,
    pub reset_camera_look: bool,
    pub use_custom_collision: bool,
    pub use_camera_collision: bool,
    pub enable_foot_placement: bool,
    pub enable_against_wall: bool,
    pub movement_group: u8,
    pub precise_location_mode: PreciseMode,
    pub disable_movement_time: f32,
    pub disable_look_time: f32,
    pub last_can_do_move_time: f32,
    pub last_stop_move_time: f32,
    pub move_active_time: f32,
    pub precise_location_speed: f32,
    pub precise_location: Vec3,
    pub precise_rotation_interpolation_time: f32,
    pub precise_rotation: Rotator,
    pub look_at_target_location_v: Vec3,
    pub look_at_target_angle_v: Rotator,
    pub look_at_target_interpolation_time: f32,
    pub look_at_target_duration: f32,
    pub cancel_reset_camera_look_time: f32,
    pub reset_camera_look_time: f32,
    pub min_look_constraint: Rotator,
    pub max_look_constraint: Rotator,
    pub custom_collision_radius: f32,
    pub custom_collision_height: f32,
    pub root_motion_scale: Vec3,
    pub root_offset: Vec3,
    pub current_custom_anim: Option<(Slot, String)>,
    pub timer: f32,
    pub timer_functions: Vec<u8>,
    // ---- TdPhysicsMove
    pub physics_move: bool,
    pub pawn_physics: Physics,
    pub controller_state: Option<CtrlState>,
    pub hand_plant_extent_check_height: f32,
    pub hand_plant_extent_check_width: f32,
    pub hand_plant_check_distance: f32,
    pub hand_plant_check_height: f32,
    pub context_move_distance_multiplier: f32,
    pub check_for_grab: bool,
    pub check_for_vault_over: bool,
    pub check_for_wall_climb: bool,
    pub check_for_edge_in_vel_dir: bool,
    pub check_exit_to_falling: bool,
    pub check_exit_to_uncontrolled_falling: bool,
    pub check_for_soft_landing: bool,
    pub delay_time_check_auto_moves: f32,
    pub exit_to_falling_z_speed: f32,
    pub soft_landing_z_speed_threshold: f32,
    pub time_to_soft_landing_threshold: f32,
    /// FirstPersonDPG / FirstPersonLowerBodyDPG, applied in StartMove.
    pub first_person_dpg: crate::pawn::Dpg,
    pub first_person_lower_body_dpg: crate::pawn::Dpg,
}

impl MoveBase {
    pub(crate) fn new(class: Class, cfg: &Config) -> Self {
        let ch = config_chain(class);
        // classes extending TdMove directly (not TdPhysicsMove)
        let physics_move = !matches!(class, Class::Landing | Class::None | Class::Unported | Class::Turn180 | Class::Slide);
        let mut b = MoveBase {
            class,
            speed_modifier: cfg.f32(ch, "SpeedModifier", 1.0),
            friction_modifier: cfg.f32(ch, "FrictionModifier", 1.0),
            redo_move_time: cfg.f32(ch, "RedoMoveTime", 0.0),
            disable_collision: false,
            constrain_look: false,
            use_absolute_yaw_constraint: false,
            look_at_target_location: false,
            look_at_target_angle: false,
            disable_face_rotation: false,
            disable_controller_facing_pawn_yaw_rotation: false,
            avoid_ledges: true,
            use_precise_location: false,
            reached_precise_location: false,
            use_precise_rotation: false,
            reached_precise_rotation: false,
            delay_rotation_and_location_callback: false,
            reset_camera_look: false,
            use_custom_collision: false,
            use_camera_collision: false,
            enable_foot_placement: false,
            enable_against_wall: false,
            movement_group: 0,
            precise_location_mode: PreciseMode::Fly,
            disable_movement_time: 0.0,
            disable_look_time: 0.0,
            last_can_do_move_time: 0.0,
            last_stop_move_time: -10.0,
            move_active_time: 0.0,
            precise_location_speed: 400.0,
            precise_location: Vec3::ZERO,
            precise_rotation_interpolation_time: 0.0,
            precise_rotation: Rotator::ZERO,
            look_at_target_location_v: Vec3::ZERO,
            look_at_target_angle_v: Rotator::ZERO,
            look_at_target_interpolation_time: 0.0,
            look_at_target_duration: -1.0,
            cancel_reset_camera_look_time: 0.0,
            reset_camera_look_time: 0.0,
            min_look_constraint: Rotator::new(-32768, -32768, -32768),
            max_look_constraint: Rotator::new(32768, 32768, 32768),
            custom_collision_radius: 0.0,
            custom_collision_height: 0.0,
            root_motion_scale: Vec3::new(1.0, 1.0, 1.0),
            root_offset: Vec3::ZERO,
            current_custom_anim: None,
            timer: 0.0,
            timer_functions: Vec::new(),
            physics_move,
            pawn_physics: Physics::Walking,
            controller_state: None,
            hand_plant_extent_check_height: cfg.f32(ch, "HandPlantExtentCheckHeight", 80.0),
            hand_plant_extent_check_width: cfg.f32(ch, "HandPlantExtentCheckWidth", 10.0),
            hand_plant_check_distance: cfg.f32(ch, "HandPlantCheckDistance", 200.0),
            hand_plant_check_height: cfg.f32(ch, "HandPlantCheckHeight", 112.0),
            context_move_distance_multiplier: cfg.f32(ch, "ContextMoveDistanceMultiplier", 1.8),
            check_for_grab: false,
            check_for_vault_over: false,
            check_for_wall_climb: false,
            check_for_edge_in_vel_dir: false,
            check_exit_to_falling: false,
            check_exit_to_uncontrolled_falling: false,
            check_for_soft_landing: false,
            delay_time_check_auto_moves: -1.0,
            exit_to_falling_z_speed: -400.0,
            soft_landing_z_speed_threshold: -400.0,
            time_to_soft_landing_threshold: 5.0,
            first_person_dpg: crate::pawn::Dpg::Foreground,
            first_person_lower_body_dpg: crate::pawn::Dpg::Intermediate,
        };
        // class defaultproperties, then config bools
        match class {
            Class::Walking => {
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.use_camera_collision = true;
                b.enable_foot_placement = true;
                b.enable_against_wall = true;
            }
            Class::Falling => {
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.check_for_grab = true;
                b.check_for_vault_over = true;
                b.check_exit_to_uncontrolled_falling = true;
                b.check_for_soft_landing = true;
            }
            Class::Jump => {
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.check_for_grab = true;
                b.check_for_vault_over = true;
                b.check_for_wall_climb = true;
                b.check_exit_to_falling = true;
                b.use_camera_collision = true;
            }
            Class::WallRun => {
                b.controller_state = Some(CtrlState::PlayerWallWalking);
                b.constrain_look = true;
                b.use_absolute_yaw_constraint = true;
                b.disable_controller_facing_pawn_yaw_rotation = true;
                b.use_camera_collision = true;
                b.redo_move_time = cfg.f32(ch, "RedoMoveTime", 0.15);
                b.min_look_constraint = Rotator::new(-13000, -16384, -32768);
                b.max_look_constraint = Rotator::new(13000, 16384, 32768);
            }
            Class::WallClimb => {
                b.pawn_physics = Physics::WallClimbing;
                b.controller_state = Some(CtrlState::PlayerWallWalking);
                b.check_for_grab = true;
                b.check_for_vault_over = true;
                b.check_for_edge_in_vel_dir = true;
                b.friction_modifier = cfg.f32(ch, "FrictionModifier", 0.3);
            }
            Class::Grab => {
                b.pawn_physics = Physics::None;
                b.controller_state = Some(CtrlState::PlayerGrabbing);
                b.disable_collision = true;
                b.constrain_look = true;
                b.disable_face_rotation = true;
                b.disable_look_time = 0.8;
                b.redo_move_time = cfg.f32(ch, "RedoMoveTime", 0.15);
                b.min_look_constraint = grab::GRAB_DEFAULT_MIN;
                b.max_look_constraint = grab::GRAB_DEFAULT_MAX;
            }
            Class::IntoGrab => {
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.check_for_vault_over = true;
                b.check_exit_to_uncontrolled_falling = true;
            }
            Class::GrabPullUp => {
                b.pawn_physics = Physics::Flying;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.disable_collision = true;
                b.constrain_look = true;
                b.disable_face_rotation = true;
                b.use_camera_collision = true;
                b.disable_movement_time = -1.0;
                b.disable_look_time = 0.2;
                b.min_look_constraint = Rotator::new(0, -10000, 0);
                b.max_look_constraint = Rotator::new(16384, 10000, 0);
            }
            Class::GrabJump => {
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.check_for_grab = true;
                b.check_for_vault_over = true;
                b.check_exit_to_falling = true;
                b.delay_time_check_auto_moves = 0.2;
                b.disable_face_rotation = true;
            }
            Class::Crouch => {
                b.speed_modifier = cfg.f32(ch, "SpeedModifier", 0.2);
                b.constrain_look = true;
                b.use_custom_collision = true;
                b.use_camera_collision = true;
                b.enable_foot_placement = true;
                b.enable_against_wall = true;
                b.min_look_constraint = Rotator::new(-14000, -32768, -32768);
                b.max_look_constraint = Rotator::new(14000, 32768, 32768);
            }
            Class::Slide => {
                b.friction_modifier = cfg.f32(ch, "FrictionModifier", 0.1);
                b.constrain_look = true;
                b.disable_face_rotation = true;
                b.avoid_ledges = false;
                b.use_custom_collision = true;
                b.use_camera_collision = true;
                b.disable_movement_time = -1.0;
                b.min_look_constraint = Rotator::new(-10000, -10000, 0);
                b.max_look_constraint = Rotator::new(10000, 10000, 0);
            }
            Class::WallrunJump => {
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.check_for_grab = true;
                b.check_for_vault_over = true;
                b.check_for_wall_climb = true;
                b.check_exit_to_falling = true;
                b.use_absolute_yaw_constraint = true;
            }
            Class::WallrunDodgeJump => {
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.check_exit_to_falling = true;
            }
            Class::WallClimbDodgeJump => {
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.check_for_grab = true;
                b.check_for_vault_over = true;
                b.check_for_wall_climb = true;
                b.check_exit_to_falling = true;
            }
            Class::WallClimb180TurnJump => {
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.check_for_grab = true;
                b.check_for_vault_over = true;
                b.check_for_wall_climb = true;
                b.check_exit_to_falling = true;
                b.exit_to_falling_z_speed = -800.0;
                b.disable_movement_time = 0.4;
                b.redo_move_time = cfg.f32(ch, "RedoMoveTime", 1.0);
            }
            Class::DodgeJump => {
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerGrabbing);
                b.check_exit_to_falling = true;
                b.exit_to_falling_z_speed = -190.0;
                b.disable_movement_time = -1.0;
                b.redo_move_time = cfg.f32(ch, "RedoMoveTime", 0.3);
            }
            Class::Turn180 => {
                b.friction_modifier = cfg.f32(ch, "FrictionModifier", 0.3);
                b.constrain_look = true;
                b.use_camera_collision = true;
                b.disable_movement_time = 0.3;
                b.redo_move_time = cfg.f32(ch, "RedoMoveTime", 0.5);
                b.min_look_constraint = Rotator::new(-10000, -16384, 0);
                b.max_look_constraint = Rotator::new(10000, 16384, 0);
            }
            Class::Turn180InAir => {
                b.pawn_physics = Physics::Falling;
                b.check_exit_to_uncontrolled_falling = true;
                b.check_for_soft_landing = true;
                b.constrain_look = true;
                b.look_at_target_location = true;
                b.disable_face_rotation = true;
                b.use_custom_collision = true;
                b.disable_movement_time = -1.0;
                b.min_look_constraint = Rotator::new(0, -5000, -32768);
                b.max_look_constraint = Rotator::new(32768, 5000, 32768);
                b.first_person_lower_body_dpg = crate::pawn::Dpg::Foreground;
            }
            Class::Coil => {
                b.first_person_lower_body_dpg = crate::pawn::Dpg::Foreground;
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.check_exit_to_uncontrolled_falling = true;
                b.constrain_look = true;
                b.use_custom_collision = true;
                b.min_look_constraint = Rotator::new(-5000, -32768, -32768);
                b.max_look_constraint = Rotator::new(30000, 32768, 32768);
            }
            Class::SoftLanding => {
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.check_exit_to_falling = true;
                b.check_for_soft_landing = true;
                b.constrain_look = true;
                b.disable_face_rotation = true;
                b.disable_movement_time = -1.0;
                b.min_look_constraint = Rotator::new(-16384, -5000, 0);
                b.max_look_constraint = Rotator::new(16384, 5000, 0);
            }
            Class::SkillRoll => {
                b.controller_state = Some(CtrlState::PlayerGrabbing);
                b.constrain_look = true;
                b.disable_face_rotation = true;
                b.avoid_ledges = false;
                b.min_look_constraint = Rotator::new(-2000, -5000, -32768);
                b.max_look_constraint = Rotator::new(32768, 5000, 32768);
            }
            Class::SpringBoard => {
                b.pawn_physics = Physics::Flying;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.check_for_grab = true;
                b.check_for_vault_over = true;
                b.check_for_wall_climb = true;
                b.check_exit_to_falling = true;
                b.delay_time_check_auto_moves = 0.7;
                b.disable_movement_time = -1.0;
            }
            Class::LayOnGround => {
                b.first_person_lower_body_dpg = crate::pawn::Dpg::Foreground;
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerGrabbing);
                b.friction_modifier = cfg.f32(ch, "FrictionModifier", 0.15);
                b.constrain_look = true;
                b.disable_face_rotation = true;
                b.avoid_ledges = false;
                b.use_custom_collision = true;
                b.min_look_constraint = Rotator::new(-2000, -5000, -32768);
                b.max_look_constraint = Rotator::new(32768, 5000, 32768);
            }
            Class::IntoClimb => {
                b.pawn_physics = Physics::Flying;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.first_person_dpg = crate::pawn::Dpg::Intermediate;
                b.disable_movement_time = -1.0;
                b.disable_look_time = -1.0;
                b.redo_move_time = cfg.f32(ch, "RedoMoveTime", 0.5);
            }
            Class::Climb => {
                b.pawn_physics = Physics::Flying;
                b.controller_state = Some(CtrlState::PlayerGrabbing);
                b.constrain_look = true;
                b.disable_face_rotation = true;
                b.first_person_dpg = crate::pawn::Dpg::Intermediate;
                b.redo_move_time = cfg.f32(ch, "RedoMoveTime", 0.5);
                b.min_look_constraint = Rotator::new(-5000, -32000, 0);
                b.max_look_constraint = Rotator::new(10000, 32000, 0);
            }
            Class::Swing => {
                b.pawn_physics = Physics::Flying;
                b.controller_state = Some(CtrlState::PlayerGrabbing);
                b.check_for_grab = true;
                b.check_for_vault_over = true;
                b.check_for_wall_climb = true;
                b.movement_group = 2;
                b.first_person_dpg = crate::pawn::Dpg::Intermediate;
                b.disable_look_time = -1.0;
            }
            Class::SwingJump => {
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.check_for_grab = true;
                b.check_for_vault_over = true;
                b.check_for_wall_climb = true;
                b.check_exit_to_falling = true;
                b.constrain_look = true;
                b.min_look_constraint = Rotator::new(-11000, -32768, -32768);
                b.max_look_constraint = Rotator::new(16384, 32768, 32768);
            }
            Class::IntoZipLine => {
                b.pawn_physics = Physics::Flying;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.constrain_look = true;
                b.movement_group = 3;
                b.redo_move_time = cfg.f32(ch, "RedoMoveTime", 0.5);
                b.min_look_constraint = Rotator::new(-2500, -7000, -32768);
                b.max_look_constraint = Rotator::new(32768, 7000, 32768);
            }
            Class::ZipLine => {
                b.pawn_physics = Physics::Flying;
                b.controller_state = Some(CtrlState::PlayerGrabbing);
                b.constrain_look = true;
                b.disable_face_rotation = true;
                b.movement_group = 2;
                b.disable_movement_time = -1.0;
                b.min_look_constraint = Rotator::new(-3200, -7000, -32768);
                b.max_look_constraint = Rotator::new(32768, 7000, 32768);
            }
            // TdMove_MeleeBase: MG_NonInteractive, MAM_NoHands
            Class::Melee => {
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.movement_group = 3;
                b.constrain_look = true;
                b.use_camera_collision = true;
                b.min_look_constraint = Rotator::new(-10000, -32768, -32768);
                b.max_look_constraint = Rotator::new(10000, 32768, 32768);
            }
            Class::MeleeAir => {
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.movement_group = 3;
                b.first_person_lower_body_dpg = crate::pawn::Dpg::Foreground;
                b.disable_movement_time = -1.0;
                b.redo_move_time = cfg.f32(ch, "RedoMoveTime", 0.3);
            }
            Class::MeleeAirAbove => {
                b.pawn_physics = Physics::Flying;
                b.movement_group = 3;
                b.disable_movement_time = -1.0;
                b.disable_look_time = -1.0;
            }
            Class::MeleeSlide => {
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.movement_group = 3;
                b.friction_modifier = cfg.f32(ch, "FrictionModifier", 0.1);
                b.use_custom_collision = true;
                b.disable_movement_time = -1.0;
                b.disable_look_time = -1.0;
            }
            Class::MeleeWallrun => {
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.movement_group = 3;
                b.constrain_look = true;
                b.min_look_constraint = Rotator::new(-3000, -8000, 0);
                b.max_look_constraint = Rotator::new(16000, 8000, 0);
            }
            Class::MeleeCrouch => {
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.movement_group = 3;
                b.speed_modifier = cfg.f32(ch, "SpeedModifier", 0.2);
                b.use_custom_collision = true;
            }
            Class::Disarm => {
                b.pawn_physics = Physics::Flying;
                b.movement_group = 3;
                b.first_person_dpg = crate::pawn::Dpg::Intermediate;
                b.disable_movement_time = -1.0;
                b.disable_look_time = -1.0;
                b.redo_move_time = cfg.f32(ch, "RedoMoveTime", 0.2);
            }
            Class::Stumble => {
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.movement_group = 2;
                b.check_exit_to_uncontrolled_falling = true;
                b.disable_movement_time = -1.0;
                b.disable_look_time = -1.0;
            }
            Class::Balance => {
                b.controller_state = Some(CtrlState::PlayerBalanceWalk);
                b.speed_modifier = cfg.f32(ch, "SpeedModifier", 0.34);
                b.constrain_look = true;
                b.disable_face_rotation = true;
                b.disable_controller_facing_pawn_yaw_rotation = true;
                b.avoid_ledges = false;
                b.movement_group = 2;
                b.redo_move_time = cfg.f32(ch, "RedoMoveTime", 0.5);
                b.min_look_constraint = Rotator::new(-13000, -6000, -32768);
                b.max_look_constraint = Rotator::new(25000, 6000, 32768);
            }
            Class::Vertigo => {
                b.use_camera_collision = true;
                b.movement_group = 3;
                b.disable_movement_time = 1.5;
                b.disable_look_time = 1.5;
                b.redo_move_time = cfg.f32(ch, "RedoMoveTime", 3.0);
            }
            Class::GrabTransfer => {
                b.pawn_physics = Physics::Flying;
                b.controller_state = Some(CtrlState::PlayerWalking);
                b.disable_face_rotation = true;
                b.disable_movement_time = -1.0;
                b.disable_look_time = -1.0;
            }
            Class::FallingUncontrolled => {
                b.pawn_physics = Physics::Falling;
                b.controller_state = Some(CtrlState::PlayerDying);
                b.check_for_soft_landing = true;
            }
            Class::SpeedVault | Class::VaultOver => {
                b.pawn_physics = Physics::Flying;
                b.disable_collision = true;
                b.constrain_look = true;
                b.disable_face_rotation = true;
                b.avoid_ledges = false;
                b.use_camera_collision = true;
                b.disable_movement_time = -1.0;
                b.disable_look_time = -1.0;
                b.min_look_constraint = Rotator::new(-3000, -8000, -32768);
                b.max_look_constraint = Rotator::new(6000, 8000, 32768);
            }
            _ => {}
        }
        b.check_for_grab = cfg.bool(ch, "bCheckForGrab", b.check_for_grab);
        b.check_for_vault_over = cfg.bool(ch, "bCheckForVaultOver", b.check_for_vault_over);
        b.check_for_wall_climb = cfg.bool(ch, "bCheckForWallClimb", b.check_for_wall_climb);
        b.check_for_edge_in_vel_dir = cfg.bool(ch, "bCheckForEdgeInVelDir", b.check_for_edge_in_vel_dir);
        b
    }
}

/// All move objects plus each class's own variables.
pub struct Moves {
    pub bases: Vec<MoveBase>,
    pub walking: walking::Walking,
    pub falling: falling::Falling,
    pub jump: jump::Jump,
    pub landing: landing::Landing,
    pub wallrun: wallrun::WallRun,
    pub wallclimb: wallclimb::WallClimb,
    pub into_grab: grab::IntoGrab,
    pub grab: grab::Grab,
    pub grab_pull_up: grab::GrabPullUp,
    pub grab_jump: grab::GrabJump,
    /// TdMove_VaultOver (Moves[VaultOver]); Moves[SpeedVaulting] is never selected by the player.
    pub vault: vault::Vault,
    pub slide: crouch::Slide,
    pub air: air::AirMoves,
    pub springboard: springboard::SpringBoard,
    pub into_climb: climb::IntoClimb,
    pub climb: climb::Climb,
    pub grab_transfer: transfer::GrabTransfer,
    pub vertigo: vertigo::Vertigo,
    pub swing: swing::Swing,
    pub swing_jump: swing::SwingJump,
    pub into_zipline: zipline::IntoZipLine,
    pub zipline: zipline::ZipLine,
    pub balance: balance::Balance,
    pub melee: melee::MeleeMoves,
    pub stumble: melee::Stumble,
    pub disarm: disarm::Disarm,
}

impl Moves {
    pub fn new(cfg: &Config) -> Self {
        let bases = (0..Move::COUNT).map(|i| MoveBase::new(class_of(Move::from_u8(i as u8)), cfg)).collect();
        Moves {
            bases,
            walking: walking::Walking::new(cfg),
            falling: falling::Falling::new(cfg),
            jump: jump::Jump::new(cfg),
            landing: landing::Landing::new(cfg),
            wallrun: wallrun::WallRun::new(cfg),
            wallclimb: wallclimb::WallClimb::new(cfg),
            into_grab: grab::IntoGrab::new(cfg),
            grab: grab::Grab::new(cfg),
            grab_pull_up: grab::GrabPullUp::new(cfg),
            grab_jump: grab::GrabJump::new(cfg),
            vault: vault::Vault::new(cfg),
            slide: crouch::Slide::new(cfg),
            air: air::AirMoves::new(cfg),
            springboard: springboard::SpringBoard::new(cfg),
            into_climb: climb::IntoClimb::new(cfg),
            climb: climb::Climb::new(cfg),
            grab_transfer: transfer::GrabTransfer::new(cfg),
            vertigo: vertigo::Vertigo::new(cfg),
            swing: swing::Swing::new(cfg),
            swing_jump: swing::SwingJump::new(cfg),
            into_zipline: zipline::IntoZipLine::new(cfg),
            zipline: zipline::ZipLine::new(cfg),
            balance: balance::Balance::new(cfg),
            melee: melee::MeleeMoves::new(cfg),
            stumble: melee::Stumble::new(),
            disarm: disarm::Disarm::default(),
        }
    }

    /// Moves[4], [5] and [40] share one object (ATdPawn::InitMoveObjects).
    fn slot(m: Move) -> usize {
        match m {
            Move::WallRunningLeft | Move::WallRun => Move::WallRunningRight as usize,
            _ => m as usize,
        }
    }

    pub fn base(&self, m: Move) -> &MoveBase {
        &self.bases[Self::slot(m)]
    }

    pub fn base_mut(&mut self, m: Move) -> &mut MoveBase {
        &mut self.bases[Self::slot(m)]
    }
}

// ============================================================================ dispatch

impl Sim {
    /// `Moves[m].CanDoMove()`.
    pub fn can_do_move(&mut self, m: Move) -> bool {
        // the CanDoMoves that refuse a heavy gun (GetWeaponType() == EWT_Heavy): 180TurnInAir,
        // Coil, DodgeJump, IntoClimb, IntoGrab, SpringBoard, Swing, Vertigo, WallRun,
        // SpeedVault, Melee, MeleeAir, MeleeCrouch
        if self.heavy_weapon()
            && matches!(
                class_of(m),
                Class::Turn180InAir | Class::Coil | Class::DodgeJump | Class::IntoClimb | Class::IntoGrab | Class::SpringBoard | Class::Swing | Class::Vertigo | Class::WallRun | Class::SpeedVault | Class::Melee | Class::MeleeAir | Class::MeleeCrouch
            )
        {
            return false;
        }
        match class_of(m) {
            Class::Walking => self.walking_can_do_move(m),
            Class::Falling => self.falling_can_do_move(m),
            Class::Jump => self.jump_can_do_move(m),
            Class::Landing => self.landing_can_do_move(m),
            Class::WallRun => self.wallrun_can_do_move(m),
            Class::WallClimb => self.wallclimb_can_do_move(m),
            Class::IntoGrab => self.into_grab_can_do_move(m),
            Class::Grab => self.grab_can_do_move(m),
            Class::GrabPullUp => self.grab_pull_up_can_do_move(m),
            Class::GrabJump => self.grab_jump_can_do_move(m),
            Class::SpeedVault | Class::VaultOver => self.vault_can_do_move(m),
            Class::Crouch => self.crouch_can_do_move(m),
            Class::Slide => self.slide_can_do_move(m),
            Class::WallrunDodgeJump => self.wallrun_dodge_can_do_move(m),
            Class::WallClimbDodgeJump => self.wallclimb_dodge_can_do_move(m),
            Class::WallClimb180TurnJump => self.wallclimb_180_can_do_move(m),
            Class::DodgeJump => self.dodge_can_do_move(m),
            Class::Turn180 => self.turn180_can_do_move(m),
            Class::Turn180InAir => self.turn180_air_can_do_move(m),
            Class::Coil => self.coil_can_do_move(m),
            Class::SpringBoard => self.springboard_can_do_move(m),
            Class::SkillRoll => self.skill_roll_can_do_move(m),
            Class::IntoClimb => self.into_climb_can_do_move(m),
            Class::GrabTransfer => self.grab_transfer_can_do_move(m),
            Class::Vertigo => self.vertigo_can_do_move(m),
            Class::Swing => self.swing_can_do_move(m),
            Class::IntoZipLine => self.into_zipline_can_do_move(m),
            Class::Balance => self.balance_can_do_move(m),
            Class::Melee => self.melee_can_do_move(m),
            Class::MeleeAir => self.melee_air_can_do_move(m),
            Class::MeleeAirAbove => self.tdmove_can_do_move(m),
            Class::MeleeSlide | Class::MeleeWallrun => self.tdmove_can_do_move(m),
            Class::MeleeCrouch => self.melee_crouch_can_do_move(m),
            Class::Stumble => self.stumble_can_do_move(m),
            Class::Disarm => self.disarm_can_do_move(m),
            Class::SwingJump | Class::ZipLine => self.tdmove_can_do_move(m),
            Class::WallrunJump | Class::SoftLanding | Class::FallingUncontrolled | Class::LayOnGround | Class::Climb => self.tdmove_can_do_move(m),
            Class::None | Class::Unported => false,
            // ported later; not reachable yet
            _ => false,
        }
    }

    pub fn start_move(&mut self, m: Move) {
        match class_of(m) {
            Class::Walking => self.walking_start_move(m),
            Class::Falling => self.falling_start_move(m),
            Class::Jump => self.jump_start_move(m),
            Class::Landing => self.landing_start_move(m),
            Class::WallRun => self.wallrun_start_move(m),
            Class::WallClimb => self.wallclimb_start_move(m),
            Class::IntoGrab => self.into_grab_start_move(m),
            Class::Grab => self.grab_start_move(m),
            Class::GrabPullUp => self.grab_pull_up_start_move(m),
            Class::GrabJump => self.grab_jump_start_move(m),
            Class::SpeedVault | Class::VaultOver => self.vault_start_move(m),
            Class::Crouch => self.crouch_start_move(m),
            Class::Slide => self.slide_start_move(m),
            Class::WallrunJump => self.wallrun_jump_start_move(m),
            Class::WallrunDodgeJump => self.wallrun_dodge_start_move(m),
            Class::WallClimbDodgeJump => self.wallclimb_dodge_start_move(m),
            Class::WallClimb180TurnJump => self.wallclimb_180_start_move(m),
            Class::DodgeJump => self.dodge_start_move(m),
            Class::Turn180 => self.turn180_start_move(m),
            Class::Turn180InAir => self.turn180_air_start_move(m),
            Class::Coil => self.coil_start_move(m),
            Class::SoftLanding => self.soft_landing_start_move(m),
            Class::SkillRoll => self.skill_roll_start_move(m),
            Class::LayOnGround => self.lay_start_move(m),
            Class::SpringBoard => self.springboard_start_move(m),
            Class::IntoClimb => self.into_climb_start_move(m),
            Class::Climb => self.climb_start_move(m),
            Class::GrabTransfer => self.grab_transfer_start_move(m),
            Class::Vertigo => self.vertigo_start_move(m),
            Class::Swing => self.swing_start_move(m),
            Class::SwingJump => self.swing_jump_start_move(m),
            Class::IntoZipLine => self.into_zipline_start_move(m),
            Class::ZipLine => self.zipline_start_move(m),
            Class::Balance => self.balance_start_move(m),
            Class::Melee => self.melee_start_move(m),
            Class::MeleeAir => self.melee_air_start_move(m),
            Class::MeleeAirAbove => self.melee_air_above_start_move(m),
            Class::MeleeSlide => self.melee_slide_start_move(m),
            Class::MeleeWallrun => self.melee_wallrun_start_move(m),
            Class::MeleeCrouch => self.melee_crouch_start_move(m),
            Class::Stumble => self.stumble_start_move(m),
            Class::Disarm => self.disarm_start_move(m),
            Class::FallingUncontrolled => {
                self.physics_move_start_move(m);
                // TdPlayerPawn.GoIntoUncontrolledFall -> state UncontrolledFall.BeginState
                if !self.pawn.uncontrolled_fall {
                    self.pawn.uncontrolled_fall = true;
                    // UncontrolledFall.BeginState: FallingSound
                    self.sound(crate::sound::SoundEvent::LoopStart { slot: crate::sound::LoopSlot::Falling, sound: crate::sound::LoopSound::Cue(crate::sound::DEATH_FALL_SOUND.into()), fade_in: 0.0 });
                    self.set_ignore_move_input(-1.0);
                    self.set_ignore_look_input(-1.0);
                }
            }
            _ => self.physics_move_start_move(m),
        }
    }

    pub fn stop_move(&mut self, m: Move) {
        match class_of(m) {
            Class::Walking => self.walking_stop_move(m),
            Class::Falling => self.falling_stop_move(m),
            Class::Jump => self.jump_stop_move(m),
            Class::Landing => self.tdmove_stop_move(m),
            Class::WallRun => self.wallrun_stop_move(m),
            Class::WallClimb => self.wallclimb_stop_move(m),
            Class::IntoGrab => self.into_grab_stop_move(m),
            Class::Grab => self.grab_stop_move(m),
            Class::GrabPullUp => self.grab_pull_up_stop_move(m),
            Class::GrabJump => self.grab_jump_stop_move(m),
            Class::SpeedVault | Class::VaultOver => self.vault_stop_move(m),
            Class::Crouch => self.crouch_stop_move(m),
            Class::Slide => self.slide_stop_move(m),
            Class::WallrunJump => self.wallrun_jump_stop_move(m),
            Class::WallClimb180TurnJump => self.wallclimb_180_stop_move(m),
            Class::Turn180 => self.turn180_stop_move(m),
            Class::Turn180InAir => self.turn180_air_stop_move(m),
            Class::Coil => self.coil_stop_move(m),
            Class::SoftLanding => self.soft_landing_stop_move(m),
            Class::SkillRoll => self.skill_roll_stop_move(m),
            Class::LayOnGround => self.lay_stop_move(m),
            Class::SpringBoard => self.springboard_stop_move(m),
            Class::IntoClimb => self.into_climb_stop_move(m),
            Class::Climb => self.climb_stop_move(m),
            Class::GrabTransfer => self.grab_transfer_stop_move(m),
            Class::Vertigo => self.vertigo_stop_move(m),
            Class::Swing => self.swing_stop_move(m),
            Class::IntoZipLine => self.into_zipline_stop_move(m),
            Class::ZipLine => self.zipline_stop_move(m),
            Class::Balance => self.balance_stop_move(m),
            Class::Melee => self.melee_stop_move(m),
            Class::MeleeSlide => self.melee_slide_stop_move(m),
            Class::MeleeCrouch => self.melee_crouch_stop_move(m),
            Class::MeleeAir | Class::MeleeWallrun | Class::MeleeAirAbove => self.melee_base_stop_move(m),
            Class::Stumble => self.stumble_stop_move(m),
            Class::Disarm => self.disarm_stop_move(m),
            _ => self.physics_move_stop_move(m),
        }
    }

    pub fn post_stop_move(&mut self, _m: Move) {}

    /// Native TickMove (vt71): move timer and MoveActiveTime.
    pub fn move_tick(&mut self, m: Move, dt: f32) {
        let b = self.moves.base_mut(m);
        if b.timer > 0.0 {
            b.timer -= dt;
            if b.timer <= 0.0 {
                b.timer = 0.0;
                self.move_on_timer(m);
            }
        }
        self.moves.base_mut(m).move_active_time += dt;
        match class_of(m) {
            Class::Falling => self.falling_tick_close_to_ground(m),
            Class::Swing if self.pawn.movement_state == m => self.swing_tick_move(),
            Class::Balance if self.pawn.movement_state == m => self.balance_tick_move(dt),
            _ => {}
        }
    }

    /// vt73 (TickSpecial, before vt70): TdPhysicsMove's auto-move detection.
    pub fn move_pre_physics(&mut self, m: Move) {
        if class_of(m) == Class::IntoGrab {
            self.into_grab_pre_physics(m);
        } else if self.moves.base(m).physics_move {
            self.physics_move_detect_auto_moves(m);
        }
    }

    /// vt70: precise location / rotation, then TdPhysicsMove exit checks and class extras.
    pub fn move_precise_physics(&mut self, m: Move, dt: f32) {
        match class_of(m) {
            Class::Climb => self.climb_tick(m, dt),
            Class::Swing => self.swing_tick(m, dt),
            // UTdMove_ZipLine vt70 doesn't run the TdPhysicsMove part
            Class::ZipLine => return self.zipline_tick(m, dt),
            _ => {}
        }
        // a class tick that changed the move ends this one
        if matches!(class_of(m), Class::Climb | Class::Swing) && self.pawn.movement_state != m {
            return;
        }
        self.update_precise_location(m, dt);
        if self.pawn.movement_state != m {
            return;
        }
        if self.moves.base(m).physics_move {
            self.physics_move_exit_checks(m);
        }
        if self.pawn.movement_state != m {
            return;
        }
        match class_of(m) {
            Class::Falling => self.moves.falling.air_time += dt,
            Class::WallRun => self.wallrun_tick(m),
            Class::WallClimb => self.wallclimb_tick(m),
            Class::Grab => self.grab_tick(m, dt),
            Class::Crouch => self.crouch_tick(),
            Class::Slide => self.slide_tick(m, dt),
            Class::Turn180InAir => self.turn180_air_tick(m),
            Class::Coil => self.coil_tick(dt),
            Class::Balance => self.balance_tick(m, dt),
            Class::Melee | Class::MeleeAir | Class::MeleeSlide | Class::MeleeWallrun | Class::MeleeCrouch | Class::MeleeAirAbove => self.melee_base_tick(m),
            _ => {}
        }
    }

    /// The move's own countdown (TdMove.SetTimer) expired: OnTimer.
    pub fn move_on_timer(&mut self, m: Move) {
        if self.pawn.movement_state != m && Moves::slot(m) != Moves::slot(self.pawn.movement_state) {
            return;
        }
        let m = self.pawn.movement_state;
        match class_of(m) {
            Class::WallRun => self.wallrun_on_timer(m),
            Class::Grab => self.grab_on_timer(m),
            Class::GrabJump => self.grab_jump_on_timer(m),
            Class::SpeedVault | Class::VaultOver => self.vault_on_timer(m),
            Class::Slide => self.slide_on_timer(m),
            Class::WallClimbDodgeJump => self.wallclimb_dodge_on_timer(m),
            Class::WallClimb180TurnJump => self.wallclimb_180_on_timer(m),
            Class::Turn180 => self.turn180_on_timer(m),
            Class::Climb => self.climb_on_timer(m),
            Class::GrabTransfer => self.grab_transfer_on_timer(m),
            Class::Vertigo => self.vertigo_on_timer(m),
            Class::SwingJump => self.swing_jump_on_timer(m),
            Class::ZipLine => self.zipline_on_timer(m),
            Class::Melee => self.melee_on_timer(m),
            Class::MeleeAir | Class::MeleeSlide | Class::MeleeWallrun => self.melee_hit_detection_on(m),
            Class::MeleeAirAbove => self.melee_air_above_on_timer(m),
            Class::Stumble => self.stumble_on_timer(m),
            _ => {}
        }
    }

    pub fn move_on_move_timer(&mut self, m: Move, id: u8) {
        if self.pawn.movement_state != m {
            return;
        }
        match class_of(m) {
            Class::Walking => self.walking_on_move_timer(m, id),
            Class::Grab => self.grab_on_move_timer(m, id),
            Class::GrabPullUp => self.grab_pull_up_on_move_timer(m, id),
            Class::Crouch => self.crouch_on_move_timer(m, id),
            Class::Swing => self.swing_on_move_timer(m, id),
            Class::Balance => self.balance_on_move_timer(m, id),
            _ => {}
        }
    }

    /// Moves[MovementState].Landed (TdMove default: go to Landing).
    pub fn move_landed(&mut self, _normal: Vec3) {
        let m = self.pawn.movement_state;
        match class_of(m) {
            // TdMove_SkillRoll.Landed is empty
            Class::SkillRoll => {}
            // TdMove_Climb.Landed: LetGo
            Class::Climb => self.climb_landed(m),
            Class::MeleeAir => self.melee_air_landed(m),
            // TdMove_MeleeWallrun.Landed: only from MS_MeleePending
            Class::MeleeWallrun => {}
            // TdMove_Stumble.Landed: only after the stumble fall (AnimationMovementState 180TurnInAir)
            Class::Stumble => {
                if self.pawn.animation_movement_state == Move::Turn180InAir {
                    self.anim.stop(crate::pawn::Slot::FullBody, 0.2);
                    if self.can_do_move(Move::Landing) {
                        self.set_move(Move::Landing, false, false);
                    }
                }
            }
            _ => {
                if self.can_do_move(Move::Landing) {
                    self.set_move(Move::Landing, false, false);
                }
            }
        }
    }

    /// Moves[MovementState].HitWall.
    pub fn move_hit_wall(&mut self, _normal: Vec3) {
        let m = self.pawn.movement_state;
        // TdMove_Climb.HitWall: LetGo (climbing down into the floor steps off the ladder)
        match class_of(m) {
            Class::Climb => self.climb_landed(m),
            // TdMove_GrabTransfer.HitWall
            Class::GrabTransfer => self.grab_transfer_fall(m),
            Class::IntoZipLine => self.into_zipline_fall(m),
            Class::ZipLine => self.zipline_hit_wall(m, _normal),
            Class::Balance => self.balance_hit_wall(m),
            _ => {}
        }
    }

    pub fn move_reached_wall(&mut self, m: Move) {
        match class_of(m) {
            Class::WallRun => self.wallrun_reached_wall(m),
            Class::WallClimb => self.wallclimb_reached_wall(m),
            _ => {}
        }
    }

    pub fn move_update_view_rotation(&mut self, m: Move, view: &mut Rotator, dt: f32, delta: &mut Rotator) {
        match class_of(m) {
            Class::Grab => return self.grab_update_view_rotation(m, view, dt, delta),
            Class::GrabJump => self.grab_jump_view_rotation(m, delta),
            _ => {}
        }
        self.tdmove_update_view_rotation(m, view, dt, delta);
        match class_of(m) {
            Class::Walking => self.walking_after_view_rotation(m, delta),
            // TdMove_180TurnInAir / TdMove_SoftLanding.UpdateViewRotation
            Class::Turn180InAir | Class::SoftLanding => {
                if delta.yaw != 0 {
                    self.abort_look_at_target(m);
                }
            }
            Class::WallClimb => self.wallclimb_after_view_rotation(m),
            Class::Vertigo => self.vertigo_after_view_rotation(m),
            Class::ZipLine => self.zipline_view_rotation(m, delta),
            Class::Melee | Class::MeleeAir | Class::MeleeSlide | Class::MeleeWallrun | Class::MeleeCrouch if self.pc.melee_lock_on => {
                let v = *view;
                self.melee_auto_lock_on(dt, v, delta);
            }
            // TdMOVE_Disarm.UpdateViewRotation: the canned snatch keeps the view on the enemy
            // (kept on even with the melee lock-on off: the takedown is aligned to it)
            // (not on a miss: there the lock-on would grab the nearest cop, and SnatchFail's
            // lunge into its cylinder would slide the player round it while the view turns)
            Class::Disarm if self.moves.disarm.target.is_some() && self.moves.disarm.state != Some(crate::bots::DisarmState::Miss) => {
                let v = *view;
                self.melee_auto_lock_on(dt, v, delta);
            }
            _ => {}
        }
    }

    /// TdPawn.OnAnimEnd -> Moves[MovementState].OnCustomAnimEnd (only for the move's own anim).
    pub(crate) fn dispatch_anim_events(&mut self) {
        for e in self.anim.take_events() {
            let (slot, name, cease) = match e {
                crate::anim::AnimEvent::Notify(n) => {
                    self.fire_notify(&n);
                    continue;
                }
                crate::anim::AnimEvent::End { slot, name, .. } => (slot, name, false),
                crate::anim::AnimEvent::CeaseRelevantRootMotion { slot, name } => (slot, name, true),
            };
            let m = self.pawn.movement_state;
            let is_current = self
                .moves
                .base(m)
                .current_custom_anim
                .as_ref()
                .is_some_and(|(s, n)| *s == slot && n.eq_ignore_ascii_case(&name));
            if is_current {
                if cease {
                    self.move_on_cease_relevant_root_motion(m);
                } else {
                    self.move_on_custom_anim_end(m, &name);
                }
            }
        }
    }

    /// Moves[MovementState].OnCeaseRelevantRootMotion.
    fn move_on_cease_relevant_root_motion(&mut self, m: Move) {
        match class_of(m) {
            // TdMove_180TurnInAir (only JumpTurnFly plays there)
            Class::Turn180InAir => self.use_root_rotation(false),
            Class::GrabPullUp => self.use_root_motion(false),
            Class::Climb => self.climb_on_cease_relevant_root_motion(m),
            Class::Swing => self.swing_on_cease_relevant_root_motion(m),
            Class::Balance => self.balance_on_cease_relevant_root_motion(m),
            _ => {}
        }
    }

    fn move_on_custom_anim_end(&mut self, m: Move, name: &str) {
        match class_of(m) {
            Class::Walking => self.walking_on_custom_anim_end(m),
            Class::Landing => self.landing_on_custom_anim_end(m),
            Class::Grab => self.grab_on_custom_anim_end(m, name),
            Class::GrabPullUp => self.grab_pull_up_on_custom_anim_end(m),
            // TdMove_GrabJump.OnCustomAnimEnd
            Class::GrabJump => {
                self.set_move(Move::Falling, false, false);
            }
            // TdMove_180Turn / TdMove_SkillRoll.OnCustomAnimEnd
            Class::Turn180 | Class::SkillRoll => {
                self.set_move(Move::Walking, false, false);
            }
            Class::LayOnGround => self.lay_on_custom_anim_end(),
            Class::IntoClimb => self.into_climb_on_custom_anim_end(m),
            Class::Melee => self.melee_on_custom_anim_end(m),
            Class::MeleeAir => self.melee_air_on_custom_anim_end(m),
            Class::MeleeAirAbove => self.melee_air_above_on_custom_anim_end(m),
            Class::MeleeSlide => self.melee_slide_on_custom_anim_end(m),
            // TdMove_MeleeWallrun.OnCustomAnimEnd
            Class::MeleeWallrun => {
                self.set_move(Move::Walking, false, false);
            }
            Class::MeleeCrouch => self.melee_crouch_on_custom_anim_end(m),
            Class::Stumble => self.stumble_on_custom_anim_end(m),
            // TdMOVE_Disarm.OnCustomAnimEnd
            Class::Disarm => {
                self.set_move(Move::Walking, false, false);
            }
            _ => {}
        }
    }

    /// TdPlayerMoveManager.HandleMoveAction.
    pub fn move_manager_handle_action(&mut self, a: MoveAction) {
        use Move as M;
        let prev = self.pawn.movement_state;
        self.move_handle_action(prev, a);
        if prev != self.pawn.movement_state {
            return;
        }
        let try_set = |s: &mut Sim, m: Move| -> bool {
            if s.can_do_move(m) {
                s.set_move(m, false, false);
                true
            } else {
                false
            }
        };
        match prev {
            M::Vertigo | M::Walking | M::Turn180 => match a {
                MoveAction::Jump => {
                    let h = self.find_ledge_in_front();
                    self.pawn.found_ledge = h.is_some();
                    if let Some(h) = h {
                        self.pawn.move_ledge_location = h.ledge_location;
                        self.pawn.move_ledge_normal = h.ledge_normal;
                        self.pawn.move_normal = h.move_normal;
                    }
                    let _ = try_set(self, M::DodgeJump) || try_set(self, M::SpringBoarding) || try_set(self, M::Jump);
                }
                MoveAction::Crouch => {
                    let _ = try_set(self, M::Slide) || try_set(self, M::Crouch);
                }
                MoveAction::Turn => {
                    try_set(self, M::Turn180);
                }
                // Barge isn't ported
                MoveAction::Melee => {
                    try_set(self, M::Melee);
                }
                MoveAction::Snatch => {
                    try_set(self, M::Snatch);
                }
                MoveAction::Stumble => {
                    try_set(self, M::Stumble);
                }
                _ => {}
            },
            M::Turn180InAir => {
                if a == MoveAction::Turn {
                    try_set(self, M::Turn180);
                }
            }
            M::SpringBoarding => {
                if a == MoveAction::Crouch {
                    try_set(self, M::Coil);
                }
            }
            M::IntoGrab | M::Jump | M::WallRunJump | M::GrabJump | M::SwingJump | M::WallClimb180TurnJump | M::Falling => match a {
                MoveAction::Crouch => {
                    try_set(self, M::Coil);
                }
                MoveAction::Jump => {
                    try_set(self, M::WallKick);
                }
                MoveAction::Turn => {
                    try_set(self, M::Turn180InAir);
                }
                // AirBarge isn't ported
                MoveAction::Melee => {
                    try_set(self, M::MeleeAir);
                }
                MoveAction::Stumble => {
                    self.moves.stumble.in_air = true;
                    try_set(self, M::Stumble);
                }
                _ => {}
            },
            M::Slide if a == MoveAction::Melee => {
                try_set(self, M::MeleeSlide);
            }
            M::Melee | M::Stumble => {
                if a == MoveAction::Stumble {
                    try_set(self, M::Stumble);
                } else if a == MoveAction::Snatch && self.pawn.movement_state == M::Melee {
                    try_set(self, M::Snatch);
                }
            }
            M::Slide => match a {
                MoveAction::StopCrouch => {
                    if self.can_stop_move(M::Slide) {
                        let _ = try_set(self, M::Walking) || try_set(self, M::Crouch);
                    }
                }
                _ => {}
            },
            M::RumpSlide => {
                if a == MoveAction::Jump {
                    try_set(self, M::Jump);
                }
            }
            M::Crouch if a == MoveAction::Melee => {
                try_set(self, M::MeleeCrouch);
            }
            M::Crouch if a == MoveAction::Stumble => {
                try_set(self, M::Stumble);
            }
            M::Crouch => {
                if a == MoveAction::StopCrouch {
                    if self.pawn.physics == Physics::Falling {
                        try_set(self, M::Falling);
                    } else {
                        try_set(self, M::Walking);
                    }
                }
            }
            M::Grabbing => match a {
                MoveAction::Jump => {
                    let _ = try_set(self, M::GrabTransfer) || try_set(self, M::GrabPullUp) || try_set(self, M::GrabJump);
                }
                MoveAction::Crouch => self.grab_request_drop_down(),
                MoveAction::ClimbUpLong => {
                    try_set(self, M::GrabPullUp);
                }
                _ => {}
            },
            M::Climb => {
                if a == MoveAction::Jump {
                    let _ = try_set(self, M::GrabTransfer) || try_set(self, M::GrabJump);
                }
            }
            M::WallRunningLeft | M::WallRunningRight if a == MoveAction::Melee => {
                try_set(self, M::MeleeWallrun);
            }
            M::WallRunningLeft | M::WallRunningRight => match a {
                MoveAction::Jump => {
                    let _ = try_set(self, M::WallRunDodgeJump) || try_set(self, M::WallRunJump);
                }
                MoveAction::ClimbDown | MoveAction::Crouch => {
                    try_set(self, M::Falling);
                }
                _ => {}
            },
            // let go of the cable
            M::ZipLine => {
                if a == MoveAction::Crouch {
                    try_set(self, M::Falling);
                }
            }
            M::WallClimbing => match a {
                MoveAction::Jump => {
                    try_set(self, M::WallClimbDodgeJump);
                }
                MoveAction::Turn => {
                    try_set(self, M::WallClimb180TurnJump);
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// Moves[m].HandleMoveAction.
    fn move_handle_action(&mut self, m: Move, a: MoveAction) {
        match class_of(m) {
            Class::Walking => self.walking_handle_move_action(m, a),
            Class::WallRun => self.wallrun_handle_move_action(m, a),
            Class::Grab => self.grab_handle_move_action(m, a),
            Class::SpeedVault | Class::VaultOver => self.vault_handle_move_action(m, a),
            Class::Crouch => self.crouch_handle_move_action(m, a),
            Class::WallClimb180TurnJump => self.wallclimb_180_handle_move_action(m, a),
            Class::LayOnGround => self.lay_handle_move_action(m, a),
            Class::Climb => self.climb_handle_move_action(m, a),
            Class::Swing => self.swing_handle_move_action(m, a),
            Class::Melee => self.melee_handle_move_action(m, a),
            _ => {}
        }
    }

    /// Moves[m].CanStopMove.
    pub fn can_stop_move(&mut self, m: Move) -> bool {
        match class_of(m) {
            Class::Slide => self.slide_can_stop_move(),
            _ => true,
        }
    }

    // ============================================================================ TdMove

    /// TdMove.CanDoMove.
    pub fn tdmove_can_do_move(&mut self, m: Move) -> bool {
        let t = self.time;
        let b = self.moves.base_mut(m);
        if b.last_stop_move_time > 0.0 && b.redo_move_time > t - b.last_stop_move_time {
            return false;
        }
        // PawnOwner.IsInState('Dying')
        if self.pawn.dying {
            return false;
        }
        let b = self.moves.base_mut(m);
        b.last_can_do_move_time = t;
        true
    }

    /// TdMove.StartMove.
    pub fn tdmove_start_move(&mut self, m: Move) {
        let (dmt, dlt, custom, root_offset) = {
            let b = self.moves.base_mut(m);
            b.move_active_time = 0.0;
            (b.disable_movement_time, b.disable_look_time, b.use_custom_collision, b.root_offset)
        };
        self.set_ignore_move_input(dmt);
        self.set_ignore_look_input(dlt);
        // a light gun in a MovementGroup >= 2 move, a heavy one in >= 1: PC.StopFire
        let group = self.moves.base(m).movement_group;
        if let Some(w) = self.weapon.as_mut() {
            if (!w.class.heavy && group >= 2) || (w.class.heavy && group >= 1) {
                w.firing = false;
            }
        }
        // AimMode MAM_NoHands with a gun: SetUnarmed (the gun is put away for the move)
        if self.weapon.is_some() && m != Move::Snatch && crate::moves::aim::class_aim_mode(class_of(m)) == crate::moves::aim::AimMode::NoHands {
            self.set_weapon_anim_state(crate::weapons::WeaponAnimState::Unarmed);
        }
        // TdPlayerPawn.SetFirstPersonDPG / SetFirstPersonLowerBodyDPG
        self.pawn.first_person_dpg = self.moves.base(m).first_person_dpg;
        self.pawn.first_person_lower_body_dpg = self.moves.base(m).first_person_lower_body_dpg;
        if self.moves.base(m).disable_collision {
            self.pawn.collide_world = false;
        }
        let old = self.pawn.old_movement_state;
        if custom && !self.moves.base(old).use_custom_collision {
            self.shrink_collision(m);
        }
        self.set_root_offset(root_offset, 0.3);
    }

    /// TdMove.StopMove.
    pub fn tdmove_stop_move(&mut self, m: Move) {
        // AimMode MAM_NoHands with a gun: SetArmed
        if self.weapon.is_some() && m != Move::Snatch && crate::moves::aim::class_aim_mode(class_of(m)) == crate::moves::aim::AimMode::NoHands {
            self.set_weapon_anim_state(crate::weapons::WeaponAnimState::Relaxed);
        }
        self.stop_ignore_look_input();
        self.stop_ignore_move_input();
        if self.moves.base(m).disable_collision {
            self.pawn.collide_world = true;
        }
        let pending = self.pawn.pending_movement_state;
        if self.moves.base(m).use_custom_collision && !self.moves.base(pending).use_custom_collision {
            self.enlarge_collision(m);
        }
        self.use_root_motion(false);
        self.use_root_rotation(false);
        let t = self.time;
        let b = self.moves.base_mut(m);
        b.use_precise_location = false;
        b.use_precise_rotation = false;
        b.last_stop_move_time = t;
        b.look_at_target_angle = false;
        b.look_at_target_location = false;
        b.look_at_target_duration = -1.0;
        b.reset_camera_look = false;
        b.timer = 0.0;
        b.current_custom_anim = None;
        let funcs = std::mem::take(&mut b.timer_functions);
        self.set_root_offset(Vec3::ZERO, 0.3);
        self.clear_animation_movement_state(-1.0);
        for id in funcs {
            self.clear_timer(TimerFn::Move(m, id));
        }
    }

    /// TdMove.SetMoveTimer.
    pub fn set_move_timer(&mut self, m: Move, time: f32, looping: bool, id: u8) {
        self.moves.base_mut(m).timer_functions.push(id);
        self.set_timer(TimerFn::Move(m, id), time, looping);
    }

    pub fn clear_move_timer(&mut self, m: Move, id: u8) {
        self.clear_timer(TimerFn::Move(m, id));
    }

    /// TdMove.SetTimer (the move's own countdown, fires OnTimer).
    pub fn set_move_countdown(&mut self, m: Move, time: f32) {
        if time > 0.0 {
            self.moves.base_mut(m).timer = time;
        } else {
            self.move_on_timer(m);
            self.moves.base_mut(m).timer = 0.0;
        }
    }

    /// TdMove.PlayMoveAnim.
    pub fn play_move_anim(&mut self, m: Move, slot: Slot, name: &str, rate: f32, blend_in: f32, blend_out: f32, root_motion: bool, root_rotation: bool) {
        self.anim.play(slot, name, rate, blend_in, blend_out, false, root_motion, root_rotation);
        let playing = self.anim.current(slot).is_some();
        self.moves.base_mut(m).current_custom_anim = if playing { Some((slot, name.to_string())) } else { None };
    }

    /// TdMove.SetPreciseLocation.
    pub fn set_precise_location(&mut self, m: Move, loc: Vec3, mode: PreciseMode, speed: f32) {
        let gs = self.pawn.ground_speed;
        let b = self.moves.base_mut(m);
        b.precise_location_speed = if speed < 0.0 { gs } else { speed };
        b.precise_location_mode = mode;
        b.precise_location = loc;
        b.use_precise_location = true;
        b.reached_precise_location = false;
    }

    /// TdMove.SetPreciseRotation.
    pub fn set_precise_rotation(&mut self, m: Move, rot: Rotator, time: f32) {
        let b = self.moves.base_mut(m);
        b.precise_rotation = rot;
        b.precise_rotation_interpolation_time = time;
        b.use_precise_rotation = true;
        b.reached_precise_rotation = false;
    }

    /// TdMove.SetLookAtTargetLocation.
    pub fn set_look_at_target_location(&mut self, m: Move, target: Vec3, interp: f32, duration: f32) {
        let b = self.moves.base_mut(m);
        b.look_at_target_location = true;
        b.look_at_target_interpolation_time = interp;
        b.look_at_target_duration = if duration == -1.0 { duration } else { b.move_active_time + duration };
        b.look_at_target_location_v = target;
    }

    /// TdMove.SetLookAtTargetAngle.
    pub fn set_look_at_target_angle(&mut self, m: Move, target: Rotator, interp: f32, duration: f32) {
        let b = self.moves.base_mut(m);
        b.look_at_target_angle = true;
        b.look_at_target_interpolation_time = interp;
        b.look_at_target_duration = if duration == -1.0 { duration } else { b.move_active_time + duration };
        b.look_at_target_angle_v = target;
    }

    /// TdMove.AbortLookAtTarget.
    pub fn abort_look_at_target(&mut self, m: Move) {
        let b = self.moves.base_mut(m);
        b.look_at_target_location = false;
        b.look_at_target_angle = false;
        b.look_at_target_duration = -1.0;
    }

    /// TdMove.GetMinLookConstrainYaw / GetMaxLookConstrainYaw / GetMinLookConstrainPitch (virtual).
    fn look_constrain_getters(&self, m: Move) -> (i32, i32, i32) {
        if class_of(m) == Class::Climb {
            return self.climb_look_constrain_getters(m);
        }
        let b = self.moves.base(m);
        if class_of(m) == Class::Grab && self.moves.grab.shimmy != grab::Shimmy::NoShimmy {
            return (-5000, 5000, 0);
        }
        if class_of(m) == Class::WallrunJump {
            let w = &self.moves.air.wallrun_jump;
            return (w.min_constraint_world, w.max_constraint_world, b.min_look_constraint.pitch);
        }
        (b.min_look_constraint.yaw, b.max_look_constraint.yaw, b.min_look_constraint.pitch)
    }

    /// TdMove.ResetCameraLook.
    pub fn reset_camera_look(&mut self, m: Move, time: f32) {
        let t = self.time;
        let b = self.moves.base_mut(m);
        b.reset_camera_look = true;
        b.cancel_reset_camera_look_time = t + time;
        b.reset_camera_look_time = time;
    }

    /// TdMove.CanStand.
    pub fn can_stand(&self, location: Vec3, from_above: bool) -> bool {
        let r = self.pawn.default_collision_radius;
        let ext = Vec3::new(r, r, 8.0);
        let h = self.pawn.default_collision_height - ext.z;
        let start = location - Vec3::new(0.0, 0.0, h);
        let end = location + Vec3::new(0.0, 0.0, h);
        let (a, b) = if from_above { (start, end) } else { (end, start) };
        !self.movement_trace_for_blocking(a, b, ext)
    }

    /// TdMove.ShrinkCollision.
    pub fn shrink_collision(&mut self, m: Move) {
        let crouch_h = 0.5 * 122.0;
        let r = self.pawn.default_collision_radius;
        self.set_custom_collision_size(m, r, crouch_h);
        let dz = self.pawn.default_collision_height - crouch_h;
        self.move_actor(Vec3::new(0.0, 0.0, -dz));
        // SetTargetMeshZ(dz, bForceSet=true)
        self.pawn.target_mesh_translation_z = dz;
        self.pawn.mesh_translation_z = dz;
    }

    /// TdMove.EnlargeCollision.
    pub fn enlarge_collision(&mut self, m: Move) {
        let on_ground = self.pawn.physics != Physics::Falling;
        let crouch_h = 0.5 * 122.0;
        let (r, h) = (self.pawn.default_collision_radius, self.pawn.default_collision_height);
        self.set_custom_collision_size(m, r, h);
        let mut dz = h - crouch_h;
        if on_ground {
            dz += 2.0;
        }
        let hit = self.move_actor(Vec3::new(0.0, 0.0, dz));
        if hit.time < 1.0 {
            let l = self.pawn.location + Vec3::new(0.0, 0.0, dz);
            self.set_location(l);
        }
        self.pawn.target_mesh_translation_z = 0.0;
        self.pawn.mesh_translation_z = 0.0;
        if on_ground {
            let d = -self.pawn.floor * self.pawn.max_step_height;
            self.move_actor(d);
        }
    }

    pub(crate) fn set_custom_collision_size(&mut self, m: Move, radius: f32, height: f32) {
        self.pawn.collision_radius = radius;
        self.pawn.collision_height = height;
        let b = self.moves.base_mut(m);
        b.custom_collision_radius = radius;
        b.custom_collision_height = height;
    }

    /// TdMove.UpdateViewRotation.
    pub fn tdmove_update_view_rotation(&mut self, m: Move, out: &mut Rotator, dt: f32, delta: &mut Rotator) {
        let pawn_rot = self.pawn.rotation;
        let t = self.time;
        let b = self.moves.base(m).clone();
        if b.look_at_target_location {
            if b.look_at_target_duration == -1.0 || b.look_at_target_duration >= b.move_active_time {
                // Pawn.GetActorEyesViewPoint: Location + EyeHeight (TODO(port): smoothed EyeHeight)
                let head_pos = self.pawn.location + Vec3::new(0.0, 0.0, self.pawn.base_eye_height);
                let wanted = Rotator::from_vector(b.look_at_target_location_v - head_pos).normalize();
                let head = out.normalize();
                let d = (wanted - head).normalize();
                *out = *out + d * (dt / b.look_at_target_interpolation_time).min(1.0);
            } else {
                self.abort_look_at_target(m);
            }
        } else if b.look_at_target_angle {
            if b.look_at_target_duration == -1.0 || b.look_at_target_duration >= b.move_active_time {
                let wanted = b.look_at_target_angle_v;
                let head = out.normalize();
                let d = (wanted - head).normalize();
                *out = *out + d * (dt / b.look_at_target_interpolation_time).min(1.0);
            } else {
                self.abort_look_at_target(m);
            }
        }
        if b.constrain_look || self.pawn.constrain_look {
            let ctrl_delta = (out.normalize() - pawn_rot.normalize()).normalize();
            let (gmin_yaw, gmax_yaw, gmin_pitch) = self.look_constrain_getters(m);
            let (min_yaw, max_yaw) = if b.use_absolute_yaw_constraint {
                (crate::math::norm_axis(gmin_yaw - pawn_rot.yaw), crate::math::norm_axis(gmax_yaw - pawn_rot.yaw))
            } else {
                (gmin_yaw.max(self.pawn.min_look_constraint.yaw), gmax_yaw.min(self.pawn.max_look_constraint.yaw))
            };
            let min_pitch = gmin_pitch.max(self.pawn.min_look_constraint.pitch);
            let max_pitch = b.max_look_constraint.pitch.min(self.pawn.max_look_constraint.pitch);
            let before = *delta;
            constrain_axis(ctrl_delta.yaw, min_yaw, max_yaw, dt / 0.2, &mut delta.yaw);
            constrain_axis(ctrl_delta.pitch, min_pitch, max_pitch, dt / 0.2, &mut delta.pitch);
            if class_of(m) == Class::Climb {
                self.climb_post_constrain_camera(before - *delta, delta);
            }
            *delta = delta.normalize();
        }
        if b.reset_camera_look {
            if b.cancel_reset_camera_look_time > t {
                let mut wanted = pawn_rot;
                wanted.pitch = 0;
                let wanted = wanted.normalize();
                let head = out.normalize();
                let d = (wanted - head).normalize();
                let s = 1.0f32.max((b.cancel_reset_camera_look_time - t) / dt);
                *out = *out + Rotator::new((d.pitch as f32 / s) as i32, (d.yaw as f32 / s) as i32, (d.roll as f32 / s) as i32);
            } else {
                out.pitch = 0;
                self.moves.base_mut(m).reset_camera_look = false;
            }
        }
    }

    // ============================================================================ TdPhysicsMove

    pub fn physics_move_start_move(&mut self, m: Move) {
        self.tdmove_start_move(m);
        let pp = self.moves.base(m).pawn_physics;
        if self.pawn.physics != pp {
            self.set_physics(pp);
        }
        if let Some(s) = self.moves.base(m).controller_state {
            self.goto_controller_state(s);
        }
    }

    pub fn physics_move_stop_move(&mut self, m: Move) {
        self.tdmove_stop_move(m);
        if self.moves.base(m).controller_state.is_some() {
            self.goto_controller_state(CtrlState::PlayerWalking);
        }
    }

    /// UTdPhysicsMove vt73: wall-climb / vault / step-up / grab detection while moving.
    fn physics_move_detect_auto_moves(&mut self, m: Move) {
        // GetWeaponType() == EWT_Heavy (sub_12B0900): no wall run, wall climb, step-up, vault
        // or ledge grab with a heavy gun
        if self.pawn.velocity.is_nearly_zero() || self.heavy_weapon() || self.pawn.dying {
            return;
        }
        let b = self.moves.base(m).clone();
        if b.check_for_wall_climb && self.can_do_move(Move::WallRun) {
            let next = self.moves.wallrun.next_move;
            self.set_move(next, false, false);
            return;
        }
        self.pawn.move_ledge_result = 0;
        if b.move_active_time > b.delay_time_check_auto_moves && (b.check_for_grab || b.check_for_vault_over) {
            let (loc, rot) = (self.pawn.location, self.pawn.rotation);
            let r = self.detect_possible_hand_plant(m, loc, rot, b.hand_plant_check_distance, true);
            self.pawn.move_ledge_result = r;
        }
        let r = self.pawn.move_ledge_result;
        if r <= 0 {
            return;
        }
        if b.check_for_wall_climb && self.can_do_move(Move::WallClimbing) {
            self.set_move(Move::WallClimbing, false, false);
            return;
        }
        if r != 2 {
            return;
        }
        self.pawn.found_ledge = true;
        if b.check_for_vault_over {
            if self.can_do_move(Move::AutoStepUp) {
                self.set_move(Move::AutoStepUp, false, false);
                return;
            }
            if self.can_do_move(Move::VaultOver) {
                self.set_move(Move::VaultOver, false, false);
                return;
            }
        }
        // Moves[IntoGrab] vt74 (native), not the script CanDoMove
        if b.check_for_grab && self.into_grab_native_can_do() {
            self.set_move(Move::IntoGrab, false, false);
        }
    }

    /// sub_11F9970: where will we be in 2 s? Hit Z when that lands on a soft-landing surface.
    fn soft_landing_target(&self) -> Option<f32> {
        let p = &self.pawn;
        let accel = Vec3::new(p.acceleration.x, p.acceleration.y, p.gravity_z());
        let mut feet = p.location;
        feet.z -= p.collision_height;
        let end = feet + p.velocity * 2.0 + accel * 4.0 * 0.5;
        let h = self.world.line_check(end, feet, Vec3::ZERO);
        if !h.hit || h.normal.z <= 0.9 || !h.surface.soft_landing {
            return None;
        }
        Some(h.location.z)
    }

    /// UTdPhysicsMove vt70 (0x1206DF0): soft landing, exit to falling, exit to uncontrolled falling.
    fn physics_move_exit_checks(&mut self, m: Move) {
        let b = self.moves.base(m).clone();
        if b.check_for_soft_landing && b.soft_landing_z_speed_threshold > self.pawn.velocity.z && !self.pawn.is_using_root_rotation {
            if let Some(z) = self.soft_landing_target() {
                if (z - self.pawn.location.z).abs() > self.pawn.collision_height + self.pawn.collision_height {
                    // Cast<TdMove_SoftLanding>(Moves[SoftLanding]) only, no CanDoMove
                    self.set_move(Move::SoftLanding, false, false);
                    return;
                }
            }
        }
        if b.check_exit_to_falling && b.exit_to_falling_z_speed > self.pawn.velocity.z {
            self.set_move(Move::Falling, false, false);
            return;
        }
        if b.check_exit_to_uncontrolled_falling {
            let h = self.pawn.enter_falling_height - self.pawn.location.z;
            if h > self.pawn.falling_uncontrolled_height {
                self.set_move(Move::FallingUncontrolled, false, false);
            }
        }
    }

    // ============================================================================ natives used by moves

    /// UTdMove::MovementTraceForBlocking: does a box sweep from start to end hit anything?
    pub fn movement_trace_for_blocking(&self, end: Vec3, start: Vec3, extent: Vec3) -> bool {
        self.world.line_check(end, start, extent).hit
    }

    pub(crate) fn wants_ledge_check(&self) -> bool {
        self.pawn.avoid_ledges && self.moves.base(self.pawn.movement_state).avoid_ledges
    }

    pub(crate) fn player_pawn_script_tick(&mut self, _dt: f32) {
        // TdPlayerPawn.Tick: roll trigger
        if !self.pawn.roll_trigger_pressed && self.pc.duck {
            if self.pawn.roll_trigger_time + 0.6 < self.time {
                self.pawn.roll_trigger_time = self.time;
            }
            self.pawn.roll_trigger_pressed = true;
        } else if !self.pc.duck {
            self.pawn.roll_trigger_pressed = false;
        }
    }
}

/// TdMove.ConstrainAxis.
pub fn constrain_axis(angle: i32, min: i32, max: i32, speed: f32, delta: &mut i32) {
    let mut target = angle + *delta;
    if angle < min {
        *delta = (((min - angle) as f32 * speed) as i32).max(*delta);
    } else if angle > max {
        *delta = (((max - angle) as f32 * speed) as i32).min(*delta);
    } else {
        if target < min {
            *delta -= target - min;
        } else if target > max {
            *delta -= target - max;
        }
        target = angle + *delta;
        if (target as f32) < min as f32 * 0.6 {
            let damp = ((min - angle) as f32 / (min as f32 * 0.4)).abs();
            *delta = (*delta).max((*delta as f32 * damp) as i32);
        } else if (target as f32) > max as f32 * 0.6 {
            let damp = ((max - angle) as f32 / (max as f32 * 0.4)).abs();
            *delta = (*delta).min((*delta as f32 * damp) as i32);
        }
    }
}

pub(crate) fn _unused(v: Vec3) -> Vec3 {
    v.safe_normal()
}
