//! First-person body pose: samples AS_C1P_Unarmed for the 74-bone 1p skeleton (shared by
//! SK_UpperBody and SK_LowerBody) and blends the sim's custom-animation slots over a
//! locomotion layer, standing in for AT_C1P.
//!
//! Output is in glTF space as written by umodel: position (x, z, y) / 100, rotation
//! (x, z, y, -w) of the Unreal values. That is also Bevy's space.

use glam::{Mat4, Quat, Vec3 as GVec3};
use tdsim::pawn::{Move, Physics, Slot, WalkingState};
use tdsim::Sim;
use upk::anim::AnimSet;
use upk::skelmesh::SkelMesh;
use crate::anims::AimComponent;

#[derive(Clone, Copy, Debug)]
pub struct BonePose {
    pub pos: GVec3,
    pub rot: Quat,
}

impl BonePose {
    fn lerp(a: BonePose, b: BonePose, t: f32) -> BonePose {
        let rb = if a.rot.dot(b.rot) < 0.0 { -b.rot } else { b.rot };
        BonePose { pos: a.pos.lerp(b.pos, t), rot: a.rot.lerp(rb, t).normalize() }
    }
}

/// Unreal bone-space translation/rotation to glTF space.
pub fn ue_pos(p: [f32; 3]) -> GVec3 {
    GVec3::new(p[0], p[2], p[1]) * 0.01
}
/// Reference-pose bone rotation (SkeletalMesh RefSkeleton) to glTF space.
pub fn ue_rot(q: [f32; 4]) -> Quat {
    Quat::from_xyzw(q[0], q[2], q[1], -q[3]).normalize()
}
/// Animation track rotation to glTF space: AnimSequence keys are stored conjugated relative to
/// the reference pose, so W keeps its sign here.
pub fn ue_anim_rot(q: [f32; 4]) -> Quat {
    Quat::from_xyzw(q[0], q[2], q[1], q[3]).normalize()
}

/// The AT_C3P DirBones and their node weights (walk, crouch, against wall).
struct ThirdPersonAim {
    walk: Vec<(usize, AimComponent)>,
    crouch: Vec<(usize, AimComponent)>,
    wall: Vec<(usize, AimComponent)>,
    weights: [f32; 3],
}

/// One TdSkelControlAgainstWall (mesh-space Min/Max clamp, effector easing) with the weight of
/// its arm's AgainstWallState blend (which is also the control's strength).
#[derive(Clone)]
struct WallArm {
    left: bool,
    min: GVec3,
    max: GVec3,
    effector: GVec3,
    target: GVec3,
    weight: f32,
}

impl WallArm {
    /// AT_C1P TdSkelControlAgainstWall_3 (left, class default limits) / _2 (right).
    fn new(left: bool) -> Self {
        let (min, max, eff) = if left {
            (GVec3::new(5.0, -170.0, 10.0), GVec3::new(35.0, -100.0, 30.0), GVec3::new(5.0, -154.75, 25.05708))
        } else {
            (GVec3::new(-35.0, -170.0, 10.0), GVec3::new(-5.0, -100.0, 30.0), GVec3::new(-27.059465, -147.75, 10.0))
        };
        WallArm { left, min, max, effector: eff, target: eff, weight: 0.0 }
    }
}

/// TdAnimNodeBlendDirectional state.
#[derive(Clone, Copy)]
struct DirNode {
    direction: (f32, f32),
    forward_blend: f32,
    relevant: bool,
    ticked: bool,
}

impl Default for DirNode {
    fn default() -> Self {
        DirNode { direction: (1.0, 1.0), forward_blend: 1.0, relevant: false, ticked: false }
    }
}

/// One TdSkelControlAim1p: ControlStrength, StrengthTarget and BlendTimeToGo.
#[derive(Clone, Copy, Default)]
struct ArmAim {
    strength: f32,
    target: f32,
    time_to_go: f32,
}

