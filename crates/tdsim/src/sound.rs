//! The pawn's sound triggers: AnimNotifies (footsteps, character sounds, cues), TdPawn's
//! PlayFootStepSound / GetCharacterSoundCue and the moves' looping sounds. The sim only decides
//! what to play; the presentation layer resolves cues and surfaces to sound data.

use crate::anim::Notify;
use crate::math::{Vec3, UeVec};
use crate::pawn::{Move, Physics};
use crate::sim::{Event, Sim};

/// Something to play or stop.
#[derive(Clone, Debug, PartialEq)]
pub enum SoundEvent {
    /// TdPawn.ActuallyPlayFootStepSound: the cue(s) the surface's TdPhysicalMaterialFootSteps
    /// gives for this trigger id (1..11 feet, 21..26 hands, 31..36 body).
    Footstep { id: i32, material: &'static str },
    /// A SoundCue ("Package.Path").
    Cue(String),
    /// An AudioComponent the pawn keeps playing until told to stop.
    /// `fade_in`: AudioComponent.FadeIn time (0 plays at full volume).
    LoopStart { slot: LoopSlot, sound: LoopSound, fade_in: f32 },
    LoopStop { slot: LoopSlot, fade_out: f32 },
    /// TdPawn.PlayMeleeImpact: the victim's TdPhysicalMaterialMelee fist / foot sound.
    MeleeImpact { impact: crate::combat::MeleeImpact, head: bool },
    /// A SoundCue played at a location (another pawn's gun).
    CueAt { name: String, location: crate::math::Vec3 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum LoopSound {
    Cue(String),
    Footstep { id: i32, material: &'static str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoopSlot {
    /// TdPawn.SlidingSoundComponent0/1 (StartSlideEffect / StopSlideEffect).
    Slide,
    /// TdMove_Climb.ClimbSoundComponent (sliding down a ladder or pipe).
    ClimbDownFast,
    /// TdPlayerPawn state UncontrolledFall: FallingSound.
    Falling,
    /// TdMove_Swing.SwingSoundComponent.
    Swing,
    /// TdMove_ZipLine.ZippingSoundComponent.
    ZipLine,
}

/// TdPawn.DefaultFootstepMaterial, and the landing bags' material.
pub const DEFAULT_FOOTSTEP_MATERIAL: &str = "PM_Concrete";
pub const SOFT_LANDING_MATERIAL: &str = "PM_Plastic_PropLarge";

/// TdPlayerPawn.CharacterSoundCues.
pub const CHARACTER_SOUND_CUES: [Option<&str>; 30] = [
    Some("A_Character_Female_01.Breath_Soft.Breath_Soft_Short_In"),
    Some("A_Character_Female_01.Breath_Soft.Breath_Soft_Short_Out"),
    Some("A_Character_Female_01.Breath_Soft.Breath_Soft_Long_In"),
    Some("A_Character_Female_01.Breath_Soft.Breath_Soft_Long_Out"),
    Some("A_Character_Female_01.Breath_Medium.Breath_Medium_Short_In"),
    Some("A_Character_Female_01.Breath_Medium.Breath_Medium_Short_Out"),
    Some("A_Character_Female_01.Breath_Medium.Breath_Medium_Long_In"),
    Some("A_Character_Female_01.Breath_Medium.Breath_Medium_Long_Out"),
    Some("A_Character_Female_01.Breath_Hard.Breath_Hard_Short_In"),
    Some("A_Character_Female_01.Breath_Hard.Breath_Hard_Short_Out"),
    Some("A_Character_Female_01.Breath_Hard.Breath_Hard_Long_In"),
    Some("A_Character_Female_01.Breath_Hard.Breath_Hard_Long_Out"),
    None,
    None,
    None,
    None,
    Some("A_Character_Female_01.Oral_Impact.Soft"),
    Some("A_Character_Female_01.Oral_Impact.Medium"),
    Some("A_Character_Female_01.Oral_Impact.Hard"),
    Some("A_Character_Female_01.Oral_Strain.Soft"),
    Some("A_Character_Female_01.Oral_Strain.Medium"),
    Some("A_Character_Female_01.Oral_Strain.Hard"),
    None,
    Some("A_Character_Female_01.Oral_Snatch.Snatch"),
    None,
    Some("A_Character_Female_01.Oral_Death.Death"),
    Some("A_Character_Female_01.Cloth.Crouch"),
    Some("A_Character_Female_01.Cloth.Walk"),
    Some("A_Character_Female_01.Cloth.Run"),
    Some("A_Character_Effects.Movement.Vault"),
];

/// TdPlayerPawn.NoOfBreathingSounds.
const NO_OF_BREATHING_SOUNDS: usize = 8;
/// TdPawn.FootstepTraceLength.
const FOOTSTEP_TRACE_LENGTH: f32 = 80.0;

pub const CLIMB_DOWN_LADDER_FAST_SOUND: &str = "A_Material_Handstep.Metal_Slide.Ladder";
pub const CLIMB_DOWN_PIPE_FAST_SOUND: &str = "A_Material_Handstep.Metal_Slide.Pipe";
pub const DEATH_FALL_SOUND: &str = "A_Bodyfalls.Faith.Death_Fall";
pub const DEATH_IMPACT_SOUND: &str = "A_Bodyfalls.Faith.Death_Impact";
/// TdPlayerPawn.WindSoundSC (always playing; its TdSoundNodeVelocity nodes follow the speed).
pub const WIND_SOUND: &str = "A_Character_Effects.Movement.RunWind";

/// The characters' TdPhysicalMaterialMelee sounds (PM_Character_Head / PM_Character_Body):
/// [head, body] x [gun, fist, foot].
pub const MELEE_IMPACT_SOUNDS: [[&str; 3]; 2] = [
    ["A_Character_Melee.A_Female.Gun_Head", "A_Character_Melee.A_Female.Fist_Head", "A_Character_Melee.A_Female.Foot_Head"],
    ["A_Character_Melee.A_Female.Gun_Body", "A_Character_Melee.A_Female.Fist_Body", "A_Character_Melee.A_Female.Foot_Body"],
];

/// TdPawn.PlayMeleeImpact's cue for an impact type on the head (Neck) or the body.
pub fn melee_impact_cue(impact: crate::combat::MeleeImpact, head: bool) -> &'static str {
    let k = match impact { crate::combat::MeleeImpact::Gun => 0, crate::combat::MeleeImpact::Fist => 1, crate::combat::MeleeImpact::Foot => 2 };
    MELEE_IMPACT_SOUNDS[if head { 0 } else { 1 }][k]
}

impl Sim {
    pub(crate) fn sound(&mut self, e: SoundEvent) {
        self.events.push(Event::Sound(e));
    }

    /// The AnimNotify::Notify of each notify kind.
    pub fn fire_notify(&mut self, n: &Notify) {
        match n {
            Notify::Footstep(id) => self.play_foot_step_sound(*id),
            Notify::Cue(c) => self.sound(SoundEvent::Cue(c.clone())),
            Notify::CharacterSound(t) => {
                if let Some(c) = self.character_sound_cue(*t as usize, true) {
                    self.sound(SoundEvent::Cue(c.to_string()));
                }
            }
        }
    }

    /// ATdPawn GetCharacterSoundCue (0x12B5E30): breaths alternate between the In and Out cue
    /// of their pair (bCharacterInhaling), everything else sits after the breathing pairs.
    pub fn character_sound_cue(&mut self, t: usize, toggle: bool) -> Option<&'static str> {
        if self.pawn.disable_character_sounds {
            return None;
        }
        let i = if t >= NO_OF_BREATHING_SOUNDS {
            NO_OF_BREATHING_SOUNDS + t
        } else {
            let i = if self.pawn.character_inhaling { 2 * t } else { 2 * t + 1 };
            if toggle {
                self.pawn.character_inhaling = !self.pawn.character_inhaling;
            }
            i
        };
        CHARACTER_SOUND_CUES.get(i).copied().flatten()
    }

    /// The surface under (or beside) the pawn for a footstep trace: the material name, or
    /// None when the trace finds nothing.
    fn footstep_surface(&self, start: Vec3, dest: Vec3) -> Option<&'static str> {
        let ext = Vec3::splat(0.5 * 10.0);
        let h = self.world.line_check(dest, start, ext);
        if !h.hit {
            return None;
        }
        Some(if h.surface.soft_landing { SOFT_LANDING_MATERIAL } else { DEFAULT_FOOTSTEP_MATERIAL })
    }

    /// TdPawn.PlayFootStepSound: trace from the limb towards the surface it touches (down
    /// for the feet, at the wall in wall runs / hangs / climbs) and play that surface's sound.
    /// The limb bones are approximated by the cylinder (feet at its bottom, hands at its top).
    pub fn play_foot_step_sound(&mut self, foot_down: i32) {
        let id = foot_down.abs();
        let p = &self.pawn;
        let h = p.collision_height;
        let mut facing = p.rotation.vector();
        facing.z = 0.0;
        let facing = facing.safe_normal();
        let up = Vec3::new(0.0, 0.0, 1.0);
        let feet = p.location - up * (h - 5.0);
        let hands = p.location + up * (h - 10.0);
        let limb = if (21..=30).contains(&id) { hands } else if id > 30 { p.location } else { feet };
        let len = FOOTSTEP_TRACE_LENGTH;
        let (start, dest) = if id == 0 {
            (p.location, p.location - up * (h + 35.0))
        } else {
            match p.movement_state {
                Move::WallRunningRight => (limb, limb + (facing.cross(up) + up) * len),
                Move::WallRunningLeft => (limb, limb - (facing.cross(up) + up) * len),
                Move::SpeedVaulting | Move::VaultOver | Move::IntoGrab | Move::GrabPullUp => {
                    let s = if id > 20 { p.move_ledge_location + up * 2.0 } else { limb };
                    (s, s + (facing - up * 0.5) * len)
                }
                Move::Grabbing | Move::WallClimbing | Move::IntoClimb | Move::Climb | Move::Falling | Move::Coil => {
                    (limb, limb + (facing + up * 0.5) * len)
                }
                Move::Jump | Move::SpringBoarding | Move::DodgeJump => (limb, limb - up * len * 2.0),
                _ if (8..=10).contains(&id) => (limb, limb - up * len * 2.0),
                _ => (limb, limb - up * len),
            }
        };
        let Some(material) = self.footstep_surface(start, dest) else { return };
        // ActuallyPlayFootStepSound skips these trigger ids
        if id == 12 || (26..31).contains(&id) {
            return;
        }
        self.sound(SoundEvent::Footstep { id, material });
    }

    /// TdPawn.StartSlideEffect: the slide loop from the surface under the right foot (id 36).
    pub(crate) fn start_slide_effect(&mut self) {
        let p = &self.pawn;
        let foot = p.location - Vec3::new(0.0, 0.0, p.collision_height - 5.0);
        let material = self.footstep_surface(foot, foot - Vec3::new(0.0, 0.0, FOOTSTEP_TRACE_LENGTH)).unwrap_or(DEFAULT_FOOTSTEP_MATERIAL);
        self.sound(SoundEvent::LoopStart { slot: LoopSlot::Slide, sound: LoopSound::Footstep { id: 36, material }, fade_in: 0.0 });
    }

    /// TdPawn.StopSlideEffect.
    pub(crate) fn stop_slide_effect(&mut self) {
        self.sound(SoundEvent::LoopStop { slot: LoopSlot::Slide, fade_out: 0.5 });
    }

    /// Speed the wind loop's TdSoundNodeVelocity nodes read (SPEEDTYPE_Source: the pawn's).
    pub fn wind_speed(&self) -> f32 {
        if self.pawn.physics == Physics::None {
            0.0
        } else {
            self.pawn.velocity.length()
        }
    }
}
