//! TdPlayerController + TdPlayerInput: input axes -> pawn acceleration, move actions and view
//! rotation, as in their PlayerWalking / PlayerGrabbing / PlayerWallWalking states.

use crate::config::Config;
use crate::math::{Rotator, UeVec, Vec3, norm_axis};
use crate::pawn::{Move, MoveAction, Physics, Slot};
use crate::sim::{InputFrame, Sim, TimerFn};

/// TdPlayerController states that change PlayerMove.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CtrlState {
    #[default]
    PlayerWalking,
    PlayerGrabbing,
    PlayerWallWalking,
    PlayerLedgeWalking,
    PlayerBalanceWalk,
    PlayerDying,
}

#[derive(Clone, Debug)]
pub struct Controller {
    /// Controller.Rotation (the view).
    pub rotation: Rotator,
    pub state: CtrlState,
    // PlayerInput
    pub a_base_y: f32,
    pub a_forward: f32,
    pub a_strafe: f32,
    pub a_turn: f32,
    pub a_look_up: f32,
    pub input_size: f32,
    pub mouse_sensitivity: f32,
    pub walk_button_multiplier: f32,
    pub walk_button_pressed: bool,
    pub fov: f32,
    /// TdPlayerController.UpdateRotation's RotSpeedMod (see update_rotation).
    pub pitch_turn_slowdown: bool,
    /// PlayerController DefaultFOV / DesiredFOV / FOVZoomRate / FOVZoomDelay (AdjustFOV).
    pub default_fov: f32,
    pub desired_fov: f32,
    pub fov_zoom_rate: f32,
    pub fov_zoom_delay: f32,
    // PlayerController
    pub pressed_jump: bool,
    pub released_jump: bool,
    pub jump_held: bool,
    pub duck: bool,
    pub ignore_move_input: bool,
    pub ignore_look_input: bool,
    // TdPlayerController (PlayerWalking)
    pub is_stopping: bool,
    pub is_walking_anim: bool,
    pub stopping_velocity: f32,
    pub acceleration_time: f32,
    pub input_max_sprint_radius_limit: f32,
    pub input_max_sprint_height_limit: f32,
    pub walk_cycle_part1: f32,
    pub walk_cycle_part2: f32,
    pub stop_anim_blend_in: f32,
    pub stop_anim_blend_out: f32,
    pub right_stick_passed_dead_zone: bool,
    pub left_stick_passed_dead_zone: bool,
    pub prev_jump: bool,
    pub prev_attack: bool,
    /// Melee lock-on (TdPlayerController.TargetingPawn, TargetingPawnInterp).
    pub targeting_pawn: Option<usize>,
    /// TdPlayerController.TargetPawn (SnatchAttempt's GetHumanTarget).
    pub target_pawn: Option<usize>,
    pub prev_switch_weapon: bool,
    pub targeting_pawn_interp: f32,
    pub targeting_cutoff_angle: f32,
    pub close_combat_max_angle: f32,
    /// The enemies' AITemplate.SoftLockStrength.
    pub soft_lock_strength: f32,
    /// TdMove.UpdateMeleeAutoLockOn swings the view onto the melee target. Off by the user's
    /// request (like pitch_turn_slowdown).
    pub melee_lock_on: bool,
}