impl ArmAim {
    /// USkelControlBase::SetSkelControlStrength (0xD10650).
    fn set_strength(&mut self, target: f32, time: f32) {
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

    /// USkelControlBase::TickSkelControl's strength blend (0xD132A0).
    fn tick(&mut self, dt: f32) {
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

/// An AimTransform quaternion (mesh space) in glTF space.
fn aim_rot(q: [f32; 4]) -> Quat {
    Quat::from_xyzw(q[0], q[2], q[1], AIM_W_SIGN * q[3]).normalize()
}
const AIM_W_SIGN: f32 = -1.0;

/// A locomotion blend: clips with weights, all at one normalized phase (AnimNodeSynch).
#[derive(Clone, Default)]
struct Loco {
    clips: Vec<(usize, f32)>,
    phase: f32,
}

impl Loco {
    fn same_clips(&self, other: &[(usize, f32)]) -> bool {
        self.clips.len() == other.len() && self.clips.iter().zip(other).all(|(a, b)| a.0 == b.0)
    }
}

pub struct PoseEvaluator {
    pub bone_names: Vec<String>,
    pub parents: Vec<usize>,
    ref_pose: Vec<BonePose>,
    /// skeleton bone -> AnimSet track
    track_of_bone: Vec<Option<usize>>,
    seq_by_name: std::collections::HashMap<String, usize>,
    /// The armed sets' sequences (TdPawn.UpdateAnimSets with a gun), searched first while
    /// `armed`.
    armed_sets: std::collections::HashMap<String, std::collections::HashMap<String, usize>>,
    /// The gun class whose sets are in use (TdPawn.UpdateAnimSets).
    pub armed: Option<&'static str>,
    /// TdAnimNodeWeaponPoseOffset (the AT_C1P root) profiles: per name, (bone, glTF
    /// translation offset, glTF rotation offset).
    weapon_pose_profiles: std::collections::HashMap<String, Vec<(usize, GVec3, Quat)>>,
    /// Sequence indices from a CommonArmed set, and the active offset profile (set each update).
    common_armed: std::collections::HashSet<usize>,
    active_pose_profile: Option<&'static str>,
    set: AnimSet,
    loco: Loco,
    /// AnimNodeBlendList-style: the active clip set blends in while the ones it replaced keep
    /// their current weights and blend out, so a change mid-blend continues from where it is.
    loco_weight: f32,
    fading: Vec<(Loco, f32)>,
    loco_blend_left: f32,
    /// TdAnimNodeGrabbing keeps its last turn blend while the view is inside StartTurningAngle.
    last_grab_turn: Vec<(&'static str, f32)>,
    /// TdAnimNodeClimb switches children with no blend; the slide child is a TdAnimNodeRandom.
    climb_active: bool,
    climb_slide_pick: usize,
    was_climb_sliding: bool,
    /// Crossfade time for the next locomotion change (SetActiveChild blend time).
    next_loco_blend: Option<f32>,
    loco_blend_time: f32,
    /// TdAnimNodeBlendDirectional Sneak / Walk / Run.
    dir_nodes: [DirNode; 3],
    dt: f32,
    /// (MovementState, WalkingState) last frame, for TdAnimNodeWalkingState's blend times.
    last_walk_key: Option<(Move, WalkingState)>,
    /// TdAnimNodeDirBone "1pAim" (hips/spine twist by LegRotation) and "AgainstWallCam" (eye
    /// pulled back looking down at a wall): resolved bones and their node weights.
    hip_aim: Vec<(usize, AimComponent)>,
    wall_cam_aim: Vec<(usize, AimComponent)>,
    hip_aim_weight: f32,
    wall_cam_weight: f32,
    /// TdAnimNodeAimOffset_2 over the standing idle: feet and hips settle towards the view
    /// when the legs are turned (bAimSourceIsLegRotation, interpolated at speed 7, +-0.2).
    idle_aim: Vec<(usize, AimComponent)>,
    idle_aim_weight: f32,
    idle_interp: f32,
    /// TdAnimNodeTurn's turn step over the idle: weight and the clip / position shown.
    turn_weight: f32,
    turn_clip: Option<(&'static str, f32)>,
    /// AgainstWallState_1/_0 ("againstwall" on the left / right arm: ArmedLeft / ArmedRight
    /// per-bone blends) and the TdSkelControlAgainstWall hand IK on each side.
    wall_arms: [WallArm; 2],
    wall_clip_time: f32,
    /// Branch start bones of the ArmedLeft / ArmedRight per-bone blends.
    wall_branches: [Vec<&'static str>; 2],
    /// The third-person body's AT_C3P DirBones (shadow body): its leg twist sits above the
    /// slots, under TdAnimNodeAgainstWallState_2 / MoveSwitch.
    third_person: Option<ThirdPersonAim>,
    /// SkeletalMesh Origin / RotOrigin (mesh space <-> actor space).
    mesh_origin: [f32; 3],
    mesh_rot_origin: tdsim::Rotator,
    /// TdAnimNodeBalanceBlend's BalanceFactor, RandomLeanTarget, RandomLean, and its rand().
    balance_node: [f32; 3],
    balance_relevant: bool,
    rng: u32,
    /// Play-rate scale of the locomotion clips this frame (ScalePlayRateBySpeed nodes).
    loco_rate: f32,
    /// TdSkelControlAim1p on SpineXLeft / SpineXRight: [left, right].
    arm_aim: [ArmAim; 2],
    /// TdAnimNodeWeaponState "WeaponTypeSwitch" (the ArmedRight branch): the ready arm's
    /// weight and clip time.
    ready_weight: f32,
    ready_time: f32,
    /// TdSkelControlRecoil "RightHandRecoil"'s strength (on while a light gun is ready).
    recoil_weight: f32,
    /// TdSwanNeck.GetSwanNeckPos for the last PlayerCameraRotation's yaw (world, Unreal
    /// units), which the aim controls add to the arms.
    pub swan_world: tdsim::Vec3,
    /// Only the 1p evaluator feeds the MasterSync phase back to the sim.
    pub drives_phase: bool,
    pub pose: Vec<BonePose>,
}

impl PoseEvaluator {
    pub fn new(set: AnimSet, skel: &SkelMesh) -> Self {
        let bone_names: Vec<String> = skel.bones.iter().map(|b| b.name.clone()).collect();
        let parents = skel.bones.iter().enumerate().map(|(i, b)| if i == 0 { 0 } else { b.parent as usize }).collect();
        let ref_pose = skel.bones.iter().map(|b| BonePose { pos: ue_pos(b.position), rot: ue_rot(b.orientation) }).collect::<Vec<_>>();
        let track_of_bone = bone_names
            .iter()
            .map(|n| set.track_bone_names.iter().position(|t| t.eq_ignore_ascii_case(n)))
            .collect();
        let seq_by_name = set.seqs.iter().enumerate().map(|(i, s)| (s.name.to_ascii_lowercase(), i)).collect();
        let pose = ref_pose.clone();
        PoseEvaluator {
            bone_names,
            parents,
            ref_pose,
            track_of_bone,
            seq_by_name,
            armed_sets: Default::default(),
            armed: None,
            weapon_pose_profiles: std::collections::HashMap::new(),
            common_armed: Default::default(),
            active_pose_profile: None,
            set,
            loco: Loco::default(),
            loco_weight: 1.0,
            fading: Vec::new(),
            loco_blend_left: 0.0,
            last_grab_turn: Vec::new(),
            climb_active: false,
            climb_slide_pick: 0,
            was_climb_sliding: false,
            next_loco_blend: None,
            loco_blend_time: 0.2,
            dir_nodes: Default::default(),
            dt: 0.0,
            last_walk_key: None,
            hip_aim: Vec::new(),
            wall_cam_aim: Vec::new(),
            hip_aim_weight: 0.0,
            wall_cam_weight: 0.0,
            idle_aim: Vec::new(),
            idle_aim_weight: 0.0,
            idle_interp: 0.0,
            turn_weight: 0.0,
            turn_clip: None,
            // [left, right], the order the AgainstWallState / branch tables use
            wall_arms: [WallArm::new(true), WallArm::new(false)],
            wall_clip_time: 0.0,
            wall_branches: [vec!["SpineXLeft", "LeftHand_GameIK"], vec!["SpineXRight", "RightHand_GameIK", "CameraJoint"]],
            third_person: None,
            mesh_origin: skel.origin,
            mesh_rot_origin: tdsim::Rotator::new(skel.rot_origin[0], skel.rot_origin[1], skel.rot_origin[2]),
            balance_node: [0.0; 3],
            balance_relevant: false,
            rng: 0x2545_F491,
            loco_rate: 1.0,
            arm_aim: Default::default(),
            ready_weight: 0.0,
            ready_time: 0.0,
            recoil_weight: 0.0,
            swan_world: tdsim::Vec3::ZERO,
            drives_phase: true,
            pose,
        }
    }

    /// The AT_C1P DirBone profiles (`anims::load_aim_profile` of TdAnimNodeDirBone_0 / _1).
    pub fn with_aim(mut self, hip: Vec<AimComponent>, wall_cam: Vec<AimComponent>) -> Self {
        let resolve = |v: Vec<AimComponent>, s: &Self| v.into_iter().filter_map(|c| s.bone_index(&c.bone).map(|i| (i, c))).collect::<Vec<_>>();
        self.hip_aim = resolve(hip, &self);
        self.wall_cam_aim = resolve(wall_cam, &self);
        self
    }

    /// AT_C3P TdAnimNodeDirBone_15 "WalkRelaxed", _7 "CrouchWalkRelaxed" and _0 "againstwall":
    /// makes this the third-person evaluator (AT_C3P locomotion and leg twist).
    pub fn third_person(mut self, walk: Vec<AimComponent>, crouch: Vec<AimComponent>, wall: Vec<AimComponent>) -> Self {
        let resolve = |v: Vec<AimComponent>, s: &Self| v.into_iter().filter_map(|c| s.bone_index(&c.bone).map(|i| (i, c))).collect::<Vec<_>>();
        self.third_person = Some(ThirdPersonAim {
            walk: resolve(walk, &self),
            crouch: resolve(crouch, &self),
            wall: resolve(wall, &self),
            weights: [0.0; 3],
        });
        // AT_C3P ArmedLeft / ArmedRight (AnimNodeBlendPerBone_1 / _0) branch at LeftArm /
        // RightShoulder; its TdSkelControlAgainstWall_1 / _0 start their effectors elsewhere
        self.wall_branches = [vec!["LeftArm"], vec!["RightShoulder"]];
        for (arm, e) in self.wall_arms.iter_mut().zip([GVec3::new(5.0, -141.75, 30.0), GVec3::new(-30.259008, -146.25, 30.0)]) {
            arm.effector = e;
            arm.target = e;
        }
        self
    }

    /// AT_C1P TdAnimNodeAimOffset_2's profile (the standing idle's leg-rotation adjustment).
    pub fn with_idle_aim(mut self, comps: Vec<AimComponent>) -> Self {
        self.idle_aim = comps.into_iter().filter_map(|c| self.bone_index(&c.bone).map(|i| (i, c))).collect();
        self
    }

    /// TdAnimNodeBlendDirectional (UpdateDirection 0x1212B90, TickAnim 0x1212980). Children:
    /// Forward, ForwardRight, ForwardLeft, Backward, BackWardRight, BackWardLeft. Direction is
    /// the velocity in pawn space (X right, Y forward) normalised so |X| + |Y| = 1; the side
    /// children take up to 0.9 of it, forward/backward the rest, and ForwardBlend fades
    /// between the forward and backward halves as bGoingForward flips (ForwardInterpTime 0.4).
    fn blend_directional(&mut self, sim: &Sim, node: usize, clips: [&'static str; 6]) -> Vec<(&'static str, f32)> {
        let p = &sim.pawn;
        let n = &mut self.dir_nodes[node];
        let v = tdsim::Vec3::new(p.velocity.x, p.velocity.y, 0.0);
        if v.length() > 1e-8 {
            let d = v.normalize();
            let (fwd, right, _) = p.rotation.axes();
            // DirInterpTime 0.1 * 10: the new direction is taken whole
            n.direction = (d.dot(right), d.dot(fwd));
        }
        let (mut x, mut y) = n.direction;
        if (x * x + y * y).sqrt() >= 1e-8 {
            let k = 1.0 / (x.abs() + y.abs());
            x *= k;
            y *= k;
        } else {
            (x, y) = (1.0, 1.0);
        }
        n.direction = (x, y);
        if !n.relevant {
            // OnBecomeRelevant
            n.forward_blend = if y > 0.0 { 1.0 } else { 0.0 };
        }
        n.ticked = true;
        let step = self.dt / 0.4;
        n.forward_blend = if p.going_forward { (n.forward_blend + step).min(1.0) } else { (n.forward_blend - step).max(0.0) };
        let fb = n.forward_blend;
        let fwd = y.abs().max(0.1);
        let r = x.clamp(0.0, 0.9);
        let l = (-x).clamp(0.0, 0.9);
        let w = [fwd * fb, r * fb, l * fb, fwd * (1.0 - fb), r * (1.0 - fb), l * (1.0 - fb)];
        let mut out: Vec<(&'static str, f32)> = Vec::new();
        for (c, w) in clips.iter().zip(w) {
            match out.iter_mut().find(|(n, _)| n == c) {
                Some(e) => e.1 += w,
                None => out.push((c, w)),
            }
        }
        out
    }

    fn seq(&self, name: &str) -> Option<usize> {
        let key = name.to_ascii_lowercase();
        if let Some(&i) = self.armed.and_then(|c| self.armed_sets.get(c)).and_then(|m| m.get(&key)) {
            return Some(i);
        }
        self.seq_by_name.get(&key).copied()
    }

    /// Add the armed sets' sequences (already keyed to this set's tracks); later ones win.
    /// A gun's armed sequences; the first `n_common` are the CommonArmed set's (the ones the
    /// gun's weapon pose offset applies to).
    pub fn add_armed_seqs(&mut self, class: &str, seqs: Vec<upk::anim::AnimSeq>, n_common: usize) {
        let mut map = std::collections::HashMap::new();
        for (k, s) in seqs.into_iter().enumerate() {
            let i = self.set.seqs.len();
            map.insert(s.name.to_ascii_lowercase(), i);
            if k < n_common {
                self.common_armed.insert(i);
            }
            self.set.seqs.push(s);
        }
        self.armed_sets.insert(class.to_string(), map);
    }

    fn sample(&self, seq: Option<usize>, time: f32) -> Vec<BonePose> {
        let Some(si) = seq else { return self.ref_pose.clone() };
        let mut pose = self.sample_raw(si, time);
        // TdAnimNodePoseOffset (sub_12104E0) on the CommonArmed set's sequences: re-posed to hold
        // this gun (the gun's own sequences are authored for it already)
        if self.common_armed.contains(&si) {
            if let Some(p) = self.active_pose_profile.and_then(|n| self.weapon_pose_profiles.get(n)) {
                for &(b, t, q) in p {
                    // decoded keys go to Q * rotation; the glTF mapping reverses products
                    pose[b].pos -= t;
                    pose[b].rot = (pose[b].rot * q).normalize();
                }
            }
        }
        pose
    }

    fn sample_raw(&self, si: usize, time: f32) -> Vec<BonePose> {
        let s = &self.set.seqs[si];
        self.ref_pose
            .iter()
            .enumerate()
            .map(|(b, r)| match self.track_of_bone[b] {
                // Root bone: TdPawn plays custom anims with RRA_Discard / RRA_Extract rotation and
                // the translation either extracted as root motion or moved by the move itself, so
                // the bone stays at its reference pose.
                Some(_) if b == 0 => *r,
                Some(t) if t < s.tracks.len() => {
                    let (p, q) = s.sample(t, time);
                    let has_pos = !s.tracks[t].pos.is_empty();
                    let has_rot = !s.tracks[t].rot.is_empty();
                    BonePose { pos: if has_pos { ue_pos(p) } else { r.pos }, rot: if has_rot { ue_anim_rot(q) } else { r.rot } }
                }
                _ => *r,
            })
            .collect()
    }

    /// TdAnimNodeGrabSlope (UpdateWeights 0x1213ED0): lean into hang45left/right by how much
    /// the ledge tilts sideways.
    fn grab_slope(sim: &Sim, base: &'static str, left: &'static str, right: &'static str) -> Vec<(&'static str, f32)> {
        let p = &sim.pawn;
        let n = p.move_normal;
        let side = (-n.y, n.x, 0.0);
        let ln = p.move_ledge_normal;
        let d = ln.x * side.0 + ln.y * side.1 + ln.z * side.2;
        let s = (d.abs().min(1.0).asin() * 1.273_239_5).clamp(-1.0, 1.0);
        // children: Hang, Hang45 (hang45right), Hang45m (hang45left); d > 0 weights child 2
        let (l, r) = if d > 0.0 { (s, 0.0) } else { (0.0, s) };
        vec![(base, 1.0 - l - r), (left, l), (right, r)]
    }

    /// TdAnimNodeGrabbing (TickAnim 0x12118A0): hang / hang free, or while turned around
    /// (CurrentGrabTurnType End/Idle) the turn idles, blending to the 02 variants towards 180.
    fn grab_blend(&mut self, sim: &Sim) -> Vec<(&'static str, f32)> {
        use tdsim::moves::grab::GrabTurn;
        let free = sim.grab_is_hanging_free();
        let hang = || {
            if free {
                Self::grab_slope(sim, "HangFree", "hangfree45left", "hangfree45right")
            } else {
                Self::grab_slope(sim, "Hang", "hang45left", "hang45right")
            }
        };
        match sim.pawn.current_grab_turn_type {
            GrabTurn::None | GrabTurn::Start => {
                self.last_grab_turn.clear();
                hang()
            }
            GrabTurn::End | GrabTurn::Idle => {
                let dy = ((sim.pc.rotation.yaw - sim.pawn.rotation.yaw) as u16 as i16) as i32;
                let sta = sim.moves.grab.start_turning_angle;
                let t = ((dy.abs() as f32 - sta) / (32768.0 - sta)).clamp(0.0, 1.0);
                if dy as f32 > sta {
                    self.last_grab_turn = if free { hang() } else { vec![("HangTurnRightIdle", 1.0 - t), ("hangturnrightidle02", t)] };
                } else if (dy as f32) < -sta {
                    self.last_grab_turn = if free { hang() } else { vec![("HangTurnLeftIdle", 1.0 - t), ("hangturnleftidle02", t)] };
                }
                if self.last_grab_turn.is_empty() { hang() } else { self.last_grab_turn.clone() }
            }
        }
    }

    /// TdAnimNodeClimb (TickAnim 0x1213850): hand-over-hand idles, the slide when climbing down
    /// fast, or the look-around idles past StartTurningAngle. Children switch with no blend.
    fn climb_blend(&mut self, sim: &Sim) -> Vec<(&'static str, f32)> {
        let p = &sim.pawn;
        let sta = sim.moves.climb.start_turning_angle;
        let dy = ((sim.pc.rotation.yaw - p.rotation.yaw) as u16 as i16) as i32;
        let pipe = p.ladder_type == tdsim::ladder::LadderType::Pipe;
        let sliding = p.climb_down_fast && dy <= sta && dy >= -sta;
        if sliding && !self.was_climb_sliding {
            // TdAnimNodeRandom picks a child each time it becomes relevant
            self.climb_slide_pick = (self.climb_slide_pick + 1 + (sim.time * 1000.0) as usize) % 3;
        }
        self.was_climb_sliding = sliding;
        let name = if dy > sta {
            "ladderclimblookright"
        } else if dy < -sta {
            "ladderclimblookleft"
        } else if pipe {
            if p.climb_down_fast {
                ["PipeClimbDownFast", "pipeclimbdownfast02", "PipeClimbDownFast"][self.climb_slide_pick]
            } else if p.climb_left_hand {
                "PipeClimbUpLeftHandStill"
            } else {
                "PipeClimbUpRightHandStill"
            }
        } else if p.climb_down_fast {
            ["LadderClimbDownFast", "ladderclimbdownfast02", "ladderclimbdownfast03"][self.climb_slide_pick]
        } else if p.climb_left_hand {
            "LadderClimbUpLeftHandStill"
        } else {
            "LadderClimbUpRightHandStill"
        };
        if self.climb_active {
        }
        vec![(name, 1.0)]
    }

    /// What the locomotion part of AT_C1P shows for the pawn's state, as weighted clips.
    fn locomotion_blend(&mut self, sim: &Sim) -> Vec<(&'static str, f32)> {
        let p = &sim.pawn;
        let one = |n: &'static str| vec![(n, 1.0)];
        // TdAnimNodeMovementState (0x1210D40): AnimationMovementState when set, else MovementState
        let state = if p.animation_movement_state != Move::None { p.animation_movement_state } else { p.movement_state };
        match state {
            Move::Grabbing => self.grab_blend(sim),
            // TdAnimNodeMovementState_0 (bUseOldState, StateMapping [180TurnInAir])
            Move::FallingUncontrolled => one(if p.old_movement_state == Move::Turn180InAir { "fallinguncontrolledbwd" } else { "fallinguncontrolled" }),
            Move::SoftLanding => one("fallinglandintosoftlanding"),
            Move::Vertigo => one("edgedetectionidle"),
            // TdAnimNodeGrabTransfer (0x1211B60): by Moves[GrabTransfer].TransferHint
            Move::GrabTransfer => one(match sim.moves.grab_transfer.transfer_hint {
                tdsim::pawn::MoveActionHint::Left | tdsim::pawn::MoveActionHint::Right => "JumpLand",
                _ => "hangtransferupidle",
            }),
            Move::Climb => self.climb_blend(sim),
            // TdAnimNodeSequence_73 (looping, NormalizedStartPosition set by IntoZipLine)
            Move::ZipLine => one("ZipLine"),
            Move::Swing => Self::swing_blend(sim, false),
            Move::Balance => self.balance_blend(sim),
            Move::WallRunningLeft => one("WallrunLeft"),
            Move::WallRunningRight => one("WallrunRight"),
            Move::WallClimbing => one("WallRunVertical"),
            Move::LayOnGround => one("jumpturnlandingidle"),
            Move::Turn180InAir => one("jumpturnflyend"),
            // TdAnimNodeWalkingState_8: Idle -> TdAnimNodeTurn_18, else TdAnimNodeDirSwitch
            // on bGoingForward
            Move::Crouch | Move::Slide => one(if p.velocity_magnitude_2d < 10.0 {
                "crouchstill"
            } else if !p.going_forward {
                "crouchbwd"
            } else {
                "crouchfwd"
            }),
            _ if p.physics == Physics::Falling => one("jumpair"),
            // TdAnimNodeWalkingState_1
            _ => match p.current_walking_state {
                WalkingState::Idle | WalkingState::None => one("Stand"),
                // AT_C3P: TdAnimNodeDirSwitch on bGoingForward
                WalkingState::Sneak | WalkingState::Walk | WalkingState::Jog | WalkingState::Run if self.third_person.is_some() => {
                    let fwd = p.going_forward;
                    one(match p.current_walking_state {
                        WalkingState::Sneak => if fwd { "sneakfwd" } else { "sneakbwd" },
                        WalkingState::Walk => if fwd { "walkfwd" } else { "walkbwd" },
                        _ => if fwd { "runfwd" } else { "runbwd" },
                    })
                }
                WalkingState::Sneak => self.blend_directional(sim, 0, ["sneakfwd", "sneakfwd", "sneakfwd", "sneakbwd", "sneakbwd", "sneakbwd"]),
                WalkingState::Walk => self.blend_directional(sim, 1, ["walkfwd", "walkfwdstiff", "walkfwdstiff", "walkbwd", "walkbwdstiff", "walkbwdstiff"]),
                WalkingState::Jog | WalkingState::Run => self.blend_directional(sim, 2, ["runfwd", "runfwdstiff", "runfwdstiff", "runbwd", "runbwdstiff", "runbwdstiff"]),
                WalkingState::Sprint => one("SprintFwd"),
            },
        }
    }

    /// Clip by name, with stand-ins for clips one set lacks (the 3p set has no `jumpair`).
    fn seq_or_fallback(&self, name: &str) -> Option<usize> {
        self.seq(name).or_else(|| match name.to_ascii_lowercase().as_str() {
            "jumpair" => self.seq("jumpslow"),
            "ladderclimbdownfast02" | "ladderclimbdownfast03" => self.seq("LadderClimbDownFast"),
            "ladderclimblookleft" | "ladderclimblookright" => self.seq("LadderClimbUpLeftHandStill"),
            _ => None,
        })
    }

    /// Bone-to-mesh matrices for one clip at a normalized phase (debugging).
    pub fn clip_globals(&self, name: &str, phase: f32) -> Option<Vec<Mat4>> {
        let si = self.seq(name)?;
        let pose = self.sample(Some(si), phase * self.set.seqs[si].length);
        let mut g: Vec<Mat4> = Vec::with_capacity(pose.len());
        for (i, p) in pose.iter().enumerate() {
            let local = Mat4::from_rotation_translation(p.rot, p.pos);
            g.push(if i == 0 { local } else { g[self.parents[i]] * local });
        }
        Some(g)
    }

    /// AnimNodeSequence::IssueNotifies for sequence `si` between two play positions.
    fn fire_notifies(&self, sim: &mut Sim, si: usize, from: f32, to: f32, looping: bool) {
        let s = &self.set.seqs[si];
        let info = tdsim::anim::AnimSeqInfo { length: s.length, rate_scale: s.rate_scale, root: None, notifies: crate::anims::notifies(s) };
        let mut out = Vec::new();
        info.notifies_between(from, to, looping, &mut out);
        for n in out {
            sim.fire_notify(&n);
        }
    }

    /// Is bone `b` `root` or one of its descendants?
    fn is_under(&self, mut b: usize, root: usize) -> bool {
        loop {
            if b == root {
                return true;
            }
            if b == 0 {
                return false;
            }
            b = self.parents[b];
        }
    }

    /// Against-wall arm weights, left and right (debugging).
    pub fn wall_weights(&self) -> [f32; 2] {
        [self.wall_arms[0].weight, self.wall_arms[1].weight]
    }

    /// Names and weights of the current locomotion blend (debugging).
    pub fn locomotion_debug(&self) -> Vec<(String, f32)> {
        self.loco.clips.iter().map(|&(si, w)| (self.set.seqs[si].name.clone(), w)).collect()
    }

    fn sample_loco(&self, l: &Loco) -> Vec<BonePose> {
        let mut out: Option<Vec<BonePose>> = None;
        let mut acc = 0.0f32;
        for &(si, w) in &l.clips {
            if w <= 0.0001 {
                continue;
            }
            let p = self.sample(Some(si), l.phase * self.set.seqs[si].length);
            acc += w;
            out = Some(match out {
                None => p,
                Some(prev) => prev.iter().zip(&p).map(|(a, b)| BonePose::lerp(*a, *b, w / acc)).collect(),
            });
        }
        out.unwrap_or_else(|| self.ref_pose.clone())
    }

    /// Advance the locomotion layer and build the pose for this frame.
    pub fn update(&mut self, sim: &mut Sim, dt: f32) {
        self.update_pose_profile(sim);
        self.armed = sim.anim_weapon;
        // locomotion blend with a 0.2 s crossfade when the clips change, phase kept across
        self.next_loco_blend = None;
        self.dt = dt;
        for n in self.dir_nodes.iter_mut() {
            n.ticked = false;
        }
        // TdAnimNodeWalkingState_1 BlendWeight per child (Idle, Sneak, Walk, Jog, Run, Sprint),
        // SetActiveMove taking at least the 0.2 default
        let key = (if sim.pawn.animation_movement_state != Move::None { sim.pawn.animation_movement_state } else { sim.pawn.movement_state }, sim.pawn.current_walking_state);
        if let Some(last) = self.last_walk_key {
            if last.0 == Move::Walking && key.0 == Move::Walking && last.1 != key.1 {
                let bw = match key.1 {
                    WalkingState::Idle | WalkingState::None => 0.15,
                    WalkingState::Sneak => 0.1,
                    WalkingState::Walk => 0.35,
                    WalkingState::Jog => 0.3,
                    WalkingState::Run => 0.6,
                    WalkingState::Sprint => 0.8,
                };
                self.next_loco_blend = Some(f32::max(bw, 0.2));
            }
        }
        self.last_walk_key = Some(key);
        self.loco_rate = 1.0;
        let names = self.locomotion_blend(sim);
        if key.0 != Move::Balance {
            self.balance_relevant = false;
        }
        for n in self.dir_nodes.iter_mut() {
            n.relevant = n.ticked;
        }
        let s = if sim.pawn.animation_movement_state != Move::None { sim.pawn.animation_movement_state } else { sim.pawn.movement_state };
        self.climb_active = s == Move::Climb;
        let want: Vec<(usize, f32)> = names.iter().filter_map(|(n, w)| self.seq_or_fallback(n).map(|s| (s, *w))).collect();
        if !self.loco.same_clips(&want) {
            // SetActiveChild: the old child keeps its weight and blends out; a child still fading
            // out comes back from the weight it has now
            let old = std::mem::take(&mut self.loco);
            self.fading.push((old, self.loco_weight));
            let phase = self.fading.last().map(|f| f.0.phase).unwrap_or(0.0);
            if let Some(i) = self.fading.iter().position(|(l, _)| l.same_clips(&want)) {
                let (mut l, w) = self.fading.remove(i);
                l.clips = want;
                self.loco = l;
                self.loco_weight = w;
            } else {
                // a looping child starts at its NormalizedStartPosition when it becomes relevant
                let phase = if s == Move::ZipLine { sim.moves.zipline.idle_start_position } else { phase };
                self.loco = Loco { clips: want, phase };
                self.loco_weight = 0.0;
            }
            self.loco_blend_time = self.next_loco_blend.unwrap_or(0.2);
            self.loco_blend_left = self.loco_blend_time;
        } else {
            self.loco.clips = want;
        }
        // child weights move towards their targets over the remaining blend time
        if self.loco_blend_left > 0.0 {
            let f = (dt / self.loco_blend_left).min(1.0);
            self.loco_weight += (1.0 - self.loco_weight) * f;
            for (_, w) in self.fading.iter_mut() {
                *w -= *w * f;
            }
            self.loco_blend_left -= dt;
        } else {
            self.loco_weight = 1.0;
            self.fading.clear();
        }
        self.fading.retain(|(_, w)| *w > 1e-3);
        let rate = self.loco_rate;
        let advance = |l: &mut Loco, set: &AnimSet| {
            if let Some(&(si, _)) = l.clips.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
                let s = &set.seqs[si];
                l.phase = (l.phase + dt * rate * s.rate_scale.max(1e-3) / s.length.max(1e-3)).rem_euclid(1.0);
            }
        };
        // AnimNodeSynch: only the group's master (the heaviest sequence) issues notifies
        // (bFireSlaveNotifies is off)
        let master = {
            let mut best: Option<(f32, usize, f32)> = None;
            let sets = std::iter::once((&self.loco, self.loco_weight)).chain(self.fading.iter().map(|(l, w)| (l, *w)));
            for (l, w) in sets {
                for &(si, cw) in &l.clips {
                    let ew = cw * w;
                    if best.is_none_or(|b| ew > b.0) {
                        best = Some((ew, si, l.phase));
                    }
                }
            }
            best
        };
        advance(&mut self.loco, &self.set);
        for (l, _) in self.fading.iter_mut() {
            advance(l, &self.set);
        }
        if self.drives_phase {
            sim.anim.locomotion_phase = self.loco.phase;
            if let Some((w, si, old_phase)) = master.filter(|m| m.0 > 0.0) {
                let _ = w;
                let s = &self.set.seqs[si];
                let mut new_phase = old_phase + dt * rate.max(0.0) * s.rate_scale.max(1e-3) / s.length.max(1e-3);
                if new_phase < old_phase {
                    new_phase += 1.0;
                }
                self.fire_notifies(sim, si, old_phase * s.length, new_phase * s.length, true);
            }
        }
        let mut pose = self.sample_loco(&self.loco);
        let mut total = self.loco_weight;
        for (l, w) in &self.fading {
            total += *w;
            let other = self.sample_loco(l);
            let t = if total > 0.0 { *w / total } else { 1.0 };
            for (b, p) in pose.iter_mut().enumerate() {
                *p = BonePose::lerp(*p, other[b], t);
            }
        }
        // TdAnimNodeSwing's Middle child blends per bone (EyeJoint branch), under the slots
        if s == Move::Swing && self.loco_weight > 0.0 {
            self.swing_eye_branch(sim, &mut pose);
        }
        // AnimNodeBlendPerBone_6 over the balance walk: the legs (Hips branch) walk the beam
        if s == Move::Balance && self.loco_weight > 0.0 {
            if let (Some(hips), Some(si)) = (self.bone_index("Hips"), self.seq("walkbalancefwd")) {
                let legs = self.sample_loco(&Loco { clips: vec![(si, 1.0)], phase: self.loco.phase });
                for b in 0..pose.len() {
                    if self.is_under(b, hips) {
                        pose[b] = BonePose::lerp(pose[b], legs[b], self.loco_weight);
                    }
                }
            }
        }
        // TdAnimNodeTurn under the idle (children blend in 0.1 s)
        let tn = &sim.turn_node;
        let turning = sim.turn_node_set().filter(|_| tn.relevant && tn.active_child != 0).map(|set| (set[tn.active_child], tn.position));
        if let Some((name, pos)) = turning {
            // the turn step's own notifies (it isn't in the synch group)
            if self.drives_phase {
                let from = match self.turn_clip {
                    Some((n, p)) if n == name && p <= pos => p,
                    _ => 0.0,
                };
                if let Some(si) = self.seq(name) {
                    self.fire_notifies(sim, si, from, pos, false);
                }
            }
            self.turn_clip = turning;
        }
        let target = if turning.is_some() { 1.0 } else { 0.0 };
        self.turn_weight += (target - self.turn_weight).clamp(-dt / 0.1, dt / 0.1);
        if self.turn_weight > 0.001 {
            if let Some((name, pos)) = self.turn_clip {
                let t = self.sample(self.seq(name), pos);
                // standing: AnimNodeBlendPerBone_1 takes the turn only from Hips down (the arms,
                // EyeJoint and CameraJoint keep Stand); crouched, TdAnimNodeTurn_18 is the
                // whole idle
                let hips = if name.starts_with("Stand") { self.bone_index("Hips") } else { None };
                for (b, p) in pose.iter_mut().enumerate() {
                    if hips.is_some_and(|h| !self.is_under(b, h)) {
                        continue;
                    }
                    *p = BonePose::lerp(*p, t[b], self.turn_weight);
                }
            }
        }
        // TdAnimNodeAimOffset_2 (WalkingState Default = idle): GetAim 0x1211E50 eases Aim.X
        // towards WantedAiming (TickAnim 0x1212040: view yaw off LegRotation) at speed 7; Aim.Y
        // stays at the node's -0.103; UAnimNodeAimOffset::GetBoneAtoms (0xCFA8B0) divides the
        // aim by the profile's range ends (HorizontalRange +-0.2) and clamps it to +-1
        let idle = sim.turn_node_set().is_some_and(|s| s[0] == "Stand");
        if idle && self.idle_aim_weight <= 0.001 {
            // OnBecameRelevant
            self.idle_interp = 0.0;
        }
        let target = if idle { 1.0 } else { 0.0 };
        self.idle_aim_weight += (target - self.idle_aim_weight).clamp(-dt / 0.15, dt / 0.15);
        if self.idle_aim_weight > 0.001 && !self.idle_aim.is_empty() {
            let wanted = tdsim::math::norm_axis(sim.pc.rotation.yaw - sim.pawn.leg_rotation) as f32 / 16384.0;
            let d = wanted - self.idle_interp;
            self.idle_interp = if d * d < 1e-8 { wanted } else { self.idle_interp + d * (dt * 7.0).clamp(0.0, 1.0) };
            let aim = ((self.idle_interp / 0.2).clamp(-1.0, 1.0), -0.102_941_155);
            Self::apply_aim(&self.parents, &self.idle_aim, aim, self.idle_aim_weight, &mut pose);
        }
        // AT_C1P below MasterSync: Custom_FullBody_Dir, then TdAnimNodeMovementState_1's
        // DirBone (Walking, Jump, Falling, Crouch, Vertigo), then Custom_FullBody, then the
        // AgainstWallState DirBone, and Custom_Canned over everything
        self.apply_slot(sim, Slot::FullBodyDir, &mut pose);
        // AnimNodeBlendPerBone_7 (the Walking branch): Custom_LowerBody over Hips and EyeJoint
        if self.third_person.is_none() {
            self.apply_slot_branches(&sim.anim, Slot::LowerBody, &["Hips", "EyeJoint"], &mut pose);
        }
        let s = if sim.pawn.animation_movement_state != Move::None { sim.pawn.animation_movement_state } else { sim.pawn.movement_state };
        let target = if matches!(s, Move::Walking | Move::Jump | Move::Falling | Move::Crouch | Move::Vertigo) { 1.0 } else { 0.0 };
        self.hip_aim_weight += (target - self.hip_aim_weight).clamp(-dt / 0.2, dt / 0.2);
        if self.hip_aim_weight > 0.001 && !self.hip_aim.is_empty() && self.third_person.is_none() {
            let aim = (Self::leg_aim(sim), 0.0);
            Self::apply_aim(&self.parents, &self.hip_aim, aim, self.hip_aim_weight, &mut pose);
        }
        self.apply_slot(sim, Slot::FullBody, &mut pose);
        // UpperBodySplit (AnimNodeBlendPerBone_0): Custom_UpperBody over SpineX and EyeJoint
        if self.third_person.is_none() {
            self.apply_slot_branches(&sim.anim, Slot::UpperBody, &["SpineX", "EyeJoint"], &mut pose);
        }
        if let Some(tp) = self.third_person.as_mut() {
            // AgainstWallState_2: Default -> "againstwall"; None -> MoveSwitch (Walking, Jump,
            // Falling, Vertigo -> "WalkRelaxed", Crouch -> "CrouchWalkRelaxed")
            let wall = sim.pawn.against_wall_state != tdsim::body::AgainstWall::None;
            let targets = [
                (!wall && matches!(s, Move::Walking | Move::Jump | Move::Falling | Move::Vertigo)) as i32 as f32,
                (!wall && s == Move::Crouch) as i32 as f32,
                wall as i32 as f32,
            ];
            for (w, t) in tp.weights.iter_mut().zip(targets) {
                *w += (t - *w).clamp(-dt / 0.2, dt / 0.2);
            }
            let pitch = tdsim::math::norm_axis(sim.pc.rotation.pitch - sim.pawn.rotation.pitch) as f32 / 16384.0;
            // GetAim (0x120E130): bInvertXAxis (set on WalkRelaxed / CrouchWalkRelaxed) negates X
            let x = Self::leg_aim(sim);
            for (comps, w, invert) in [(&tp.walk, tp.weights[0], true), (&tp.crouch, tp.weights[1], true), (&tp.wall, tp.weights[2], false)] {
                if w > 0.001 && !comps.is_empty() {
                    let aim = (if invert { -x } else { x }, pitch);
                    Self::apply_aim(&self.parents, comps, aim, w, &mut pose);
                }
            }
        }
        let target = if sim.pawn.against_wall_state != tdsim::body::AgainstWall::None { 1.0 } else { 0.0 };
        self.wall_cam_weight += (target - self.wall_cam_weight).clamp(-dt / 0.2, dt / 0.2);
        if self.wall_cam_weight > 0.001 && !self.wall_cam_aim.is_empty() {
            // bUsePitch: the view pitch relative to the pawn
            let pitch = tdsim::math::norm_axis(sim.pc.rotation.pitch - sim.pawn.rotation.pitch) as f32 / 16384.0;
            let aim = (Self::leg_aim(sim), pitch);
            Self::apply_aim(&self.parents, &self.wall_cam_aim, aim, self.wall_cam_weight, &mut pose);
        }
        // ArmedLeft / ArmedRight: the "againstwall" arms over each side's branch; ArmedRight's
        // Default child is Custom_Weapon (the gun's fire / out-of-ammo anims)
        self.against_wall_arms(sim, dt, &mut pose);
        {
            let right = self.wall_branches[1].clone();
            // WeaponTypeSwitch: a ready gun's arm plays DefaultWalkingState (standready /
            // walkfwdready / runfwdready) under Custom_Weapon; a heavy gun's WeaponType node
            // routes the left arm (ArmedLeft) through Custom_Weapon too
            let mut arms = right.clone();
            if sim.heavy_weapon() {
                arms.extend(self.wall_branches[0].iter().copied());
            }
            self.ready_arm(sim, dt, &arms, &mut pose);
            if sim.heavy_weapon() {
                let left = self.wall_branches[0].clone();
                self.apply_slot_branches(&sim.anim, Slot::Weapon, &left, &mut pose);
            }
            self.apply_slot_branches(&sim.anim, Slot::Weapon, &right, &mut pose);
            // Custom_CannedUpperBody (throwaway, unholster) over the upper body
            let upper: &[&str] = if self.third_person.is_some() { &["Spine1"] } else { &["SpineX", "EyeJoint"] };
            self.apply_slot_branches(&sim.anim, Slot::CannedUpperBody, upper, &mut pose);
        }
        self.apply_slot(sim, Slot::Canned, &mut pose);
        // skeletal controls after the tree, parents first: the arm aim on SpineXLeft /
        // SpineXRight, then TdSkelControlAgainstWall on each hand
        self.root_controls(sim, &mut pose);
        if self.third_person.is_none() {
            self.arm_aim(sim, dt, &mut pose);
            self.recoil(sim, dt, &mut pose);
        }
        self.against_wall_ik(sim, dt, &mut pose);
        self.pose = pose;
    }

    /// Load the TdAnimNodeWeaponPoseOffset profiles (anims::load_weapon_pose_profiles).
    pub fn set_weapon_pose_profiles(&mut self, p: &std::collections::HashMap<String, Vec<(String, [f32; 3], [f32; 4])>>) {
        self.weapon_pose_profiles = p
            .iter()
            .map(|(n, v)| (n.clone(), v.iter().filter_map(|(b, t, q)| Some((self.bone_index(b)?, ue_pos(*t), ue_anim_rot(*q)))).collect()))
            .collect();
    }

    /// The WeaponPoseOffset1p profile (TdPawn.UpdateWeaponPoseProfile: the gun's
    /// WeaponPoseProfileName), off during the disarm (TdMOVE_Disarm.StartMove bDisable).
    fn update_pose_profile(&mut self, sim: &Sim) {
        self.active_pose_profile = if self.third_person.is_some() || sim.pawn.movement_state == tdsim::Move::Snatch {
            None
        } else {
            sim.weapon.as_ref().map(|w| w.class.pose_profile)
        };
    }

    /// TdAnimNodeAgainstWallState_1 (left: AW_AgainstWall / AW_AgainstWallLeft) and _0 (right:
    /// AW_AgainstWall / AW_AgainstWallRight), BlendWeight 0.35 in and 0.55 back out, picking the
    /// looping "againstwall" clip for the arm branches of AnimNodeBlendPerBone_5 (SpineXLeft,
    /// LeftHand_GameIK) and _2 (SpineXRight, RightHand_GameIK, CameraJoint).
    /// TdSkelControlRecoil (TdSkelControlLimb on the right arm): the hand pulled along its own
    /// X axis (BCS_BoneSpace effector) by EffectorLocation.X. UpdateWeaponSkelControls turns it
    /// on (0.2 s) while a light gun is ready.
    fn recoil(&mut self, sim: &Sim, dt: f32, pose: &mut [BonePose]) {
        use tdsim::weapons::WeaponAnimState as W;
        let on = sim.weapon.is_some() && sim.weapon_anim_state == W::Ready;
        let target = if on { 1.0 } else { 0.0 };
        self.recoil_weight += (target - self.recoil_weight).clamp(-dt / 0.2, dt / 0.2);
        if self.recoil_weight <= 0.001 {
            return;
        }
        let (Some(a), Some(b), Some(c)) = (self.bone_index("RightArm"), self.bone_index("RightForeArm"), self.bone_index("RightHand")) else { return };
        let mut g: Vec<Mat4> = Vec::with_capacity(pose.len());
        for (i, p) in pose.iter().enumerate() {
            let local = Mat4::from_rotation_translation(p.rot, p.pos);
            g.push(if i == 0 { local } else { g[self.parents[i]] * local });
        }
        let sw = |v: GVec3| GVec3::new(v.x, v.z, v.y);
        let hand = sw(g[c].w_axis.truncate()) * 100.0;
        let x_axis = sw(g[c].x_axis.truncate()).normalize_or_zero();
        let effector = hand + x_axis * sim.recoil_x;
        let w = self.recoil_weight;
        self.limb_ik([a, b, c], effector, true, w, pose);
    }

    /// A weapon mesh's pose: its reference pose with one of the owner's slots (the gun plays
    /// the same sequence names: standfire / weaponposeempty on Custom_Weapon, and a disarmed
    /// cop's snatch through TdWeapon.PlayCustomWeaponAnimation).
    ///
    /// `base` is the looping clip the owner's arm plays under its slots (standready /
    /// walkfwdready / runfwdready for Faith's ready arm, Stand for a cop): the gun's anim tree
    /// plays the same sequences, which pose its own bones (the Minimi's belt is modeled
    /// sticking up and only hangs right under them).
    pub fn update_weapon_mesh(&mut self, anim: &tdsim::anim::AnimPlayer, slot: Slot, base: Option<&str>, dt: f32) {
        let mut pose = match base.and_then(|n| self.seq(n)) {
            Some(si) => {
                let len = self.set.seqs[si].length.max(1e-3);
                self.ready_time = (self.ready_time + dt) % len;
                self.sample(Some(si), self.ready_time)
            }
            None => self.ref_pose.clone(),
        };
        self.apply_slot_from(anim, slot, &mut pose);
        self.pose = pose;
    }

    /// AT_Weapons.AT_C1P_Weapon_Default under its Custom_Weapon slot: TdAnimNodeWeaponTypeState
    /// (Heavy -> TdAnimNodeWeaponState: Ready -> TdAnimNodeWalkingState: Jog / Run / Sprint
    /// runfwdready), every other branch WeaponPose.
    pub fn gun_base_clip_1p(sim: &Sim) -> &'static str {
        use tdsim::pawn::WalkingState as WS;
        use tdsim::weapons::WeaponAnimState as W;
        let ready = matches!(sim.weapon_anim_state, W::Ready | W::Reload | W::Throwing);
        if sim.heavy_weapon() && ready && matches!(sim.pawn.current_walking_state, WS::Jog | WS::Run | WS::Sprint) {
            "runfwdready"
        } else {
            "WeaponPose"
        }
    }

    fn ready_arm(&mut self, sim: &Sim, dt: f32, branches: &[&str], pose: &mut [BonePose]) {
        use tdsim::weapons::WeaponAnimState as W;
        let ready = sim.weapon.is_some() && matches!(sim.weapon_anim_state, W::Ready | W::Reload | W::Throwing);
        let target = if ready { 1.0 } else { 0.0 };
        self.ready_weight += (target - self.ready_weight).clamp(-dt / 0.2, dt / 0.2);
        if self.ready_weight <= 0.001 {
            self.ready_time = 0.0;
            return;
        }
        // TdAnimNodeWalkingState_2: Idle -> standready, Walk / Sneak -> walkfwdready, else runfwdready
        use tdsim::pawn::WalkingState as WS;
        let name = match sim.pawn.current_walking_state {
            WS::Idle => "standready",
            WS::Walk | WS::Sneak => "walkfwdready",
            _ => "runfwdready",
        };
        let Some(si) = self.seq(name) else { return };
        let len = self.set.seqs[si].length.max(1e-3);
        self.ready_time = (self.ready_time + dt) % len;
        let clip = self.sample(Some(si), self.ready_time);
        let starts: Vec<usize> = branches.iter().filter_map(|n| self.bone_index(n)).collect();
        let w = self.ready_weight;
        for b in 0..pose.len() {
            if starts.iter().any(|&r| self.is_under(b, r)) {
                pose[b] = BonePose::lerp(pose[b], clip[b], w);
            }
        }
    }

    fn against_wall_arms(&mut self, sim: &Sim, dt: f32, pose: &mut [BonePose]) {
        use tdsim::body::AgainstWall as W;
        let s = sim.pawn.against_wall_state;
        let on = [matches!(s, W::AgainstWall | W::Left), matches!(s, W::AgainstWall | W::Right)];
        for (arm, on) in self.wall_arms.iter_mut().zip(on) {
            let (target, time) = if on { (1.0, 0.35) } else { (0.0, 0.55) };
            arm.weight += (target - arm.weight).clamp(-dt / time, dt / time);
        }
        if self.wall_arms.iter().all(|a| a.weight <= 0.001) {
            self.wall_clip_time = 0.0;
            return;
        }
        let Some(si) = self.seq("againstwall") else { return };
        let len = self.set.seqs[si].length.max(1e-3);
        self.wall_clip_time = (self.wall_clip_time + dt) % len;
        let clip = self.sample(Some(si), self.wall_clip_time);
        for (arm, names) in self.wall_arms.iter().zip(&self.wall_branches) {
            if arm.weight <= 0.001 {
                continue;
            }
            let starts: Vec<usize> = names.iter().filter_map(|n| self.bone_index(n)).collect();
            for b in 0..pose.len() {
                let mut a = b;
                let inside = loop {
                    if starts.contains(&a) {
                        break true;
                    }
                    if a == 0 {
                        break false;
                    }
                    a = self.parents[a];
                };
                if inside {
                    pose[b] = BonePose::lerp(pose[b], clip[b], arm.weight);
                }
            }
        }
    }

    /// TdAnimNodeBalanceWalk (TickAnim 0x12132B0): the lose-balance clip for the side you lean
    /// to while in danger, else TdAnimNodeBalanceBlend (0x12135C0): lean left / walk / lean
    /// right by its own BalanceFactor, which follows the move's plus a random lean (a kick of
    /// 0.25 either way on 2 rand() % 10 results a tick, decaying otherwise). The walk clips play
    /// at ground speed / 220 (ScalePlayRateBySpeed, RateMin -2).
    fn balance_blend(&mut self, sim: &Sim) -> Vec<(&'static str, f32)> {
        let b = &sim.moves.balance;
        if b.danger && b.balance_factor < 0.0 {
            return vec![("walkbalancelosebalanceleft", 1.0)];
        }
        if b.danger && b.balance_factor > 0.0 {
            return vec![("walkbalancelosebalanceright", 1.0)];
        }
        if !self.balance_relevant {
            // OnBecomeRelevant
            self.balance_node = [0.0; 3];
            self.balance_relevant = true;
        }
        let dt = self.dt;
        // MSVC rand(): 15 bits of an LCG
        self.rng = self.rng.wrapping_mul(214_013).wrapping_add(2_531_011);
        let r = (self.rng >> 16) & 0x7FFF;
        let [mut bf, mut target, mut lean] = self.balance_node;
        target = match r % 10 {
            1 => target + 0.25,
            2 => target - 0.25,
            _ => target - target * dt * 0.5,
        };
        lean += (target - lean) * dt * 0.5;
        bf += (lean + b.balance_factor - bf) * (dt * 3.5).min(1.0);
        self.balance_node = [bf, target, lean];
        let p = &sim.pawn;
        let speed = tdsim::math::UeVec::size_2d(p.velocity) * if p.velocity.dot(p.rotation.vector()) < 0.0 { -1.0 } else { 1.0 };
        self.loco_rate = (speed / 220.0).max(-2.0);
        vec![("walkbalancefwdleanleft", bf.min(0.0).abs()), ("walkbalancefwd", 1.0 - bf.abs()), ("walkbalancefwdleanright", bf.max(0.0))]
    }

    /// TdAnimNodeSwing (TickAnim 0x1213950): Front / Middle / back by SwingAngle * 2 / pi
    /// (front and back clamped to 0..1, Middle the rest). Middle is AnimNodeBlendPerBone_14:
    /// AnimNodeCrossfader_1 (swingposefronttop, swingposebackstraight at its saved 0.256) with
    /// the EyeJoint branch taking swingposebackstraight at 0.80; `eye` gives that branch's mix.
    fn swing_blend(sim: &Sim, eye: bool) -> Vec<(&'static str, f32)> {
        let a = sim.moves.swing.swing_angle * 2.0 / std::f32::consts::PI;
        let front = a.clamp(0.0, 1.0);
        let back = (-a).clamp(0.0, 1.0);
        let middle = (1.0 - front - back).max(0.0);
        let cross = 0.255_639_1;
        let (mf, mb) = if eye { ((1.0 - cross) * (1.0 - 0.801_324_5), cross * (1.0 - 0.801_324_5) + 0.801_324_5) } else { (1.0 - cross, cross) };
        vec![("swingposefronttop", front + middle * mf), ("swingposebackstraight", middle * mb), ("swingposebacktop", back)]
    }

    /// The swing's EyeJoint branch: the same clips with the per-bone Middle weights.
    fn swing_eye_branch(&self, sim: &Sim, pose: &mut [BonePose]) {
        let Some(eye) = self.bone_index("EyeJoint") else { return };
        let clips: Vec<(usize, f32)> = Self::swing_blend(sim, true).iter().filter_map(|(n, w)| self.seq(n).map(|s| (s, *w))).collect();
        let branch = self.sample_loco(&Loco { clips, phase: self.loco.phase });
        for b in 0..pose.len() {
            if self.is_under(b, eye) {
                pose[b] = BonePose::lerp(pose[b], branch[b], self.loco_weight);
            }
        }
    }

    /// The root bone's controls in chain order: SwingControl (TdMove_Swing.SetPawnRotation: roll
    /// in bone space replacing the root's own, translation added in actor space) and
    /// RootControl when SetRootOffset asked for bone space (the swing's grip offset).
    fn root_controls(&self, sim: &Sim, pose: &mut [BonePose]) {
        let p = &sim.pawn;
        let swap = |v: tdsim::Vec3| GVec3::new(v.x, v.z, v.y);
        let sc = p.swing_control;
        if sc.strength > 0.0 {
            let (x, y, z) = tdsim::Rotator::new(0, 0, sc.roll).axes();
            let r = Quat::from_mat3(&glam::Mat3::from_cols(swap(x), swap(z), swap(y))).normalize();
            let (ox, oy, oz) = self.mesh_rot_origin.axes();
            let to_mesh = glam::Mat3::from_cols(swap(ox), swap(oz), swap(oy)).transpose();
            let t = to_mesh * (swap(sc.translation) * 0.01);
            let new = BonePose { pos: pose[0].pos + t, rot: (pose[0].rot * r).normalize() };
            pose[0] = BonePose::lerp(pose[0], new, sc.strength.min(1.0));
        }
        if p.root_offset_space == tdsim::pawn::BoneControlSpace::Bone && p.root_offset_strength > 0.0 {
            let off = pose[0].rot * ue_pos([p.root_offset.x, p.root_offset.y, p.root_offset.z]);
            pose[0].pos += off * p.root_offset_strength;
        }
    }

    /// TdSkelControlAim1p: TickSkelControl (0x1222D50) blends each side in while
    /// TdPawn::GetAimMode(true) frees that hand (0.5 s; 0.1 s in melee), and
    /// UpdateTransformation (0x12220C0) turns the arm root in component space by
    /// Rotator(Pitch = view yaw off the pawn, Roll = -view pitch) and adds the swan neck offset
    /// (world space), all through SkelControlSingleBone (0xD134C0: bAddRotation keeps the
    /// bone's position, the rotation applies after the bone's own).
    fn arm_aim(&mut self, sim: &Sim, dt: f32, pose: &mut [BonePose]) {
        use tdsim::moves::aim::AimMode;
        let mode = sim.aim_mode(true);
        let melee = matches!(sim.pawn.movement_state, Move::Melee | Move::MeleeCrouch);
        for (i, ctl) in self.arm_aim.iter_mut().enumerate() {
            let right = i == 1;
            let on = mode == AimMode::TwoHanded || mode == if right { AimMode::Right } else { AimMode::Left };
            let want = if on { 1.0 } else { 0.0 };
            if ctl.strength != want {
                ctl.set_strength(want, if melee { 0.1 } else { 0.5 });
            }
            ctl.tick(dt);
        }
        if self.arm_aim.iter().all(|a| a.strength <= 0.0) {
            return;
        }
        let d = (sim.pc.rotation - sim.pawn.rotation).normalize();
        let rot = tdsim::Rotator::new(d.yaw, 0, -sim.pc.rotation.pitch);
        let swap = |v: tdsim::Vec3| GVec3::new(v.x, v.z, v.y);
        let (x, y, z) = rot.axes();
        let r = Quat::from_mat3(&glam::Mat3::from_cols(swap(x), swap(z), swap(y))).normalize();
        let to_mesh = mesh_to_world(sim, self.mesh_rot_origin, self.mesh_origin).inverse();
        let add = to_mesh.transform_vector3(swap(self.swan_world) * 0.01);
        for (i, name) in ["SpineXLeft", "SpineXRight"].iter().enumerate() {
            let strength = self.arm_aim[i].strength;
            let Some(b) = self.bone_index(name).filter(|_| strength > 0.0) else { continue };
            let mut parent = Mat4::IDENTITY;
            let mut a = self.parents[b];
            let mut chain = vec![];
            while a != 0 {
                chain.push(a);
                a = self.parents[a];
            }
            chain.push(0);
            for &c in chain.iter().rev() {
                parent = parent * Mat4::from_rotation_translation(pose[c].rot, pose[c].pos);
            }
            let g = parent * Mat4::from_rotation_translation(pose[b].rot, pose[b].pos);
            let (_, gr, gt) = g.to_scale_rotation_translation();
            let ng = Mat4::from_rotation_translation((r * gr).normalize(), gt + add);
            let (_, lr, lt) = (parent.inverse() * ng).to_scale_rotation_translation();
            pose[b] = BonePose::lerp(pose[b], BonePose { pos: lt, rot: lr }, strength);
        }
    }

    /// TdSkelControlAgainstWall (TickSkelControl 0x12246B0) + SkelControlLimb: the effector
    /// eases (VInterpTo, speed 6) towards the hand spot CheckAgainstWall found, plus HandOffset,
    /// in mesh space and clamped to Min/MaxLocation; a two-bone IK reaches the arm to it at the
    /// strength of the AgainstWallLeft / AgainstWallRight nodes.
    fn against_wall_ik(&mut self, sim: &Sim, dt: f32, pose: &mut [BonePose]) {
        use tdsim::body::AgainstWall as W;
        let s = sim.pawn.against_wall_state;
        let m = mesh_to_world(sim, self.mesh_rot_origin, self.mesh_origin);
        let to_mesh = m.inverse();
        for i in 0..2 {
            let arm = &mut self.wall_arms[i];
            let applies = s == W::AgainstWall || (arm.left && s == W::Left) || (!arm.left && s == W::Right);
            if applies {
                let hand = if arm.left { sim.pawn.against_wall_left_hand } else { sim.pawn.against_wall_right_hand };
                let w = hand + tdsim::Vec3::new(0.0, 0.0, -20.0);
                let g = to_mesh.transform_point3(GVec3::new(w.x, w.z, w.y) * 0.01);
                let ue = GVec3::new(g.x, g.z, g.y) * 100.0;
                arm.target = ue.clamp(arm.min, arm.max);
            }
            // VInterpTo
            let d = arm.target - arm.effector;
            arm.effector = if d.length_squared() < 1e-4 { arm.target } else { arm.effector + d * (dt * 6.0).clamp(0.0, 1.0) };
        }
        for i in 0..2 {
            let arm = self.wall_arms[i].clone();
            if arm.weight <= 0.001 {
                continue;
            }
            let names = if arm.left { ["LeftArm", "LeftForeArm", "LeftHand"] } else { ["RightArm", "RightForeArm", "RightHand"] };
            let (Some(a), Some(b), Some(c)) = (self.bone_index(names[0]), self.bone_index(names[1]), self.bone_index(names[2])) else { continue };
            self.limb_ik([a, b, c], arm.effector, !arm.left, arm.weight, pose);
        }
    }

    /// USkelControlLimb::CalculateNewBoneTransforms in Unreal mesh space: BoneAxis X (inverted
    /// on the right arm), JointAxis Y, joint target at the elbow (ParentBoneSpace, zero offset).
    fn limb_ik(&self, bones: [usize; 3], effector: GVec3, invert_bone_axis: bool, strength: f32, pose: &mut [BonePose]) {
        let mut g: Vec<Mat4> = Vec::with_capacity(pose.len());
        for (i, p) in pose.iter().enumerate() {
            let local = Mat4::from_rotation_translation(p.rot, p.pos);
            g.push(if i == 0 { local } else { g[self.parents[i]] * local });
        }
        let s = |v: GVec3| GVec3::new(v.x, v.z, v.y);
        let pos = |i: usize| s(g[i].w_axis.truncate()) * 100.0;
        let (root, joint, end) = (pos(bones[0]), pos(bones[1]), pos(bones[2]));
        let delta = effector - root;
        let len = delta.length();
        let dir = if len > 1e-8 { delta / len } else { GVec3::X };
        let jt = joint - root;
        let (plane, bend) = {
            let jd = jt.normalize_or_zero();
            let n = dir.cross(jd);
            if n.length_squared() < 1e-8 {
                dir.any_orthonormal_pair()
            } else {
                (n.normalize(), (jt - dir * jt.dot(dir)).normalize())
            }
        };
        let upper = (joint - root).length();
        let lower = (end - joint).length();
        let (new_joint, new_end) = if len > upper + lower {
            (root + dir * upper, root + dir * (upper + lower))
        } else {
            let two_ab = 2.0 * upper * len;
            let cos = if two_ab != 0.0 { (upper * upper + len * len - lower * lower) / two_ab } else { 0.0 };
            let angle = cos.clamp(-1.0, 1.0).acos();
            let line = upper * angle.sin();
            let mut proj = (upper * upper - line * line).max(0.0).sqrt();
            if cos < 0.0 {
                proj = -proj;
            }
            (root + dir * proj + bend * line, effector)
        };
        let sign = if invert_bone_axis { -1.0 } else { 1.0 };
        // BuildMatrixFromVectors(X = limb dir, Y = plane normal) -> Z = X ^ Y; back to glTF axes
        let frame = |d: GVec3, at: GVec3| {
            let x = d.normalize_or_zero() * sign;
            let y = plane;
            let z = x.cross(y);
            let rot = glam::Mat3::from_cols(s(x), s(z), s(y));
            Mat4::from_rotation_translation(Quat::from_mat3(&rot).normalize(), s(at) * 0.01)
        };
        let mut ng = g.clone();
        ng[bones[0]] = frame(new_joint - root, root);
        ng[bones[1]] = frame(new_end - new_joint, new_joint);
        let (_, end_rot, _) = g[bones[2]].to_scale_rotation_translation();
        ng[bones[2]] = Mat4::from_rotation_translation(end_rot, s(new_end) * 0.01);
        for &b in &bones {
            let l = ng[self.parents[b]].inverse() * ng[b];
            let (_, r, t) = l.to_scale_rotation_translation();
            pose[b] = BonePose::lerp(pose[b], BonePose { pos: t, rot: r }, strength.clamp(0.0, 1.0));
        }
    }

    /// TdAnimNodeDirBone::TickAnim (0x1212E30) Aim.X: LegRotation off the pawn's yaw over the
    /// GoBackLegAngleLimit range, held to 0.7 while crouched.
    fn leg_aim(sim: &Sim) -> f32 {
        let p = &sim.pawn;
        let (min, max) = (p.go_back_leg_angle_limit_min, p.go_back_leg_angle_limit_max);
        let d = tdsim::math::norm_axis(p.leg_rotation - p.rotation.yaw).clamp(min, max);
        let range = if d < 0 { min.abs() } else { max }.max(1);
        let lim = if p.movement_state == Move::Crouch { 0.7 } else { 1.0 };
        (d as f32 / range as f32).clamp(-lim, lim)
    }

    /// AnimNodeAimOffset: bilinear pick of the nine directional offsets for `aim` (X left-right,
    /// Y down-up, each -1..1), applied in mesh space bone by bone (parents first, so a child
    /// offset acts on top of its parent's), then weighed in.
    fn apply_aim(parents: &[usize], comps: &[(usize, AimComponent)], aim: (f32, f32), weight: f32, pose: &mut [BonePose]) {
        let (x, y) = (aim.0.clamp(-1.0, 1.0), aim.1.clamp(-1.0, 1.0));
        // grid index: column L=0 C=1 R=2, row U=0 C=1 D=2 -> LU LC LD CU CC CD RU RC RD
        let at = |c: usize, r: usize| c * 3 + r;
        let (c2, tx) = if x >= 0.0 { (2, x) } else { (0, -x) };
        let (r2, ty) = if y >= 0.0 { (0, y) } else { (2, -y) };
        let before: Vec<BonePose> = pose.to_vec();
        let mut global: Vec<Mat4> = Vec::with_capacity(pose.len());
        for i in 0..pose.len() {
            let local = Mat4::from_rotation_translation(pose[i].rot, pose[i].pos);
            let parent = if i == 0 { Mat4::IDENTITY } else { global[parents[i]] };
            let mut g = parent * local;
            if let Some((_, c)) = comps.iter().find(|(b, _)| *b == i) {
                let q = |k: usize| aim_rot(c.rot[k]);
                let t = |k: usize| ue_pos(c.trans[k]);
                let qa = q(at(1, 1)).slerp(q(at(c2, 1)), tx);
                let qb = q(at(1, r2)).slerp(q(at(c2, r2)), tx);
                let rot = qa.slerp(qb, ty);
                let ta = t(at(1, 1)).lerp(t(at(c2, 1)), tx);
                let tb = t(at(1, r2)).lerp(t(at(c2, r2)), tx);
                let tr = ta.lerp(tb, ty);
                let (_, gr, gt) = g.to_scale_rotation_translation();
                g = Mat4::from_rotation_translation((rot * gr).normalize(), gt + tr);
                let l = parent.inverse() * g;
                let (_, lr, lt) = l.to_scale_rotation_translation();
                pose[i] = BonePose { pos: lt, rot: lr };
            }
            global.push(g);
        }
        for (p, b) in pose.iter_mut().zip(before) {
            *p = BonePose::lerp(b, *p, weight);
        }
    }

    /// A bot's pose: its idle loop (the melee stance, TdBotPawn.bEnableMeleePose) under its
    /// custom animation slots. A stand-in for AT_Cop's tree, which isn't ported.
    pub fn update_bot(&mut self, bot: &tdsim::bots::Bot, dt: f32, rot_origin: tdsim::Rotator) {
        self.dt = dt;
        self.loco.phase += dt;
        // the melee dummy squares up (standmeleeidle); an armed cop's Stand aims the gun (its
        // fire anim plays on Custom_Weapon over the gun arm)
        let armed = bot.weapon.is_some();
        let idle = if armed { self.seq("Stand") } else { self.seq("standmeleeidle").or_else(|| self.seq("Stand")) };
        let len = idle.map(|i| self.set.seqs[i].length.max(1e-3)).unwrap_or(1.0);
        let mut pose = self.sample(idle, self.loco.phase % len);
        if armed {
            self.apply_slot_branches(&bot.anim, Slot::Weapon, &["RightShoulder"], &mut pose);
        }
        for slot in [Slot::Canned, Slot::FullBody, Slot::UpperBody] {
            self.apply_slot_from(&bot.anim, slot, &mut pose);
        }
        // the mesh faces the legs (LegRotation, bot_mesh_to_world); the upper body twists at
        // the spine to the body's Rotation (bUseLegRotationHack2)
        let twist = tdsim::math::norm_axis(bot.rotation.yaw - bot.leg_yaw);
        if twist != 0 {
            if let Some(spine) = self.bone_index("Spine") {
                let o = Quat::from_mat3(&ue_axes(rot_origin));
                let t_world = Quat::from_mat3(&ue_axes(tdsim::Rotator::new(0, twist, 0)));
                let t_mesh = o.inverse() * t_world * o;
                let mut parent = Quat::IDENTITY;
                let mut b = self.parents[spine];
                let mut chain = Vec::new();
                while b != usize::MAX && chain.len() < 64 {
                    chain.push(b);
                    if b == 0 {
                        break;
                    }
                    b = self.parents[b];
                }
                for &c in chain.iter().rev() {
                    parent = parent * pose[c].rot;
                }
                pose[spine].rot = (parent.inverse() * t_mesh * parent * pose[spine].rot).normalize();
            }
        }
        self.pose = pose;
    }

    /// One custom-animation slot over the pose, crossfading from the anim it replaced.
    fn apply_slot(&self, sim: &Sim, slot: Slot, pose: &mut [BonePose]) {
        self.apply_slot_from(&sim.anim, slot, pose);
    }

    /// A slot through an AnimNodeBlendPerBone (Child2Weight 1): only the branches' bones.
    fn apply_slot_branches(&self, anim: &tdsim::anim::AnimPlayer, slot: Slot, branches: &[&str], pose: &mut [BonePose]) {
        let starts: Vec<usize> = branches.iter().filter_map(|n| self.bone_index(n)).collect();
        if starts.is_empty() || anim.slots.get(&slot).is_none_or(|s| s.weight <= 0.001) {
            return;
        }
        let mut full = pose.to_vec();
        self.apply_slot_from(anim, slot, &mut full);
        for b in 0..pose.len() {
            if starts.iter().any(|&r| self.is_under(b, r)) {
                pose[b] = full[b];
            }
        }
    }

    fn apply_slot_from(&self, anim: &tdsim::anim::AnimPlayer, slot: Slot, pose: &mut [BonePose]) {
        {
            let Some(s) = anim.slots.get(&slot) else { return };
            if s.weight <= 0.001 {
                return;
            }
            let Some(si) = self.seq(&s.name) else { return };
            let mut custom = self.sample(Some(si), s.position);
            if let Some(prev) = &s.prev {
                if let Some(pi) = self.seq(&prev.name) {
                    let old = self.sample(Some(pi), prev.position);
                    for (b, c) in custom.iter_mut().enumerate() {
                        *c = BonePose::lerp(old[b], *c, s.crossfade);
                    }
                }
            }
            for (b, p) in pose.iter_mut().enumerate() {
                *p = BonePose::lerp(*p, custom[b], s.weight.clamp(0.0, 1.0));
            }
        }
    }

    /// Bone-to-mesh (glTF space) matrices for the current pose.
    pub fn globals(&self) -> Vec<Mat4> {
        let mut g: Vec<Mat4> = Vec::with_capacity(self.pose.len());
        for (i, p) in self.pose.iter().enumerate() {
            let local = Mat4::from_rotation_translation(p.rot, p.pos);
            g.push(if i == 0 { local } else { g[self.parents[i]] * local });
        }
        g
    }

    pub fn bone_index(&self, name: &str) -> Option<usize> {
        self.bone_names.iter().position(|n| n.eq_ignore_ascii_case(name))
    }
}

/// Mesh-to-world transform for the 1p meshes in Bevy space: pawn location and yaw, the
/// mesh offsets (step smoothing, root offset, XY offset), then SkeletalMesh Origin/RotOrigin.
pub fn mesh_to_world(sim: &Sim, rot_origin: tdsim::Rotator, origin: [f32; 3]) -> Mat4 {
    let p = &sim.pawn;
    let swap = |v: tdsim::Vec3| GVec3::new(v.x, v.z, v.y);
    let ue_mat = |r: tdsim::Rotator| {
        let (x, y, z) = r.axes();
        glam::Mat3::from_cols(swap(x), swap(z), swap(y))
    };
    // S * R * S for the y/z swap S on both sides: columns are R's X, Z, Y axes, each swapped.
    let r_pawn = ue_mat(tdsim::Rotator::new(0, p.rotation.yaw, 0));
    let r_origin = ue_mat(rot_origin);
    let rot = r_pawn * r_origin;
    // Mesh.Translation.Z (SetTargetMeshZ: crouched collision lifts the mesh, steps smooth it)
    let offs = tdsim::Vec3::new(0.0, 0.0, p.mesh_translation_z) + sim.mesh_offset_xy_world();
    // RootControl in actor space here; in bone space it's applied to the root bone (pose)
    let root = if p.root_offset_space == tdsim::pawn::BoneControlSpace::Bone { tdsim::Vec3::ZERO } else { p.root_offset_effective() };
    let pawn_rot = tdsim::Rotator::new(0, p.rotation.yaw, 0);
    let (fx, fy, fz) = pawn_rot.axes();
    let root_world = fx * root.x + fy * root.y + fz * root.z;
    let loc = p.location + offs + root_world;
    let t = swap(loc) * 0.01 + rot * (ue_pos(origin));
    Mat4::from_cols(rot.x_axis.extend(0.0), rot.y_axis.extend(0.0), rot.z_axis.extend(0.0), t.extend(1.0))
}

/// Mesh-to-world for a bot's mesh: its location and yaw, then Origin / RotOrigin.
/// A rotator as the glTF-space rotation matrix (Unreal axes, Y/Z swapped).
fn ue_axes(r: tdsim::Rotator) -> glam::Mat3 {
    let swap = |v: tdsim::Vec3| GVec3::new(v.x, v.z, v.y);
    let (x, y, z) = r.axes();
    glam::Mat3::from_cols(swap(x), swap(z), swap(y))
}

/// Mesh-to-world for an enemy: placed at its legs' yaw (TdPawn.LegRotation).
pub fn bot_mesh_to_world(bot: &tdsim::bots::Bot, rot_origin: tdsim::Rotator, origin: [f32; 3]) -> Mat4 {
    let swap = |v: tdsim::Vec3| GVec3::new(v.x, v.z, v.y);
    let ue_mat = ue_axes;
    let rot = ue_mat(tdsim::Rotator::new(0, bot.leg_yaw, 0)) * ue_mat(rot_origin);
    let t = swap(bot.location) * 0.01 + rot * ue_pos(origin);
    Mat4::from_cols(rot.x_axis.extend(0.0), rot.y_axis.extend(0.0), rot.z_axis.extend(0.0), t.extend(1.0))
}

/// UE3 FMatrix::Rotator for a rotation given by its X/Y/Z axes (Unreal space).
pub fn rotator_from_axes(x: GVec3, y: GVec3, z: GVec3) -> tdsim::Rotator {
    let k = 32768.0 / std::f32::consts::PI;
    let pitch = (x.z.atan2((x.x * x.x + x.y * x.y).sqrt()) * k) as i32;
    let yaw = (x.y.atan2(x.x) * k) as i32;
    let (_, sy, _) = tdsim::Rotator::new(pitch, yaw, 0).axes();
    let sy = GVec3::new(sy.x, sy.y, sy.z);
    let roll = (z.dot(sy).atan2(y.dot(sy)) * k) as i32;
    tdsim::Rotator::new(pitch, yaw, roll)
}

/// The matrix-to-rotator GetCameraAnimation uses (0x12B0670), not FMatrix::Rotator: an
/// Euler split around the mesh's Y axis (Pitch = atan2(-Z.x, Z.z), Yaw = atan2(X.y, Y.y),
/// Roll = asin(Z.y)). The two agree for small angles and part ways for the big swings of
/// e.g. HangFreeTurnRight.
pub fn camera_rotator_from_axes(x: GVec3, y: GVec3, z: GVec3) -> tdsim::Rotator {
    let k = 32768.0 / std::f32::consts::PI;
    let tiny = |v: f32| v.abs() < 1e-8;
    if z.y >= 1.0 {
        let yaw = if tiny(x.z) && tiny(x.x) { 0 } else { (x.z.atan2(x.x) * k) as i32 };
        return tdsim::Rotator::new(0, yaw, 0x4000);
    }
    if z.y <= -1.0 {
        let yaw = if tiny(x.z) && tiny(x.x) { 0 } else { (x.z.atan2(x.x) * -k) as i32 };
        return tdsim::Rotator::new(0, yaw, -16384);
    }
    let yaw = if tiny(x.y) && tiny(y.y) { 0 } else { (x.y.atan2(y.y) * k) as i32 };
    let roll = (z.y.clamp(-1.0, 1.0).asin() * k) as i32;
    let pitch = if tiny(z.x) && tiny(z.z) { 0 } else { ((-z.x).atan2(z.z) * k) as i32 };
    tdsim::Rotator::new(pitch, yaw, roll)
}

impl PoseEvaluator {
    /// ATdPawn::GetCameraAnimation (0x12B5690): the EyeJoint's mesh-space rotation through
    /// 0x12B0670. The CameraJoint-relative-to-EyeJoint delta is only added for camera-slot anims
    /// with bDeltaCameraAnimation, which this port doesn't play.
    pub fn camera_animation(&self) -> tdsim::Rotator {
        let Some(i) = self.bone_index("EyeJoint") else { return tdsim::Rotator::ZERO };
        let m = self.globals()[i];
        // glTF mesh space -> Unreal mesh space: swap Y/Z on both sides
        let s = |v: GVec3| GVec3::new(v.x, v.z, v.y);
        let (gx, gy, gz) = (m.x_axis.truncate(), m.y_axis.truncate(), m.z_axis.truncate());
        // columns of S*R*S are S*R e_x, S*R e_z, S*R e_y
        let (ux, uy, uz) = (s(gx), s(gz), s(gy));
        camera_rotator_from_axes(ux.normalize(), uy.normalize(), uz.normalize())
    }
}
