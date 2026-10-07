//! The part of the animation system the movement depends on: TdAnimNodeSlot custom animations
//! (play / stop / blend timing), `OnAnimEnd` callbacks to the current move, and root motion
//! extracted from the playing sequence (Mesh.RootMotionMode = RMM_Accel).
//!
//! Pose evaluation and the AnimTree live in the renderer; this only needs sequence lengths and
//! the root bone's translation track, supplied as an `AnimLib`.

use crate::math::{Rotator, Vec3};
use crate::pawn::{Pawn, Slot};
use std::collections::HashMap;
use std::sync::Arc;

/// Root bone translation keys in the mesh's local space (uu), plus the yaw track (UE units) if
/// the sequence has one. Times in seconds.
#[derive(Clone, Debug, Default)]
pub struct RootTrack {
    pub times: Vec<f32>,
    pub translation: Vec<Vec3>,
    /// Actor-space yaw of the root bone, unwrapped (may exceed +-32768), on its own key times.
    pub yaw_times: Vec<f32>,
    pub yaw: Vec<i32>,
}

impl RootTrack {
    pub fn translation_at(&self, t: f32) -> Vec3 {
        sample(&self.times, &self.translation, t, |a, b, f| a + (b - a) * f).unwrap_or(Vec3::ZERO)
    }
    pub fn yaw_at(&self, t: f32) -> i32 {
        sample(&self.yaw_times, &self.yaw, t, |a, b, f| a + ((b - a) as f32 * f) as i32).unwrap_or(0)
    }
}

fn sample<T: Copy>(times: &[f32], vals: &[T], t: f32, lerp: impl Fn(T, T, f32) -> T) -> Option<T> {
    if vals.is_empty() {
        return None;
    }
    if vals.len() == 1 || times.len() != vals.len() || t <= times[0] {
        return Some(vals[0]);
    }
    let n = vals.len();
    if t >= times[n - 1] {
        return Some(vals[n - 1]);
    }
    let i = times.partition_point(|&x| x <= t).max(1);
    let (t0, t1) = (times[i - 1], times[i]);
    let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
    Some(lerp(vals[i - 1], vals[i], f))
}

#[derive(Clone, Debug)]
pub struct AnimSeqInfo {
    pub length: f32,
    pub rate_scale: f32,
    pub root: Option<RootTrack>,
    /// AnimSequence.Notifies, sorted by time.
    pub notifies: Vec<(f32, Notify)>,
}

/// An AnimNotify the pawn reacts to.
#[derive(Clone, Debug, PartialEq)]
pub enum Notify {
    /// AnimNotify_Footstep -> TdPawn.PlayFootStepSound(FootDown).
    Footstep(i32),
    /// AnimNotify_Sound: a SoundCue as "Package.Path".
    Cue(String),
    /// TdAnimNotify_CharacterSound (CharacterSoundTriggerType).
    CharacterSound(u8),
}

impl AnimSeqInfo {
    /// AnimNodeSequence::IssueNotifies: notifies with times in [from, to) going forwards,
    /// wrapping round the end of a looping sequence.
    pub fn notifies_between(&self, from: f32, to: f32, looping: bool, out: &mut Vec<Notify>) {
        if self.notifies.is_empty() || to <= from {
            return;
        }
        let mut add = |a: f32, b: f32| {
            for (t, n) in &self.notifies {
                if *t >= a && *t < b {
                    out.push(n.clone());
                }
            }
        };
        if looping && to > self.length && self.length > 0.0 {
            add(from, self.length);
            add(0.0, to - self.length);
        } else {
            add(from, to.min(self.length + 1e-4));
        }
    }
}

/// Every sequence the player's AnimSets contain, by (lowercase) name.
#[derive(Clone, Default)]
pub struct AnimLib {
    pub seqs: Arc<HashMap<String, AnimSeqInfo>>,
    /// SkeletalMesh RotOrigin of the 1p mesh: mesh-local space -> pawn space.
    pub mesh_rot: Rotator,
}

impl AnimLib {
    pub fn get(&self, name: &str) -> Option<&AnimSeqInfo> {
        self.seqs.get(&name.to_ascii_lowercase())
    }
}