impl Controller {
    pub fn new(cfg: &Config) -> Self {
        let pc = &["TdPlayerController"];
        let inp = |k: &str, d: f32| cfg.raw_in("TdGame.TdPlayerInput", k).or_else(|| cfg.raw_in("Engine.PlayerInput", k)).map(crate::config::atof).unwrap_or(d);
        Controller {
            rotation: Rotator::ZERO,
            state: CtrlState::PlayerWalking,
            a_base_y: 0.0,
            a_forward: 0.0,
            a_strafe: 0.0,
            a_turn: 0.0,
            a_look_up: 0.0,
            input_size: 0.0,
            mouse_sensitivity: inp("MouseSensitivity", 18.0),
            walk_button_multiplier: inp("WalkButtonMultiplier", 0.3),
            walk_button_pressed: false,
            fov: 90.0,
            pitch_turn_slowdown: false,
            default_fov: 90.0,
            desired_fov: 90.0,
            fov_zoom_rate: 0.0,
            fov_zoom_delay: 0.0,
            pressed_jump: false,
            released_jump: false,
            jump_held: false,
            duck: false,
            ignore_move_input: false,
            ignore_look_input: false,
            is_stopping: false,
            is_walking_anim: false,
            stopping_velocity: 0.0,
            acceleration_time: 0.0,
            input_max_sprint_radius_limit: cfg.f32(pc, "InputMaxSprintRaduisLimit", 0.7),
            input_max_sprint_height_limit: cfg.f32(pc, "InputMaxSprintHeightLimit", 0.7),
            walk_cycle_part1: cfg.f32(pc, "WalkCyclePart1", 0.25),
            walk_cycle_part2: cfg.f32(pc, "WalkCyclePart2", 0.75),
            stop_anim_blend_in: cfg.f32(pc, "StopAnimBlendIn", 0.2),
            stop_anim_blend_out: cfg.f32(pc, "StopAnimBlendOut", 0.2),
            right_stick_passed_dead_zone: true,
            left_stick_passed_dead_zone: true,
            prev_jump: false,
            prev_attack: false,
            targeting_pawn: None,
            target_pawn: None,
            prev_switch_weapon: false,
            targeting_pawn_interp: 0.0,
            targeting_cutoff_angle: cfg.f32(pc, "TargetingCutoffAngle", 3900.0),
            close_combat_max_angle: cfg.f32(pc, "CloseCombatMaxAngle", 0.7),
            soft_lock_strength: cfg.f32(&["AITemplate_Default"], "SoftLockStrength", 1.2),
            melee_lock_on: false,
        }
    }
}

impl Sim {
    /// TdPlayerController.PlayerTick -> PlayerInput.PlayerInput + state PlayerMove.
    pub(crate) fn player_tick(&mut self, dt: f32, input: InputFrame) {
        // exec functions from key events (Jump/StopJump, Crouch/StopCrouch, LookBehind, WalkMod)
        if input.jump && !self.pc.prev_jump {
            self.pc.pressed_jump = true;
        }
        if !input.jump && self.pc.prev_jump {
            self.pc.released_jump = true;
        }
        self.pc.prev_jump = input.jump;
        self.pc.jump_held = input.jump;
        self.pc.duck = input.crouch;
        self.pc.walk_button_pressed = input.walk;
        if input.turn {
            self.look_behind();
        }
        // AttackPress: unarmed, it's the melee action
        // AttackPress (gated by bIgnoreButtonInput, not the move input): melee unarmed,
        // StartFire with a gun
        if input.attack && !self.pc.prev_attack {
            if self.has_weapon() {
                self.player_start_fire();
            } else if self.weapon.is_none() {
                self.handle_move_action(MoveAction::Melee);
            }
        }
        self.pc.prev_attack = input.attack;
        // SwitchWeapon (RMB): drop / snatch / pick up
        if input.switch_weapon && !self.pc.prev_switch_weapon {
            self.switch_weapon_press();
        }
        self.pc.prev_switch_weapon = input.switch_weapon;
        self.tick_player_weapon(dt, input.attack);

        self.player_input(dt, &input);
        match self.pc.state {
            CtrlState::PlayerWalking | CtrlState::PlayerLedgeWalking => self.player_move_walking(dt),
            CtrlState::PlayerBalanceWalk => self.player_move_balance_walk(dt),
            CtrlState::PlayerGrabbing => self.player_move_grabbing(dt),
            CtrlState::PlayerWallWalking => self.player_move_wall_walking(dt),
            CtrlState::PlayerDying => self.player_move_dying(dt),
        }
        // PlayerController.PlayerTick
        self.adjust_fov(dt);
    }

