//! TdMove_IntoClimb and TdMove_Climb: ladders and pipes (TdLadderVolume), plus the volume's
//! PawnUpdate hook from TdPawn.Tick.

use crate::config::Config;
use crate::ladder::LadderType;
use crate::math::{Rotator, UeVec, Vec3};
use crate::moves::PreciseMode;
use crate::pawn::{Move, MoveAction, MoveActionHint, Slot};
use crate::sim::{Sim, TimerFn};

/// TdMove_IntoClimb.EClimbEnterState.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EnterState {
    #[default]
    AtTop,
    AtBottom,
    Falling,
}

/// TdMove_Climb.EClimbState.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ClimbState {
    #[default]
    Climbing,
    ExitAtTop,
    ExitAtBottom,
}

pub struct IntoClimb {
    pub climb_state: EnterState,
    pub ladder: Option<usize>,
    pub at_bottom_step: bool,
    pub playing_impact: bool,
}

pub struct Climb {
    pub climb_state: ClimbState,
    pub wanted_action: MoveAction,
    pub playing_animation: bool,
    pub ladder: Option<usize>,
    pub climb_anims: [&'static str; 6],
    pub start_turning_angle: i32,
}

impl IntoClimb {
    pub fn new(_cfg: &Config) -> Self {
        IntoClimb { climb_state: EnterState::AtTop, ladder: None, at_bottom_step: false, playing_impact: false }
    }
}

impl Climb {
    pub fn new(cfg: &Config) -> Self {
        Climb {
            climb_state: ClimbState::Climbing,
            wanted_action: MoveAction::None,
            playing_animation: false,
            ladder: None,
            climb_anims: [""; 6],
            start_turning_angle: cfg.i32(&["TdMove_Climb"], "StartTurningAngle", 16384),
        }
    }
}

impl Sim {
    fn ladder(&self, i: Option<usize>) -> &crate::ladder::LadderVolume {
        &self.ladders[i.expect("ladder move without a ladder")]
    }

    /// TdPawn.Tick's volume half: PhysicsVolume changes fire PawnEnteredVolume /
    /// PawnLeavingVolume (bLatent volumes become ActiveMovementVolume), then
    /// ActiveMovementVolume.PawnUpdate.
    pub(crate) fn update_movement_volumes(&mut self) {
        use crate::volumes::VolumeRef;
        let at = self.pawn.location;
        let now = self
            .ladders
            .iter()
            .position(|l| l.encompasses(at))
            .map(VolumeRef::Ladder)
            .or_else(|| self.swings.iter().position(|s| s.vol.encompasses(at)).map(VolumeRef::Swing))
            .or_else(|| self.ziplines.iter().position(|z| z.vol.encompasses(at)).map(VolumeRef::Zipline))
            .or_else(|| self.balances.iter().position(|b| b.encompasses(at)).map(VolumeRef::Balance));
        if now != self.pawn.ladder_physics_volume {
            if let Some(old) = self.pawn.ladder_physics_volume {
                if self.pawn.active_movement_volume == Some(old) {
                    self.pawn.active_movement_volume = None;
                }
                if matches!(old, VolumeRef::Balance(_)) {
                    self.balance_volume_pawn_leaving();
                }
            }
            if now.is_some() {
                self.pawn.active_movement_volume = now;
            }
            self.pawn.ladder_physics_volume = now;
        }
    }

    /// TdLadderVolume.PawnUpdate.
    pub(crate) fn ladder_pawn_update(&mut self, i: usize) {
        if matches!(self.pawn.movement_state, Move::Climb | Move::IntoClimb) {
            return;
        }
        self.moves.into_climb.ladder = Some(i);
        self.moves.climb.ladder = Some(i);
        if self.can_do_move(Move::IntoClimb) {
            self.set_move(Move::IntoClimb, false, false);
            self.pawn.active_movement_volume = None;
        } else {
            self.moves.into_climb.ladder = None;
            self.moves.climb.ladder = None;
        }
    }

    // ------------------------------------------------------------------ IntoClimb

