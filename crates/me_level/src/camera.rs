//! TdPlayerPawn.CalcCamera pieces that sit on top of the EyeJoint: the swan neck (TdSwanNeck,
//! pushes the eye forward and down as you look down so you see your body instead of the inside
//! of it) and the camera animation remap from the CameraJoint.

use tdsim::pawn::Move;
use tdsim::{Rotator, Sim, Vec3};

/// TdSwanNeck with ESNT_Quadratic (DefaultGame.ini [TdGame.TdSwanNeck]).
pub struct SwanNeck {
    pub start_translate_at_degree: f32,
    pub forward: f32,
    pub downward: f32,
    /// X forward, Y down (uu).
    pub translation: (f32, f32),
}

const FORWARD_PITCH_WORLD: i32 = 65536;
const DOWNWARD_PITCH_WORLD: i32 = 48151;
const DEG_TO_UNDEG: f32 = 182.044_006;

impl Default for SwanNeck {
    fn default() -> Self {
        SwanNeck { start_translate_at_degree: 15.0, forward: 35.0, downward: 30.0, translation: (0.0, 0.0) }
    }
}

impl SwanNeck {
    /// TdMove.SetSwanNeckConstraints with the current move's class defaults (StartMove), and
    /// TdMove_SkillRoll.DisableSwanneck 0.2 s in.
    fn constraints(sim: &Sim) -> (f32, f32, f32) {
        let ms = sim.pawn.movement_state;
        match ms {
            Move::Turn180InAir | Move::LayOnGround => (0.0, 0.0, 0.0),
            Move::Grabbing => (0.0, 70.0, 30.0),
            Move::Climb => (15.0, 40.0, 30.0),
            Move::SkillRoll if sim.moves.base(ms).move_active_time >= 0.2 => (0.0, 0.0, 0.0),
            _ => (15.0, 35.0, 30.0),
        }
    }

    /// TdSwanNeck.GetSwanNeckTranslation.
    fn wanted(&self, controller_pitch: i32) -> (f32, f32) {
        let start = (self.start_translate_at_degree * DEG_TO_UNDEG) as i32;
        let pitch = controller_pitch & 0xFFFF;
        if !(pitch > DOWNWARD_PITCH_WORLD && pitch < FORWARD_PITCH_WORLD) {
            return (0.0, 0.0);
        }
        let p = FORWARD_PITCH_WORLD - pitch;
        let limit = FORWARD_PITCH_WORLD - DOWNWARD_PITCH_WORLD;
        if p <= start {
            return (0.0, 0.0);
        }
        let delta = (limit - start) as f32;
        let t = (p - start) as f32;
        let a = t / delta * std::f32::consts::PI * 0.25;
        (self.forward / delta * t * a.cos(), self.downward / delta * t * a.sin())
    }

    /// TdSwanNeck.UpdateSwanNeck (TdPlayerPawn.Tick).
    pub fn update(&mut self, sim: &Sim, dt: f32) {
        let (start, fwd, down) = Self::constraints(sim);
        self.start_translate_at_degree = start;
        self.forward = fwd;
        self.downward = down;
        let w = self.wanted(sim.pc.rotation.pitch);
        let k = (dt / 0.07).min(1.0);
        self.translation.0 += ((w.0 - self.translation.0) * k).min(self.forward);
        self.translation.1 += ((w.1 - self.translation.1) * k).min(self.downward);
    }

    /// TdSwanNeck.GetSwanNeckPos for the view yaw: forward and down, Unreal space.
    pub fn offset(&self, view: Rotator) -> Vec3 {
        let (x, _, z) = Rotator::new(0, view.yaw, 0).axes();
        x * self.translation.0 - z * self.translation.1
    }
}

/// TdPlayerPawn.CalcCamera: view rotation plus the camera animation (GetCameraAnimation).
pub fn camera_rotation(view: Rotator, cam_anim: Rotator) -> Rotator {
    Rotator::new(view.pitch - cam_anim.roll, view.yaw + cam_anim.pitch, view.roll - cam_anim.yaw)
}