    /// TdPlayerInput.PlayerInput (keyboard + mouse path).
    fn player_input(&mut self, _dt: f32, input: &InputFrame) {
        self.pc.a_base_y = input.forward;
        self.pc.a_strafe = input.strafe;
        // PreProcessInput -> SetInputHint, every frame
        self.set_input_hint();
        if self.pc.state == CtrlState::PlayerWallWalking {
            self.wall_walking_input_hint();
        }
        let pc = &mut self.pc;
        // mouse: aMouseX *= MouseSensitivity; aTurn += aMouseX (smoothing omitted: it is a
        // per-frame average that only matters at very low frame rates)
        pc.a_turn = input.mouse_x * pc.mouse_sensitivity;
        pc.a_look_up = input.mouse_y * pc.mouse_sensitivity;
        pc.a_forward = pc.a_base_y;
        pc.input_size = (pc.a_forward * pc.a_forward + pc.a_strafe * pc.a_strafe).sqrt();
        if pc.input_size > 1.0 {
            pc.a_forward /= pc.input_size;
            pc.a_strafe /= pc.input_size;
        }
        let fov_scale = pc.fov * 0.01111;
        pc.a_look_up *= fov_scale * 1.78;
        pc.a_turn *= fov_scale;
        let mut handicap = 1.0; // GetMobilityMultiplier()
        if pc.walk_button_pressed {
            handicap *= pc.walk_button_multiplier;
        }
        if pc.ignore_move_input {
            pc.a_forward = 0.0;
            pc.a_strafe = 0.0;
        } else {
            pc.a_forward *= handicap;
            pc.a_strafe *= handicap;
        }
        if pc.ignore_look_input {
            pc.a_turn = 0.0;
            pc.a_look_up = 0.0;
        } else {
            pc.a_turn *= handicap;
            pc.a_look_up *= handicap;
        }
    }

    /// state PlayerWalking: PlayerMove.
    fn player_move_walking(&mut self, dt: f32) {
        let (x, y, _) = self.pawn.rotation.axes();
        self.check_trigger_stop_anim();
        let mut new_accel = Vec3::ZERO;
        if self.pc.is_stopping {
            if self.pawn.movement_state == Move::Walking {
                let sv = self.pc.stopping_velocity;
                self.pawn.velocity = self.pawn.velocity.safe_normal() * sv;
                new_accel = self.pawn.velocity.safe_normal() * sv;
            }
        } else {
            let (af, as_) = (self.pc.a_forward, self.pc.a_strafe);
            let walk_dir = (x * af + y * as_).safe_normal();
            let turn = self.pc.a_turn as i32;
            if self.pawn.physics == Physics::Falling {
                self.pc.is_stopping = false;
                new_accel = self.get_sprint_acceleration(af, as_, turn, dt);
            } else if walk_dir.size_2d() > 0.0 {
                self.pc.acceleration_time += dt;
                self.pc.stopping_velocity = 200.0;
                let sprint = af > self.pc.input_max_sprint_height_limit
                    && self.pc.input_size > self.pc.input_max_sprint_radius_limit
                    && self.pawn.velocity.dot(walk_dir) > 0.0;
                if sprint {
                    new_accel = self.get_sprint_acceleration(af, as_, turn, dt);
                } else if walk_dir.size_2d() > 0.5 {
                    new_accel = self.get_walk_acceleration(af, as_, turn, dt);
                } else {
                    self.pc.a_forward = 0.1 * sign(af);
                    self.pc.a_strafe = 0.1 * sign(as_);
                    let (af, as_) = (self.pc.a_forward, self.pc.a_strafe);
                    new_accel = self.get_walk_acceleration(af, as_, turn, dt);
                }
            } else {
                new_accel = self.get_walk_acceleration(af, as_, turn, dt);
                if self.pc.acceleration_time > 0.0 && self.pc.acceleration_time < 0.15 {
                    self.pc.is_stopping = true;
                    self.set_timer(TimerFn::PlayStop, 0.25, false);
                    self.pc.stopping_velocity = 35.0;
                    self.pawn.velocity = self.pawn.velocity.safe_normal() * 35.0;
                }
                self.pc.acceleration_time = 0.0;
            }
        }
        let old_rot = self.pc.rotation;
        self.update_rotation(dt);
        let _ = old_rot;
        // ProcessMove
        self.pawn.acceleration = new_accel;
        self.check_crouch();
        self.check_jump_pressed();
        self.check_jump_released();
        self.pc.released_jump = false;
        self.pc.pressed_jump = false;
    }