    /// TdMove_IntoClimb.CanDoMove.
    pub fn into_climb_can_do_move(&mut self, m: Move) -> bool {
        if !self.tdmove_can_do_move(m) {
            return false;
        }
        let Some(li) = self.moves.into_climb.ladder else { return false };
        let p = &self.pawn;
        if p.movement_state == Move::Walking && p.move_action_hint != MoveActionHint::Up {
            return false;
        }
        if p.move_action_hint == MoveActionHint::Down {
            return false;
        }
        if matches!(p.movement_state, Move::Climb | Move::Cutscene | Move::IntoClimb) {
            return false;
        }
        let l = &self.ladders[li];
        let angle = p.rotation.vector().dot(l.rotation.vector());
        let mut to_ladder = l.location - p.location;
        to_ladder.z = 0.0;
        let behind = to_ladder.dot(l.rotation.vector()) < 0.0;
        let top = l.ladder_location(l.pawn_ladder_locations.len() as i32 - 1);
        if p.location.z > top.z {
            if angle > -0.85 || l.ladder_type == LadderType::Pipe {
                return false;
            }
        } else if angle < -0.1 || behind {
            return false;
        }
        true
    }

    /// TdMove_IntoClimb.StartMove.
    pub fn into_climb_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        let li = self.moves.into_climb.ladder;
        self.moves.into_climb.playing_impact = false;
        self.pawn.climb_left_hand = false;
        self.pawn.climb_down_fast = false;
        let l = self.ladder(li);
        let step = l.closest_step(self.pawn.location.z).clamp(0, l.last_step());
        self.moves.into_climb.at_bottom_step = step == 0;
        self.reset_camera_look(m, 0.3);
        let l = self.ladder(li);
        let angle = self.pawn.rotation.vector().dot(l.rotation.vector());
        let (wall_normal, move_dir, ladder_type) = (l.wall_normal, l.move_direction, l.ladder_type);
        let top = l.ladder_location(l.pawn_ladder_locations.len() as i32 - 1);
        let step_loc = l.ladder_location(step);
        self.pawn.collide_world = false;
        self.pawn.move_normal = wall_normal;
        if ladder_type != LadderType::Pipe && angle < -0.85 && top.z < self.pawn.location.z {
            self.moves.into_climb.climb_state = EnterState::AtTop;
            let target = top - wall_normal * 93.5 + move_dir * 90.0;
            self.set_precise_location(m, target, PreciseMode::Fly, 500.0);
        } else {
            self.moves.into_climb.climb_state = EnterState::Falling;
            self.into_climb_play_start_animation(m);
            let delta = step_loc - self.pawn.location;
            let speed = delta.size_2d().max(30.0) / 0.15;
            self.set_precise_location(m, step_loc, PreciseMode::Fly, speed);
            self.set_precise_rotation(m, Rotator::from_vector(-wall_normal), 0.15);
            self.pawn.face_rotation_time_left = 0.15;
        }
    }

    /// TdMove_IntoClimb.PlayStartAnimation.
    fn into_climb_play_start_animation(&mut self, m: Move) {
        let li = self.moves.into_climb.ladder;
        let pipe = self.ladder(li).ladder_type == LadderType::Pipe;
        let pick = |p: &'static str, l: &'static str| if pipe { p } else { l };
        let old = self.pawn.old_movement_state;
        let vz = self.pawn.velocity.z;
        let anim = if old == Move::WallRunningLeft {
            Some((pick("PipeClimbHangStartLeft", "LadderClimbHangStartLeft"), 0.15))
        } else if old == Move::WallRunningRight {
            Some((pick("PipeClimbHangStartRight", "LadderClimbHangStartRight"), 0.15))
        } else if old == Move::GrabTransfer {
            // the side the ladder's right is on, relative to the facing
            let l = self.ladder(li);
            let right = l.rotation.vector().cross(Vec3::new(0.0, 0.0, 1.0)).dot(self.pawn.rotation.vector()) > 0.0;
            Some(if right {
                (pick("PipeClimbHangStartRight", "LadderClimbHangStartRight"), 0.15)
            } else {
                (pick("PipeClimbHangStartLeft", "LadderClimbHangStartLeft"), 0.15)
            })
        } else if vz < -800.0 {
            // TakeDamage(1) for the hard grab: no health model here
            Some((pick("PipeClimbHangStartHard", "LadderClimbHangStartHard"), 0.1))
        } else if vz < -200.0 {
            Some((pick("PipeClimbHangStart", "LadderClimbHangStart"), 0.1))
        } else if pipe {
            self.play_move_anim(m, Slot::FullBody, "PipeClimbStart", 1.0, 0.15, 0.25, false, false);
            None
        } else {
            None
        };
        if let Some((name, blend_in)) = anim {
            self.play_move_anim(m, Slot::FullBody, name, 1.0, blend_in, 0.25, false, false);
            self.moves.into_climb.playing_impact = true;
        }
        self.set_animation_movement_state(Move::Climb, 0.15);
    }

