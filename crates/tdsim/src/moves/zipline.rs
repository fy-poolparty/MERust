//! TdMove_IntoZipLine and TdMove_ZipLine (TdZiplineVolume): grab the cable from a jump, then
//! slide down its spline (UTdMove_ZipLine vt70, 0x1209400) until the cable runs out or a wall
//! stops you.

use crate::config::Config;
use crate::math::{Rotator, UeVec, Vec3};
use crate::moves::PreciseMode;
use crate::pawn::{Move, Physics, Slot};
use crate::sim::Sim;
use crate::sound::{LoopSlot, LoopSound, SoundEvent};

/// TdMove_ZipLine.EZipLineStatus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ZipLineStatus {
    #[default]
    Moving,
    CloseToEnd,
    Impact,
}

pub struct IntoZipLine {
    pub zipline: Option<usize>,
    pub hang_offset: Vec3,
    pub z_velocity_fall_limit: f32,
    pub enter_param: f32,
    pub saved_initial_2d_velocity: Vec3,
    pub last_volume: Option<usize>,
    pub same_zipline_redo_move_time: f32,
}

pub struct ZipLine {
    pub zipline: Option<usize>,
    pub hang_offset: Vec3,
    pub min_zip_velocity: f32,
    pub min_zip_acceleration: f32,
    pub current_param_on_curve: f32,
    pub zip_fade_in_time: f32,
    pub zip_fade_out_time: f32,
    pub current_look_at_point: Vec3,
    pub look_assist: bool,
    pub status: ZipLineStatus,
    /// The ZipLine idle's NormalizedStartPosition (0.5 when entered from the left).
    pub idle_start_position: f32,
}

impl IntoZipLine {
    pub fn new(cfg: &Config) -> Self {
        let c = &["TdMove_IntoZipLine"];
        IntoZipLine {
            zipline: None,
            hang_offset: Vec3::new(0.0, 0.0, -90.0),
            z_velocity_fall_limit: cfg.f32(c, "ZVelocityFallLimit", -600.0),
            enter_param: 0.0,
            saved_initial_2d_velocity: Vec3::ZERO,
            last_volume: None,
            same_zipline_redo_move_time: 3.0,
        }
    }
}

impl ZipLine {
    pub fn new(cfg: &Config) -> Self {
        let c = &["TdMove_ZipLine"];
        ZipLine {
            zipline: None,
            hang_offset: Vec3::new(0.0, 0.0, -90.0),
            min_zip_velocity: cfg.f32(c, "MinZipVelocity", 300.0),
            min_zip_acceleration: cfg.f32(c, "MinZipAcceleration", 400.0),
            current_param_on_curve: 0.0,
            zip_fade_in_time: cfg.f32(c, "ZipFadeInTime", 0.1),
            zip_fade_out_time: cfg.f32(c, "ZipFadeOutTime", 0.5),
            current_look_at_point: Vec3::ZERO,
            look_assist: false,
            status: ZipLineStatus::Moving,
            idle_start_position: 0.0,
        }
    }
}

/// TdMove_ZipLine.ZippingSound.
pub const ZIPPING_SOUND: &str = "A_Kits.ZipLine.ZipLine";

impl Sim {
    fn zipline(&self, i: Option<usize>) -> &crate::volumes::ZiplineVolume {
        &self.ziplines[i.expect("zipline move without a zipline")]
    }

    /// TdZiplineVolume.PawnUpdate.
    pub(crate) fn zipline_volume_pawn_update(&mut self, i: usize) {
        if matches!(self.pawn.movement_state, Move::ZipLine | Move::IntoZipLine) {
            return;
        }
        self.moves.into_zipline.zipline = Some(i);
        if self.can_do_move(Move::IntoZipLine) {
            self.set_move(Move::IntoZipLine, false, false);
            self.pawn.active_movement_volume = None;
        } else {
            self.moves.into_zipline.zipline = None;
        }
    }

    // ------------------------------------------------------------------ IntoZipLine