    /// state PlayerBalanceWalk: PlayerMove. Forward walks along the facing; strafe goes in
    /// sideways (unit length), where TdMove_Balance reads it as the counter-lean.
    fn player_move_balance_walk(&mut self, dt: f32) {
        let fwd = self.pawn.rotation.vector();
        let turn = self.pc.a_turn as i32;
        let (af, balance) = (self.pc.a_forward, self.pc.a_strafe);
        let walk = self.get_walk_acceleration(af, 0.0, turn, dt);
        let new_accel = fwd * walk.dot(fwd) + fwd.cross(Vec3::new(0.0, 0.0, 1.0)) * balance;
        self.update_rotation(dt);
        // PlayerWalking.ProcessMove
        self.pawn.acceleration = new_accel;
        self.check_crouch();
        self.check_jump_pressed();
        self.check_jump_released();
        self.pc.released_jump = false;
        self.pc.pressed_jump = false;
    }

    /// state PlayerGrabbing: PlayerMove + ProcessMove (stick deflection becomes shimmy / climb
    /// actions; the pawn gets no acceleration).
    fn player_move_grabbing(&mut self, dt: f32) {
        use crate::pawn::MoveActionHint as H;
        let ax = self.pc.a_forward * 10.0;
        let ay = self.pc.a_strafe * 10.0;
        self.update_rotation(dt);
        if !(self.pawn.force_rm_velocity || self.pawn.is_using_root_motion) {
            let hint = self.pawn.move_action_hint;
            let a = if ay > 8.0 {
                MoveAction::ShimmyRightLong
            } else if ay < -8.0 {
                MoveAction::ShimmyLeftLong
            } else if ay > 3.0 {
                MoveAction::ShimmyRight
            } else if ay < -3.0 {
                MoveAction::ShimmyLeft
            } else if ax > 8.0 && hint == H::Up {
                MoveAction::ClimbUpLong
            } else if ax < -8.0 && hint == H::Down {
                MoveAction::ClimbDownLong
            } else if ax > 3.0 {
                MoveAction::ClimbUp
            } else if ax < -3.0 {
                MoveAction::ClimbDown
            } else {
                MoveAction::None
            };
            self.handle_move_action(a);
        }
        self.pawn.acceleration = Vec3::ZERO;
        self.check_crouch();
        self.check_jump_pressed();
        self.check_jump_released();
        self.pc.released_jump = false;
        self.pc.pressed_jump = false;
    }

    /// state PlayerDying: PlayerMove (global ProcessMove sets the raw local accel).
    fn player_move_dying(&mut self, dt: f32) {
        let accel = Vec3::new(self.pc.a_forward * 10.0, self.pc.a_strafe * 10.0, 0.0);
        self.update_rotation(dt);
        // ShouldDelayJump is false for the player
        self.pc.released_jump = false;
        self.pawn.acceleration = accel;
        self.pc.released_jump = false;
        self.pc.pressed_jump = false;
    }

    /// state PlayerWallWalking: PlayerMove.
    fn player_move_wall_walking(&mut self, dt: f32) {
        self.update_rotation_wall_walking(dt);
        self.pawn.acceleration = Vec3::ZERO;
        self.check_jump_pressed();
        self.check_jump_released();
        self.check_crouch();
        self.pc.released_jump = false;
        self.pc.pressed_jump = false;
    }

    /// PlayerWallWalking.SetInputHint (after the global one).
    fn wall_walking_input_hint(&mut self) {
        use crate::pawn::MoveActionHint as H;
        let thr = 0.8; // WallRunningDodgeJumpThreshold / WallClimbingDodgeJumpThreshold
        let s = self.pc.a_strafe;
        if s > thr {
            self.pawn.move_action_hint = H::Right;
            self.pawn.move_action_max = s > 0.96;
        } else if s < -thr {
            self.pawn.move_action_hint = H::Left;
            self.pawn.move_action_max = s < -0.96;
        } else if self.pawn.move_action_hint != H::Up {
            self.pawn.move_action_hint = H::None;
            self.pawn.move_action_max = false;
        }
    }