    /// TdMove_IntoClimb.StopMove.
    pub fn into_climb_stop_move(&mut self, m: Move) {
        self.set_animation_movement_state(Move::None, 0.2);
        self.pawn.collide_world = true;
        self.moves.into_climb.ladder = None;
        self.physics_move_stop_move(m);
    }

    /// TdMove_IntoClimb.ReachedPreciseLocation.
    pub fn into_climb_reached_precise_location(&mut self, m: Move) {
        self.pawn.velocity = Vec3::ZERO;
        self.pawn.acceleration = Vec3::ZERO;
        self.set_ignore_move_input(-1.0);
        if self.moves.into_climb.climb_state == EnterState::AtTop {
            self.play_move_anim(m, Slot::FullBody, "LadderEnterTop", 1.0, 0.25, 0.1, true, true);
            self.use_root_rotation(true);
            self.use_root_motion(true);
            let wn = self.ladder(self.moves.into_climb.ladder).wall_normal;
            self.set_rotation(Rotator::from_vector(wn));
            self.set_animation_movement_state(Move::Climb, 0.25);
            self.pawn.climb_left_hand = true;
        } else {
            self.into_climb_start_climb_move(m);
        }
    }

    /// TdMove_IntoClimb.StartClimbMove.
    fn into_climb_start_climb_move(&mut self, _m: Move) {
        let li = self.moves.into_climb.ladder;
        let l = self.ladder(li);
        let rot = Rotator::from_vector(-l.wall_normal);
        let loc = l.ladder_location(l.closest_step(self.pawn.location.z));
        self.set_rotation(rot);
        self.set_location(loc);
        let impact = self.moves.into_climb.playing_impact;
        if li.is_some() && self.can_do_move(Move::Climb) {
            self.set_move(Move::Climb, false, false);
            if impact {
                let len = self.anim.slots.get(&Slot::FullBody).map(|s| s.length).unwrap_or(0.0);
                self.set_ignore_move_input((len - 0.5).max(0.1));
            }
        } else {
            self.set_move(Move::Falling, false, false);
        }
    }

    /// TdMove_IntoClimb.OnCustomAnimEnd.
    pub fn into_climb_on_custom_anim_end(&mut self, m: Move) {
        if self.moves.into_climb.climb_state == EnterState::AtTop {
            self.into_climb_start_climb_move(m);
        }
    }

    /// TdMove_IntoClimb.FailedToReachPreciseLocation.
    pub fn into_climb_failed_precise_location(&mut self, m: Move) {
        if matches!(self.moves.into_climb.climb_state, EnterState::AtTop | EnterState::AtBottom) || self.moves.into_climb.at_bottom_step {
            self.set_move(Move::Falling, false, false);
            return;
        }
        let l = self.ladder(self.moves.into_climb.ladder);
        let step = l.closest_step(self.pawn.location.z);
        let loc = l.ladder_location(step);
        self.moves.into_climb.at_bottom_step = step == 0;
        let speed = self.pawn.velocity.size_2d().max(200.0);
        self.set_precise_location(m, loc, PreciseMode::Fall, speed);
    }

    // ------------------------------------------------------------------ Climb