    /// TdMove_IntoZipLine.CanDoMove.
    pub fn into_zipline_can_do_move(&mut self, m: Move) -> bool {
        let Some(zi) = self.moves.into_zipline.zipline else { return false };
        let p = &self.pawn;
        if matches!(p.movement_state, Move::ZipLine | Move::IntoZipLine) {
            return false;
        }
        let z = &self.ziplines[zi];
        if z.vol.move_direction.dot(p.rotation.vector()) <= 0.0 {
            return false;
        }
        if matches!(p.movement_state, Move::Walking | Move::Crouch | Move::Slide) {
            return false;
        }
        let last = *z.vol.spline_locations.last().unwrap_or(&z.vol.end);
        if (p.location - last).size_2d() < z.landing_strip {
            return false;
        }
        let since = self.time - self.moves.base(Move::ZipLine).last_stop_move_time;
        if self.moves.into_zipline.same_zipline_redo_move_time > since && self.moves.into_zipline.last_volume == Some(zi) {
            return false;
        }
        self.tdmove_can_do_move(m)
    }

    /// TdMove_IntoZipLine.StartMove.
    pub fn into_zipline_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        let zi = self.moves.into_zipline.zipline;
        let z = self.zipline(zi).clone();
        let (mut dest, _) = z.vol.find_closest_point_on_dspline(self.pawn.location, 0);
        let (d2, param) = z.vol.find_closest_point_on_dspline(dest + z.vol.move_direction * 100.0, 0);
        dest = d2;
        self.moves.into_zipline.enter_param = param;
        dest.z += self.moves.into_zipline.hang_offset.z;
        self.moves.into_zipline.last_volume = zi;
        let speed = self.pawn.velocity.size_2d().max(400.0);
        let n = (z.vol.spline_locations.len() as f32 - 1.0).max(1.0);
        let mut wanted = Rotator::from_vector(z.vol.slope_on_spline(param / n));
        wanted.roll = 0;
        wanted.pitch = 0;
        let time = (dest - self.pawn.location).length() / speed;
        self.set_precise_rotation(m, wanted, time);
        self.moves.into_zipline.saved_initial_2d_velocity = Vec3::new(self.pawn.velocity.x, self.pawn.velocity.y, 0.0);
        let rate = (0.3 / time).clamp(0.2, 2.0);
        self.play_move_anim(m, Slot::FullBody, "ZiplineStart", rate, 0.2, 0.4, false, false);
        self.set_animation_movement_state(Move::IntoZipLine, 0.2);
        let mode = if self.pawn.velocity.z < 0.0 { PreciseMode::Fly } else { PreciseMode::Jump };
        self.set_precise_location(m, dest, mode, speed);
        self.set_ignore_move_input(-1.0);
    }

    /// TdMove_IntoZipLine.StopMove.
    pub fn into_zipline_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.moves.into_zipline.zipline = None;
        self.set_animation_movement_state(Move::None, 0.0);
    }

    /// TdMove_IntoZipLine.ReachedPreciseLocation.
    pub fn into_zipline_reached_precise_location(&mut self, _m: Move) {
        self.stop_ignore_move_input();
        self.set_animation_movement_state(Move::None, 0.0);
        if !self.can_do_move(Move::ZipLine) {
            self.set_move(Move::Falling, false, false);
            return;
        }
        let zi = self.moves.into_zipline.zipline;
        self.moves.zipline.zipline = zi;
        let z = self.zipline(zi).clone();
        let n = (z.vol.spline_locations.len() as f32 - 1.0).max(1.0);
        let dir = z.vol.slope_on_spline(self.moves.into_zipline.enter_param / n);
        let mut dir2 = dir.safe_normal();
        dir2.z = 0.0;
        let pawn_dir = self.pawn.rotation.vector().safe_normal();
        let from_left = dir2.cross(pawn_dir).z > 0.0;
        let mut target = Rotator::from_vector(dir);
        target.pitch = 0;
        target.roll = 0;
        self.pawn.rotation = target;
        if self.pawn.velocity.z < self.moves.into_zipline.z_velocity_fall_limit {
            self.moves.into_zipline.saved_initial_2d_velocity = Vec3::ZERO;
        }
        let saved = self.moves.into_zipline.saved_initial_2d_velocity;
        self.pawn.velocity = dir * (saved.size_2d() * dir.dot(saved.safe_normal()).max(0.0));
        self.set_move(Move::ZipLine, false, false);
        self.moves.zipline.idle_start_position = if from_left { 0.5 } else { 0.0 };
    }

    /// TdMove_IntoZipLine.HitWall / FailedToReachPreciseLocation.
    pub fn into_zipline_fall(&mut self, _m: Move) {
        self.set_move(Move::Falling, false, false);
    }

    // ------------------------------------------------------------------ ZipLine

    /// TdMove_ZipLine.StartMove.
    pub fn zipline_start_move(&mut self, m: Move) {
        self.physics_move_start_move(m);
        let z = self.zipline(self.moves.zipline.zipline).clone();
        let mut start = self.pawn.location;
        start.z -= self.moves.zipline.hang_offset.z;
        let (_, param) = z.vol.find_closest_point_on_dspline(start, 0);
        self.moves.zipline.current_param_on_curve = param;
        let fade_in = self.moves.zipline.zip_fade_in_time;
        self.sound(SoundEvent::LoopStart { slot: LoopSlot::ZipLine, sound: LoopSound::Cue(ZIPPING_SOUND.into()), fade_in });
        self.moves.zipline.look_assist = true;
    }

    /// TdMove_ZipLine.StopMove.
    pub fn zipline_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        if self.moves.zipline.status == ZipLineStatus::Impact {
            self.anim.stop(Slot::FullBody, 0.3);
        } else {
            self.play_move_anim(m, Slot::FullBody, "SwingJumpOff", 1.0, 0.2, 0.2, false, false);
        }
        self.moves.zipline.zipline = None;
        self.moves.zipline.status = ZipLineStatus::Moving;
        self.set_physics(Physics::Falling);
        self.pawn.enter_falling_height -= 80.0;
        let fade_out = self.moves.zipline.zip_fade_out_time;
        self.sound(SoundEvent::LoopStop { slot: LoopSlot::ZipLine, fade_out });
        self.moves.zipline.look_assist = false;
    }

    /// UTdMove_ZipLine vt70 (0x1209400): gravity along the slope, then the velocity that lands
    /// the cable point `MinZipVelocity`-or-faster further along the spline next tick, and the
    /// look-ahead trace for the wall at the end.
    pub fn zipline_tick(&mut self, m: Move, dt: f32) {
        let Some(zi) = self.moves.zipline.zipline else { return };
        let z = self.ziplines[zi].clone();
        let g = self.pawn.gravity_z();
        let dir = self.pawn.velocity.safe_normal();
        self.pawn.velocity.z += g * dir.z.abs() * dt;
        let hang_z = self.moves.zipline.hang_offset.z;
        let p = Vec3::new(self.pawn.location.x, self.pawn.location.y, self.pawn.location.z - hang_z);
        let hint = self.moves.zipline.current_param_on_curve as i32;
        let (_, param) = z.vol.find_closest_point_on_dspline(p, hint);
        self.moves.zipline.current_param_on_curve = param;
        let min_v = self.moves.zipline.min_zip_velocity;
        let step = self.pawn.velocity.length().max(min_v) * dt;
        let pts = &z.vol.spline_locations;
        let mut i = param as i32;
        if step > 0.0 {
            loop {
                i += 1;
                if i as usize >= pts.len() {
                    // the cable ran out
                    self.set_move(Move::Falling, false, false);
                    return;
                }
                if step <= (pts[i as usize] - p).length() {
                    break;
                }
            }
        }
        let i = i.max(1) as usize;
        let d = pts[i] - pts[i - 1];
        let to_b = pts[i] - p;
        let t = to_b.dot(d) / d.length_squared();
        let perp = to_b - d * t;
        let along = (step * step - perp.length_squared()).max(0.0).sqrt();
        self.pawn.velocity = (d.safe_normal() * along + perp) * (1.0 / dt);
        if self.pawn.velocity.length() < min_v {
            self.pawn.velocity = self.pawn.velocity.safe_normal() * min_v;
        }
        let vdir = self.pawn.velocity.safe_normal();
        let acc = self.moves.zipline.min_zip_acceleration.max((g * vdir.z).abs());
        self.pawn.acceleration = vdir * acc;
        // the look-ahead trace from below the cable
        let start = Vec3::new(p.x, p.y, p.z - 40.0);
        let mut ext = self.pawn.extent();
        ext.z *= 0.5;
        let status = self.moves.zipline.status;
        let ahead = match status {
            ZipLineStatus::Moving => 600.0,
            ZipLineStatus::CloseToEnd => 20.0,
            ZipLineStatus::Impact => 2.0,
        };
        if self.movement_trace_for_blocking(start + vdir * ahead, start, ext) {
            match status {
                ZipLineStatus::Moving => {
                    self.zipline_prepare_for_forward_impact(m);
                    self.moves.zipline.status = ZipLineStatus::CloseToEnd;
                }
                ZipLineStatus::CloseToEnd => self.zipline_play_forward_impact(m),
                ZipLineStatus::Impact => {
                    self.pawn.velocity = Vec3::ZERO;
                    self.pawn.acceleration = Vec3::ZERO;
                }
            }
        }
        let v = self.pawn.velocity.safe_normal();
        self.moves.zipline.current_look_at_point = start + v * 600.0;
    }

    /// TdMove_ZipLine.PrepareForForwardImpact.
    fn zipline_prepare_for_forward_impact(&mut self, _m: Move) {
        if self.moves.zipline.status == ZipLineStatus::Moving {
            self.anim.stop(Slot::FullBody, 0.1);
            self.anim.play(Slot::FullBody, "ziplineintohitwall", 1.0, 0.3, 0.2, true, false, false);
            self.moves.zipline.status = ZipLineStatus::CloseToEnd;
        }
    }

    /// TdMove_ZipLine.PlayForwardImpact.
    fn zipline_play_forward_impact(&mut self, m: Move) {
        self.anim.stop(Slot::FullBody, 0.2);
        self.play_move_anim(m, Slot::FullBody, "ziplinehitwall", 1.0, 0.1, 0.2, false, false);
        let fade_out = self.moves.zipline.zip_fade_out_time;
        self.sound(SoundEvent::LoopStop { slot: LoopSlot::ZipLine, fade_out });
        self.set_animation_movement_state(Move::Falling, 0.0);
        self.moves.zipline.status = ZipLineStatus::Impact;
        self.set_ignore_look_input(0.8);
        self.set_ignore_move_input(0.8);
        self.set_move_countdown(m, 0.8);
    }

    /// TdMove_ZipLine.OnTimer.
    pub fn zipline_on_timer(&mut self, _m: Move) {
        self.set_move(Move::Falling, false, false);
    }

    /// TdMove_ZipLine.HitWall: a floor-ish hit would stumble (not ported); otherwise stop.
    pub fn zipline_hit_wall(&mut self, _m: Move, _normal: Vec3) {
        if self.pawn.velocity.size_2d() > 100.0 {
            self.pawn.velocity = Vec3::ZERO;
            self.pawn.acceleration = Vec3::ZERO;
        }
    }

    /// TdMove_ZipLine.UpdateViewRotation: turning the view drops the look assist, otherwise the
    /// view leads along the cable.
    pub fn zipline_view_rotation(&mut self, m: Move, delta: &Rotator) {
        if delta.yaw != 0 {
            self.abort_look_at_target(m);
            self.moves.zipline.look_assist = false;
        } else if self.moves.zipline.look_assist {
            let at = self.moves.zipline.current_look_at_point;
            self.set_look_at_target_location(m, at, 0.2, -1.0);
        }
    }
}