    /// PlayerWallWalking.UpdateRotation: no pitch slow-down, the pawn keeps its own facing.
    fn update_rotation_wall_walking(&mut self, dt: f32) {
        let mut view = self.pc.rotation;
        let mut delta = Rotator::new(self.pc.a_look_up as i32, self.pc.a_turn as i32, 0);
        let ms = self.pawn.movement_state;
        self.move_update_view_rotation(ms, &mut view, dt, &mut delta);
        view = view + delta;
        view = limit_view_rotation(view, -16384, 16383);
        self.pc.rotation = view;
        let pr = self.pawn.rotation;
        self.face_rotation(pr, dt);
    }

    /// TdPlayerController.SetInputHint (keyboard: sticks always past the dead zone).
    fn set_input_hint(&mut self) {
        use crate::pawn::MoveActionHint as H;
        let (y, s) = (self.pc.a_base_y, self.pc.a_strafe);
        let mut hint = H::None;
        let mut max = false;
        if y > 0.8 {
            hint = H::Up;
            max = y > 0.96;
        } else if y < -0.8 {
            hint = H::Down;
            max = y < -0.96;
        }
        if s > 0.3 {
            hint = H::Right;
            max = s > 0.96;
        } else if s < -0.3 {
            hint = H::Left;
            max = s < -0.96;
        }
        self.pawn.move_action_hint = hint;
        self.pawn.move_action_max = max;
    }

    /// TdPlayerController.UpdateRotation.
    pub(crate) fn update_rotation(&mut self, dt: f32) {
        let mut view = self.pc.rotation;
        let rel_pitch = (self.pc.rotation - self.pawn.rotation).normalize().pitch;
        // The original slows yaw to 40% once the view is 63 degrees off the pawn's pitch
        // (RotSpeedMod, meant for the right stick but applied to the mouse too); off unless
        // `pitch_turn_slowdown` is set, since it makes mouse look feel sluggish looking down.
        let rot_speed_mod = if self.pc.pitch_turn_slowdown {
            (1.0 - ((rel_pitch as f32 / 16384.0).abs() + 0.3).min(1.0).trunc()).max(0.4)
        } else {
            1.0
        };
        let mut delta = Rotator::ZERO;
        if self.pc.right_stick_passed_dead_zone {
            delta.yaw = (self.pc.a_turn * rot_speed_mod) as i32;
            delta.pitch = self.pc.a_look_up as i32;
        }
        let ms = self.pawn.movement_state;
        self.move_update_view_rotation(ms, &mut view, dt, &mut delta);
        // ProcessViewRotation -> Pawn.ProcessViewRotation
        view = view + delta;
        view = limit_view_rotation(view, -16384, 16383);
        view.roll = 0;
        self.pc.rotation = view;
        let mut new_rot = view;
        new_rot.roll = self.pc.rotation.roll;
        self.face_rotation(new_rot, dt);
    }

    /// TdPawn.FaceRotation.
    pub(crate) fn face_rotation(&mut self, mut new_rot: Rotator, dt: f32) {
        if self.pawn.is_using_root_rotation {
            return;
        }
        if self.moves.base(self.pawn.movement_state).disable_face_rotation {
            return;
        }
        let p = &mut self.pawn;
        if p.face_rotation_time_left > 0.0 {
            let mut r = p.rotation;
            let d = (new_rot - r).normalize();
            r.yaw += (d.yaw as f32 * (dt / p.face_rotation_time_left).min(1.0)) as i32;
            p.rotation = r.normalize();
            p.face_rotation_time_left -= dt;
            return;
        }
        if p.physics == Physics::Walking {
            new_rot.pitch = 0;
            new_rot.roll = 0;
        }
        let mut pr = p.rotation;
        new_rot = new_rot.normalize();
        pr.yaw = new_rot.yaw;
        pr.pitch = 0;
        pr.roll = 0;
        p.rotation = pr;
    }