    /// TdMove_Climb.StartMove.
    pub fn climb_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        let li = self.moves.climb.ladder;
        let ladder_type = self.ladder(li).ladder_type;
        self.pawn.ladder_type = ladder_type;
        // InitClimbAnimSeqNames
        self.moves.climb.climb_anims = if ladder_type == LadderType::Pipe {
            ["PipeClimbUpLeftHand", "PipeClimbUpRightHand", "PipeClimbUpFastLeftHand", "PipeClimbUpFastRightHand", "PipeExitTopRightHand", "PipeExitTopLeftHand"]
        } else {
            ["LadderClimbUpLeftHand", "LadderClimbUpRightHand", "LadderClimbUpFastLeftHand", "LadderClimbUpFastRightHand", "LadderExitTopRightHand", "LadderExitTopLeftHand"]
        };
        self.reset_camera_look(m, 0.3);
        self.pawn.velocity = Vec3::ZERO;
        self.pawn.acceleration = Vec3::ZERO;
        let (r, h) = (self.pawn.default_collision_radius, self.pawn.default_collision_height);
        self.set_custom_collision_size(m, r - 5.0, h);
    }

    /// TdMove_Climb.StopMove.
    pub fn climb_stop_move(&mut self, m: Move) {
        self.sound(crate::sound::SoundEvent::LoopStop { slot: crate::sound::LoopSlot::ClimbDownFast, fade_out: 0.1 });
        self.climb_reset();
        self.moves.climb.ladder = None;
        self.use_root_motion(false);
        self.use_root_rotation(false);
        self.pawn.collide_world = true;
        self.pawn.face_rotation_time_left = 0.2;
        self.pawn.leg_rotation = self.pc.rotation.yaw;
        let t = self.time;
        self.moves.base_mut(Move::IntoClimb).last_stop_move_time = t;
        let (r, h) = (self.pawn.default_collision_radius, self.pawn.default_collision_height);
        self.set_custom_collision_size(m, r, h);
        self.physics_move_stop_move(m);
    }

    /// TdMove_Climb.Reset.
    fn climb_reset(&mut self) {
        self.pawn.climb_left_hand = false;
        self.pawn.climb_down_fast = false;
        let c = &mut self.moves.climb;
        c.playing_animation = false;
        c.wanted_action = MoveAction::None;
        c.climb_state = ClimbState::Climbing;
    }

    /// TdMove_Climb.HandleMoveAction.
    pub fn climb_handle_move_action(&mut self, m: Move, a: MoveAction) {
        if a == MoveAction::Crouch && !matches!(self.moves.climb.climb_state, ClimbState::ExitAtTop | ClimbState::ExitAtBottom) {
            self.climb_let_go(m);
            return;
        }
        if a == MoveAction::Turn {
            let mut look = self.pawn.rotation;
            look.yaw += if self.pc.rotation.yaw - self.pawn.rotation.yaw > 0 { 32768 } else { -32768 };
            self.abort_look_at_target(m);
            self.set_look_at_target_angle(m, look.normalize(), 0.2, 1.0);
        }
        self.moves.climb.wanted_action = a;
        if self.moves.climb.playing_animation || self.moves.base(m).use_precise_location {
            self.abort_look_at_target(m);
            return;
        }
        let li = self.moves.climb.ladder;
        let step = self.ladder(li).closest_step(self.pawn.location.z);
        if !self.pawn.climb_down_fast && self.pawn.move_action_hint == MoveActionHint::Down && self.pawn.move_action_max && step > 4 {
            self.pawn.climb_down_fast = true;
            self.reset_camera_look(m, 0.3);
            self.set_ignore_look_input(-1.0);
            // ClimbSoundComponent.FadeIn(0.05, 1.0)
            let cue = if self.pawn.ladder_type == LadderType::Pipe { crate::sound::CLIMB_DOWN_PIPE_FAST_SOUND } else { crate::sound::CLIMB_DOWN_LADDER_FAST_SOUND };
            self.sound(crate::sound::SoundEvent::LoopStart { slot: crate::sound::LoopSlot::ClimbDownFast, sound: crate::sound::LoopSound::Cue(cue.into()), fade_in: 0.05 });
        }
        // GetAimMode(true) is MAM_NoHands unless the view is turned past StartTurningAngle
        if !self.pawn.climb_down_fast && a != MoveAction::None && self.move_aim_mode(m, true) == super::aim::AimMode::NoHands {
            self.climb_handle_climb_action(m);
        }
    }

    /// TdMove_Climb.HasFloorBelow: the trace starts and ends at the same point, so it never hits.
    fn climb_has_floor_below(&self) -> bool {
        let l = self.ladder(self.moves.climb.ladder);
        let p = l.ladder_location(l.closest_step(self.pawn.location.z)) - Vec3::new(0.0, 0.0, 20.0);
        self.movement_trace_for_blocking(p, p, Vec3::ZERO)
    }

    /// TdMove_Climb.StopClimbDownFast (event from the native tick).
    fn climb_stop_climb_down_fast(&mut self, m: Move) {
        if self.moves.base(m).use_precise_location {
            return;
        }
        if self.climb_has_floor_below() {
            self.climb_let_go(m);
            return;
        }
        let l = self.ladder(self.moves.climb.ladder);
        let loc = l.ladder_location(l.closest_step_down(self.pawn.location.z));
        let speed = 100.max(self.pawn.velocity.z as i32) as f32;
        self.set_precise_location(m, loc, PreciseMode::Fly, speed);
        self.sound(crate::sound::SoundEvent::LoopStop { slot: crate::sound::LoopSlot::ClimbDownFast, fade_out: 0.05 });
    }

    /// TdMove_Climb.HandleClimbAction.
    fn climb_handle_climb_action(&mut self, m: Move) {
        let l = self.ladder(self.moves.climb.ladder);
        let step = l.closest_step(self.pawn.location.z);
        let (last, pipe, exit_top) = (l.last_step(), l.ladder_type == LadderType::Pipe, l.can_exit_at_top);
        let left = self.pawn.climb_left_hand;
        match self.moves.climb.wanted_action {
            MoveAction::ClimbUp | MoveAction::ClimbUpLong => {
                if step == last {
                    if exit_top {
                        self.climb_exit_at_top(m, if left { 4 } else { 5 });
                    }
                } else if pipe && last - step > 1 {
                    self.climb_climb(m, if left { 3 } else { 2 }, 1.0, 2);
                } else {
                    self.climb_climb(m, if left { 1 } else { 0 }, 1.0, 1);
                }
            }
            MoveAction::ClimbDown | MoveAction::ClimbDownLong => {
                if step > 1 {
                    if pipe && step > 2 {
                        self.climb_climb(m, if left { 2 } else { 3 }, -1.0, -2);
                    } else {
                        self.climb_climb(m, if left { 0 } else { 1 }, -1.0, -1);
                    }
                } else if self.climb_has_floor_below() {
                    self.climb_let_go(m);
                }
            }
            _ => {}
        }
    }

    /// TdMove_Climb.Climb.
    fn climb_climb(&mut self, m: Move, anim: usize, rate: f32, steps: i32) {
        let l = self.ladder(self.moves.climb.ladder);
        let speed = steps.abs() as f32 * if l.ladder_type == LadderType::Pipe { 64.0 } else { 96.0 };
        let target = l.ladder_location(l.closest_step(self.pawn.location.z) + steps);
        let name = self.moves.climb.climb_anims[anim];
        self.play_move_anim(m, Slot::FullBody, name, rate, 0.1, 0.075, false, false);
        self.moves.climb.playing_animation = true;
        self.set_move_countdown(m, 0.1);
        self.set_precise_location(m, target, PreciseMode::Fly, speed);
    }

    /// TdMove_Climb.ExitAtTop.
    fn climb_exit_at_top(&mut self, m: Move, anim: usize) {
        let name = self.moves.climb.climb_anims[anim];
        self.play_move_anim(m, Slot::FullBody, name, 1.0, 0.1, 0.1, true, false);
        self.moves.climb.playing_animation = true;
        self.reset_camera_look(m, 0.1);
        self.set_ignore_look_input(-1.0);
        self.set_timer(TimerFn::StopIgnoreLookInput, 0.9, false);
        self.use_root_motion(true);
        self.moves.climb.climb_state = ClimbState::ExitAtTop;
        self.pawn.collide_world = false;
        self.set_animation_movement_state(Move::Walking, 0.5);
    }

    /// TdMove_Climb.ReachedPreciseLocation.
    pub fn climb_reached_precise_location(&mut self, _m: Move) {
        self.pawn.velocity = Vec3::ZERO;
        self.pawn.acceleration = Vec3::ZERO;
        self.use_root_motion(false);
        self.use_root_rotation(false);
        self.pawn.collide_world = true;
        let wn = self.ladder(self.moves.climb.ladder).wall_normal;
        self.set_rotation(Rotator::from_vector(-wn));
        self.moves.climb.playing_animation = false;
        self.pawn.climb_down_fast = false;
        self.stop_ignore_look_input();
    }

    /// TdMove_Climb.OnCeaseRelevantRootMotion (the exit animation is over).
    pub fn climb_on_cease_relevant_root_motion(&mut self, _m: Move) {
        if matches!(self.moves.climb.climb_state, ClimbState::ExitAtTop | ClimbState::ExitAtBottom) {
            if self.pawn.move_action_hint == MoveActionHint::Up {
                self.pawn.velocity = self.pawn.rotation.vector() * 300.0;
                self.pawn.acceleration = self.pawn.velocity.safe_normal();
                self.pc.acceleration_time = 0.2;
            }
            self.set_move(Move::Walking, false, false);
        }
    }

    /// TdMove_Climb.LetGo.
    fn climb_let_go(&mut self, m: Move) {
        let mut ext = self.pawn.extent();
        ext.z = 8.0;
        let mut start = self.pawn.location;
        start.z -= self.pawn.collision_height;
        ext.z += 8.0;
        let mut end = start;
        end.z -= 32.0;
        if self.movement_trace(end, start, ext).is_some() {
            self.set_move(Move::Walking, false, false);
        } else {
            self.set_move(Move::Falling, false, false);
        }
        self.play_move_anim(m, Slot::FullBody, "PipeExitBottom", 1.0, 0.1, 0.4, false, false);
    }

    /// TdMove_Climb.OnTimer: hands alternate.
    pub fn climb_on_timer(&mut self, _m: Move) {
        self.pawn.climb_left_hand = !self.pawn.climb_left_hand;
    }

    /// UTdMove_Climb vt70 (0x120ABE0), before the TdPhysicsMove part.
    pub fn climb_tick(&mut self, m: Move, dt: f32) {
        if self.pawn.climb_down_fast {
            self.pawn.velocity.z += self.pawn.gravity_z() * dt * 0.5;
            let l = self.ladder(self.moves.climb.ladder);
            let next = l.closest_step(self.pawn.velocity.z * dt + self.pawn.location.z);
            if self.pawn.move_action_hint != MoveActionHint::Down || !self.pawn.move_action_max || next < 2 {
                self.climb_stop_climb_down_fast(m);
            }
        } else if !self.moves.climb.playing_animation {
            self.pawn.velocity.z = 0.0;
        }
    }

    /// TdMove_Climb.GetMinLookConstrainPitch / GetMin/MaxLookConstrainYaw.
    pub fn climb_look_constrain_getters(&self, m: Move) -> (i32, i32, i32) {
        let b = self.moves.base(m);
        let sta = self.moves.climb.start_turning_angle;
        let dy = (self.pc.rotation - self.pawn.rotation).normalize().yaw.abs();
        let pitch = if dy > sta { -11000 } else { b.min_look_constraint.pitch };
        if self.moves.climb.playing_animation {
            (-sta, sta, pitch)
        } else {
            (b.min_look_constraint.yaw, b.max_look_constraint.yaw, pitch)
        }
    }

    /// TdMove_Climb.PostConstrainCamera: pitch pushed back by the constraint turns the view.
    pub fn climb_post_constrain_camera(&self, amount: Rotator, delta: &mut Rotator) {
        if ((self.pc.rotation - self.pawn.rotation).normalize().yaw as f32).abs() > 12000.0 {
            return;
        }
        if amount.pitch < 0 {
            if (self.pc.rotation.yaw - self.pawn.rotation.yaw) as f32 > 0.0 {
                if delta.yaw >= 0 {
                    delta.yaw -= (amount.pitch as f32 * 0.5) as i32;
                }
            } else if delta.yaw <= 0 {
                delta.yaw += (amount.pitch as f32 * 0.5) as i32;
            }
        }
    }

    /// TdMove_Climb.Landed.
    pub fn climb_landed(&mut self, m: Move) {
        self.climb_let_go(m);
    }
}