/// One custom animation playing in a slot.
#[derive(Clone, Debug)]
pub struct SlotAnim {
    pub name: String,
    pub position: f32,
    pub rate: f32,
    /// AnimNodeSequence.Rate (the play rate, without the sequence's RateScale).
    pub play_rate: f32,
    pub length: f32,
    pub looping: bool,
    pub root_motion: bool,
    pub root_rotation: bool,
    pub blend_in: f32,
    pub blend_out: f32,
    /// Current blend weight of the slot (0..1) and where it is heading.
    pub weight: f32,
    pub target_weight: f32,
    pub blend_time_left: f32,
    pub playing: bool,
    pub ended: bool,
    /// AnimNodeSlot crossfade: the anim this one replaced keeps playing underneath while
    /// `crossfade` goes 0 -> 1 over the new anim's blend-in time.
    pub prev: Option<Box<SlotAnim>>,
    pub crossfade: f32,
    crossfade_time: f32,
    last_root: Vec3,
    last_yaw: i32,
    start_yaw: i32,
    cease_fired: bool,
}

#[derive(Clone, Debug)]
pub enum AnimEvent {
    /// An AnimNotify reached in a custom animation.
    Notify(Notify),
    /// AnimNodeSequence reached its end (fires `OnAnimEnd` -> move.OnCustomAnimEnd).
    End { slot: Slot, name: String, played: f32, excess: f32 },
    /// A sequence extracting root motion / rotation stopped being relevant (blended out after
    /// its end or a StopCustomAnim): TdPawn.OnCeaseRelevantRootMotion.
    CeaseRelevantRootMotion { slot: Slot, name: String },
}

pub struct AnimPlayer {
    pub lib: AnimLib,
    pub slots: HashMap<Slot, SlotAnim>,
    pub events: Vec<AnimEvent>,
    pub root_rotation_delta: i32,
    /// Normalised position in the locomotion cycle (MasterSync group), set by the anim tree.
    pub locomotion_phase: f32,
}

impl AnimPlayer {
    pub fn new(lib: AnimLib) -> Self {
        Self { lib, slots: HashMap::new(), events: Vec::new(), root_rotation_delta: 0, locomotion_phase: 0.0 }
    }

    pub fn length(&self, name: &str) -> f32 {
        self.lib.get(name).map(|s| s.length / s.rate_scale.max(1e-4)).unwrap_or(0.0)
    }

    /// TdPawn::PlayCustomAnim (native): restart `name` in `slot`. Returns the play length.
    #[allow(clippy::too_many_arguments)]
    pub fn play(&mut self, slot: Slot, name: &str, rate: f32, blend_in: f32, blend_out: f32, looping: bool, root_motion: bool, root_rotation: bool) -> f32 {
        let info = self.lib.get(name).cloned();
        let (length, scale) = info.as_ref().map(|i| (i.length, i.rate_scale)).unwrap_or((0.0, 1.0));
        let old = self.slots.remove(&slot);
        let prev_weight = old.as_ref().map(|s| s.weight).unwrap_or(0.0);
        // the replaced anim fades out under the new one (only if it was still showing)
        let prev = old.filter(|o| o.weight > 0.001 && blend_in > 0.0).map(|mut o| {
            o.prev = None;
            Box::new(o)
        });
        let root0 = info.as_ref().and_then(|i| i.root.as_ref()).map(|r| r.translation_at(0.0)).unwrap_or(Vec3::ZERO);
        let yaw0 = info.as_ref().and_then(|i| i.root.as_ref()).map(|r| r.yaw_at(0.0)).unwrap_or(0);
        // a reversed sequence starts from its end (climbing down plays the climb-up clips at -1)
        let start = if rate < 0.0 { length } else { 0.0 };
        self.slots.insert(
            slot,
            SlotAnim {
                name: name.to_string(),
                position: start,
                rate: rate * scale,
                play_rate: rate,
                length,
                looping,
                root_motion,
                root_rotation,
                blend_in,
                blend_out,
                weight: prev_weight,
                target_weight: 1.0,
                blend_time_left: blend_in,
                playing: true,
                ended: false,
                crossfade: if prev.is_some() { 0.0 } else { 1.0 },
                crossfade_time: blend_in,
                cease_fired: false,
                prev,
                last_root: root0,
                last_yaw: yaw0,
                start_yaw: yaw0,
            },
        );
        if length <= 0.0 {
            // Missing sequence: UE3 plays nothing and the end notify fires immediately.
            self.events.push(AnimEvent::End { slot, name: name.to_string(), played: 0.0, excess: 0.0 });
            if let Some(s) = self.slots.get_mut(&slot) {
                s.playing = false;
                s.ended = true;
            }
        }
        if rate > 0.0 { length / (rate * scale) } else { 0.0 }
    }