    /// TdPlayerController.CheckCrouch.
    fn check_crouch(&mut self) {
        let crouching = self.is_in_move(Move::Crouch) || self.is_in_move(Move::Slide);
        if crouching && (!self.pc.duck || self.pawn.physics == Physics::Falling) {
            self.handle_move_action(MoveAction::StopCrouch);
        } else if self.pc.duck && !self.is_in_move(Move::Slide) {
            self.handle_move_action(MoveAction::Crouch);
        }
    }

    fn check_jump_pressed(&mut self) {
        if self.pc.pressed_jump {
            self.handle_move_action(MoveAction::Jump);
        }
    }

    fn check_jump_released(&mut self) {
        if self.pc.released_jump {
            self.handle_move_action(MoveAction::StopJump);
            self.pc.pressed_jump = false;
        }
    }

    /// TdPlayerController.LookBehind.
    fn look_behind(&mut self) {
        let g = self.moves.base(self.pawn.movement_state).movement_group;
        if g >= 3 {
            return;
        }
        self.handle_move_action(MoveAction::Turn);
    }

    /// PlayerWalking.CheckTriggerStopAnim.
    fn check_trigger_stop_anim(&mut self) {
        if self.pawn.movement_state != Move::Walking {
            return;
        }
        if self.pc.a_forward.abs() <= 0.01 && self.pc.a_strafe.abs() <= 0.01 {
            if self.pc.is_walking_anim {
                // where in the walk cycle are we: decides the stopping foot
                let pos = self.anim.locomotion_phase;
                if pos >= self.pc.walk_cycle_part2 || pos <= self.pc.walk_cycle_part1 {
                    self.play_stop_anim(true);
                } else {
                    self.play_stop_anim(false);
                }
                self.pc.is_stopping = false;
                self.pc.is_walking_anim = false;
            }
        } else if !self.pc.is_walking_anim
            && !self.pc.is_stopping
            && self.pawn.current_walking_state >= crate::pawn::WalkingState::Walk
        {
            self.clear_timer(TimerFn::PlayStopLeft);
            self.clear_timer(TimerFn::PlayStopRight);
            let bo = self.pc.stop_anim_blend_out;
            self.anim.stop(Slot::FullBodyDir, bo);
            self.pc.is_walking_anim = true;
            self.pc.is_stopping = false;
        }
    }

    /// PlayerWalking.PlayStopLeft / PlayStopRight.
    pub(crate) fn play_stop_anim(&mut self, left: bool) {
        if self.pawn.movement_state == Move::Walking {
            let name = if left { "walktostandpassleft" } else { "walktostandpassright" };
            let (bi, bo) = (self.pc.stop_anim_blend_in, self.pc.stop_anim_blend_out);
            self.anim.play(Slot::LowerBody, name, 1.0, bi, bo, false, false, false);
        }
        self.pc.is_stopping = false;
    }

    /// TdPlayerController.GotoState (moves set ControllerState in StartMove).
    pub fn goto_controller_state(&mut self, s: CtrlState) {
        if self.pc.state != s {
            self.pc.state = s;
            if s == CtrlState::PlayerWalking {
                // BeginState
                self.pc.pressed_jump = false;
                self.pc.is_stopping = false;
                if !matches!(self.pawn.physics, Physics::Falling | Physics::RigidBody | Physics::Flying | Physics::WallClimbing | Physics::WallRunning) {
                    self.set_physics(Physics::Walking);
                }
            }
        }
    }
}

fn sign(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// PlayerController.LimitViewRotation.
pub fn limit_view_rotation(mut r: Rotator, min: i32, max: i32) -> Rotator {
    r.pitch &= 65535;
    if r.pitch > max && r.pitch < 65535 + min {
        r.pitch = if r.pitch < 32768 { max } else { 65535 + min };
    }
    r.pitch = norm_axis(r.pitch);
    r
}
