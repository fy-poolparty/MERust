//! TdPawn::GetAimMode (0x12BA920) and the moves' GetAimMode (UTdMove vt69): which hands follow
//! the view. TdSkelControlAim1p on SpineXLeft / SpineXRight reads it to turn the arms with the
//! camera (the arm that reaches out when you look away from a ladder).

use super::{class_of, Class};
use crate::pawn::{Move, WalkingState};
use crate::sim::Sim;

/// TdPawn.MoveAimMode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AimMode {
    Left,
    Right,
    TwoHanded,
    NoHands,
    Default,
}

/// The AimMode default property of each ported move class.
pub fn class_aim_mode(c: Class) -> AimMode {
    match c {
        Class::Vertigo => AimMode::TwoHanded,
        Class::SkillRoll | Class::LayOnGround | Class::WallClimb180TurnJump | Class::WallClimbDodgeJump => AimMode::Right,
        Class::Climb | Class::IntoClimb | Class::WallClimb | Class::GrabTransfer => AimMode::NoHands,
        Class::Swing | Class::ZipLine | Class::IntoZipLine | Class::Balance | Class::Disarm => AimMode::NoHands,
        _ => AimMode::Default,
    }
}

impl Sim {
    /// UTdMove::vt69 with the native overrides: Jump (0x11F3640) is always two-handed; Climb
    /// (0x11F30E0) and Grab (0x11F2DD0, not when hanging free) free the hand on the side the
    /// view turned past StartTurningAngle.
    pub fn move_aim_mode(&self, m: Move, aiming_only: bool) -> AimMode {
        let c = class_of(m);
        let side = |sta: f32| {
            let d = (self.pc.rotation - self.pawn.rotation).normalize().yaw as f32;
            if d > sta {
                Some(AimMode::Right)
            } else if d < -sta {
                Some(AimMode::Left)
            } else {
                None
            }
        };
        match c {
            Class::Jump => AimMode::TwoHanded,
            Class::Climb if aiming_only => side(self.moves.climb.start_turning_angle as f32).unwrap_or(class_aim_mode(c)),
            Class::Grab if aiming_only && !self.grab_is_hanging_free() => side(self.moves.grab.start_turning_angle).unwrap_or(class_aim_mode(c)),
            _ => class_aim_mode(c),
        }
    }

    /// TdPawn::GetAimMode: the move's mode, and for MAM_Default while aiming, both hands once
    /// the pawn jogs or faster, else the gun hand while the gun is ready.
    pub fn aim_mode(&self, aiming_only: bool) -> AimMode {
        use crate::weapons::WeaponAnimState as W;
        let v = self.move_aim_mode(self.pawn.movement_state, aiming_only);
        if v == AimMode::Default && aiming_only {
            // 0x12BA920: jogging or faster, or a heavy gun -> both hands; a ready (or thrown)
            // light gun -> the gun hand
            if self.pawn.current_walking_state as u8 > WalkingState::Walk as u8 || self.heavy_weapon() {
                return AimMode::TwoHanded;
            }
            if matches!(self.weapon_anim_state, W::Ready | W::Throwing) {
                return AimMode::Right;
            }
        }
        v
    }
}