    /// TdPawn::StopCustomAnim.
    pub fn stop(&mut self, slot: Slot, blend_out: f32) {
        if let Some(s) = self.slots.get_mut(&slot) {
            s.target_weight = 0.0;
            s.blend_time_left = blend_out;
            s.playing = false;
        }
    }

    /// TdPawn.SetCustomAnimsBlendOutTime for one slot.
    pub fn set_blend_out(&mut self, slot: Slot, blend_out: f32) {
        if let Some(s) = self.slots.get_mut(&slot) {
            s.blend_out = blend_out;
        }
    }

    /// AnimNodeSequence.SetPosition(pos, bFireNotifies=false) on the slot's sequence.
    pub fn set_position(&mut self, slot: Slot, pos: f32) {
        if let Some(s) = self.slots.get_mut(&slot) {
            s.position = pos.clamp(0.0, s.length);
            if let Some(r) = self.lib.get(&s.name).and_then(|i| i.root.as_ref()) {
                s.last_root = r.translation_at(s.position);
                s.last_yaw = r.yaw_at(s.position);
            }
        }
    }

    /// AnimNodeSequence.GetNormalizedPosition.
    pub fn normalized_position(&self, slot: Slot) -> f32 {
        self.slots.get(&slot).map(|s| if s.length > 0.0 { s.position / s.length } else { 0.0 }).unwrap_or(0.0)
    }

    pub fn stop_all(&mut self) {
        self.slots.clear();
    }

    pub fn is_playing(&self, slot: Slot, name: &str) -> bool {
        self.slots.get(&slot).is_some_and(|s| s.playing && s.name.eq_ignore_ascii_case(name))
    }

    pub fn current(&self, slot: Slot) -> Option<&SlotAnim> {
        self.slots.get(&slot).filter(|s| s.playing)
    }

    /// Advance all slots; returns the world-space root motion delta (pawn rotation applied).
    pub fn tick(&mut self, dt: f32, pawn: &Pawn) -> Vec3 {
        self.tick_root(dt, pawn.is_using_root_motion, pawn.rotation.yaw)
    }

    /// `tick` for any pawn: whether it uses root motion and its yaw.
    pub fn tick_root(&mut self, dt: f32, use_root_motion: bool, yaw: i32) -> Vec3 {
        // root motion accumulated in actor space (mesh space through RotOrigin)
        let mut root_actor = Vec3::ZERO;
        let (mx, my, mz) = self.lib.mesh_rot.axes();
        let to_actor = |d: Vec3, unrotate: i32| {
            let v = mx * d.x + my * d.y + mz * d.z;
            if unrotate == 0 {
                return v;
            }
            // translation keys live in the anim's start frame; the actor has already turned by
            // the extracted root yaw, so take that back out
            let a = -(unrotate as f32) * std::f32::consts::PI / 32768.0;
            let (sn, cs) = a.sin_cos();
            Vec3::new(v.x * cs - v.y * sn, v.x * sn + v.y * cs, v.z)
        };
        self.root_rotation_delta = 0;
        let lib = self.lib.clone();
        for (slot, s) in self.slots.iter_mut() {
            // blend weight
            if s.blend_time_left > 0.0 {
                let f = (dt / s.blend_time_left).min(1.0);
                s.weight += (s.target_weight - s.weight) * f;
                s.blend_time_left -= dt;
            } else {
                s.weight = s.target_weight;
            }
            if (s.root_motion || s.root_rotation) && !s.cease_fired && (s.ended || !s.playing) && s.weight <= 0.0 {
                s.cease_fired = true;
                self.events.push(AnimEvent::CeaseRelevantRootMotion { slot: *slot, name: s.name.clone() });
            }
            if let Some(p) = s.prev.as_mut() {
                if p.length > 0.0 {
                    p.position += dt * p.rate;
                    p.position = if p.looping { p.position % p.length } else { p.position.min(p.length) };
                }
                s.crossfade = if s.crossfade_time > 0.0 { (s.crossfade + dt / s.crossfade_time).min(1.0) } else { 1.0 };
                if s.crossfade >= 1.0 {
                    s.prev = None;
                }
            }
            if s.ended {
                continue;
            }
            // AnimNodeSlot::StopCustomAnim only blends the slot out: the sequence keeps playing
            // and still reaches its end (OnAnimEnd), even blended out to nothing, since slot
            // children don't set bSkipTickWhenZeroWeight. It just adds no root motion then.
            if !s.playing && s.weight <= 0.0 {
                if s.length > 0.0 {
                    s.position += dt * s.rate;
                    if s.rate < 0.0 && s.position <= 0.0 {
                        s.position = if s.looping { s.position.rem_euclid(s.length) } else { 0.0 };
                        if !s.looping {
                            s.ended = true;
                            self.events.push(AnimEvent::End { slot: *slot, name: s.name.clone(), played: s.length, excess: 0.0 });
                        }
                    } else if s.position >= s.length {
                        if s.looping {
                            s.position %= s.length;
                        } else {
                            let excess = (s.position - s.length) / s.rate.max(1e-4);
                            s.position = s.length;
                            s.ended = true;
                            self.events.push(AnimEvent::End { slot: *slot, name: s.name.clone(), played: s.length, excess });
                        }
                    }
                }
                continue;
            }
            let prev = s.position;
            s.position += dt * s.rate;
            let info = lib.get(&s.name);
            if let Some(i) = info.filter(|_| s.weight > 0.0 && s.rate > 0.0) {
                let mut fired = Vec::new();
                i.notifies_between(prev, s.position, s.looping, &mut fired);
                self.events.extend(fired.into_iter().map(AnimEvent::Notify));
            }
            let root = info.and_then(|i| i.root.as_ref());
            if s.rate < 0.0 && s.position <= 0.0 && s.length > 0.0 {
                // AnimNodeSequence::AdvanceBy below the start: wrap or end
                if s.looping {
                    s.position = s.position.rem_euclid(s.length);
                } else {
                    let excess = -s.position / (-s.rate).max(1e-4);
                    s.position = 0.0;
                    s.playing = false;
                    s.ended = true;
                    s.target_weight = 0.0;
                    s.blend_time_left = s.blend_out;
                    self.events.push(AnimEvent::End { slot: *slot, name: s.name.clone(), played: s.length, excess });
                    let _ = prev;
                    continue;
                }
            }
            if s.position >= s.length && s.length > 0.0 {
                if s.looping {
                    // root motion across the wrap
                    if let Some(r) = root {
                        if s.root_motion {
                            let un = if s.root_rotation { s.last_yaw - s.start_yaw } else { 0 };
                            root_actor += to_actor(r.translation_at(s.length) - s.last_root, un);
                            s.last_root = r.translation_at(0.0);
                        }
                    }
                    s.position %= s.length;
                } else {
                    let excess = (s.position - s.length) / s.rate.max(1e-4);
                    s.position = s.length;
                    if let Some(r) = root {
                        if s.root_motion {
                            let t = r.translation_at(s.length);
                            let un = if s.root_rotation { s.last_yaw - s.start_yaw } else { 0 };
                            root_actor += to_actor(t - s.last_root, un);
                            s.last_root = t;
                        }
                    }
                    s.playing = false;
                    s.ended = true;
                    // Slot blends out over its blend-out time when the sequence finishes; a
                    // negative one (AnimNodeSlot PendingBlendOutTime < 0) stays on the last frame.
                    if s.blend_out >= 0.0 {
                        s.target_weight = 0.0;
                        s.blend_time_left = s.blend_out;
                    }
                    self.events.push(AnimEvent::End { slot: *slot, name: s.name.clone(), played: s.length, excess });
                    let _ = prev;
                    continue;
                }
            }
            if let Some(r) = root {
                if s.root_motion {
                    let t = r.translation_at(s.position);
                    let un = if s.root_rotation { s.last_yaw - s.start_yaw } else { 0 };
                    root_actor += to_actor(t - s.last_root, un);
                    s.last_root = t;
                }
                if s.root_rotation {
                    let y = r.yaw_at(s.position);
                    self.root_rotation_delta += y - s.last_yaw;
                    s.last_yaw = y;
                }
            }
        }
        if !use_root_motion {
            return Vec3::ZERO;
        }
        // actor -> world (pawn yaw)
        let v = root_actor;
        let (x, y, z) = Rotator::new(0, yaw, 0).axes();
        x * v.x + y * v.y + z * v.z
    }

    pub fn take_events(&mut self) -> Vec<AnimEvent> {
        std::mem::take(&mut self.events)
    }
}
