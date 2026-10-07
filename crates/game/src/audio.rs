//! Plays the pawn's sounds: the sim says which cue (or which footstep for which surface), the
//! SoundCue graph from the install decides the waves, volume and pitch, as UE3's SoundCue
//! nodes do (random picks, modulator ranges, mixers, loops, delays) plus Mirror's Edge's native
//! TdSoundNodeVelocity (0x122BE20) and TdSoundNodeADSR (0x122A810), which UE3 re-evaluates every
//! frame a sound plays; here each voice keeps its dynamic nodes and is updated per frame.

use bevy::audio::{AudioSinkPlayback, PlaybackMode, Volume};
use bevy::prelude::*;
use me_level::sounds::SoundBank;
use std::collections::HashMap;
use std::sync::Mutex;
use tdsim::sound::{LoopSlot, LoopSound, SoundEvent};
use upk::sound::{SoundNode, WaveRef};

/// Overall level for the pawn's sounds (UE3's SoundGroup volumes are all 1 here): the
/// settings' volume.
fn master() -> f32 {
    crate::settings::volume()
}
/// USoundNode::GetDuration of a looping node (INDEFINITELY_LOOPING_DURATION).
const LOOPING_DURATION: f32 = 10000.0;

/// The sound bank, handed over from loading to the Startup system.
#[derive(Resource)]
pub struct PendingSounds(pub Mutex<Option<SoundBank>>);

#[derive(Resource)]
pub struct Sfx {
    bank: SoundBank,
    handles: HashMap<WaveRef, Handle<AudioSource>>,
    rng: u64,
    /// Voices waiting out a SoundNodeDelay: (seconds left, voice, loop slot).
    delayed: Vec<(f32, Voice, Option<LoopSlot>)>,
}

/// Sounds the sim produced this frame.
#[derive(Resource, Default)]
pub struct SoundQueue(pub Vec<SoundEvent>);

/// The speeds TdSoundNodeVelocity reads, in uu/s: the owner's (the pawn's) and the listener's
/// (the camera's) velocity.
#[derive(Resource, Default)]
pub struct Listener {
    pub owner_velocity: Vec3,
    pub listener_velocity: Vec3,
    /// TdPawn.CustomSoundInput (SpeedType Custom).
    pub custom: f32,
    last_eye: Option<Vec3>,
}

#[derive(Component)]
pub struct LoopVoice(LoopSlot);

/// UAudioComponent::FadeIn: the volume ramps up from silence.
#[derive(Component)]
pub struct FadeIn {
    left: f32,
    total: f32,
}

#[derive(Component)]
pub struct FadeOut {
    left: f32,
    total: f32,
}

/// A playing voice: base volume and pitch from the static nodes, the dynamic nodes on top.
#[derive(Component)]
pub struct ActiveVoice {
    volume: f32,
    pitch: f32,
    dyns: Vec<Dyn>,
    /// AudioComponent.PlaybackTime.
    time: f32,
    /// FadeOut multiplier.
    fade: f32,
}

#[derive(Clone, Debug)]
enum Dyn {
    Velocity {
        min: f32,
        max: f32,
        volume: (f32, f32),
        pitch: (f32, f32),
        modulate_volume: bool,
        modulate_pitch: bool,
        fade_in: f32,
        fade_out: f32,
        interp: u8,
        speed_type: u8,
        /// The rate-limited speed (the node's per-instance payload); None until the first update.
        filtered: Option<f32>,
    },
    Adsr { a: f32, d: f32, s: f32, r: f32, methods: [u8; 3], modulate_volume: bool, modulate_pitch: bool, duration: f32 },
}

impl Sfx {
    fn rand(&mut self) -> f32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        ((self.rng.wrapping_mul(0x2545F4914F6CDD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    fn range(&mut self, r: (f32, f32)) -> f32 {
        r.0 + (r.1 - r.0) * self.rand()
    }
}

/// One wave to start: the result of walking a cue graph.
#[derive(Clone)]
struct Voice {
    wave: WaveRef,
    volume: f32,
    pitch: f32,
    looping: bool,
    delay: f32,
    dyns: Vec<Dyn>,
}

/// The shaping curve of SoundInterpolationMethod (0x1227F40) on t in [0, 1].
fn curve(t: f32, method: u8) -> f32 {
    match method {
        0 => t,
        1 => t * ((3.55 - 2.7 * t) * t * t + 0.15),
        2 => t * t,
        3 => (2.0 - t) * t,
        _ => t.clamp(0.0, 1.0),
    }
}

/// 0x1227FE0: where `v` sits between `lo` and `hi`, clamped and shaped.
fn shaped(v: f32, lo: f32, hi: f32, method: u8) -> f32 {
    if lo == hi {
        return lo;
    }
    curve(((v - lo) / (hi - lo)).clamp(0.0, 1.0), method)
}

/// 0x1228060 / 0x12280B0: from `a` to `b` as `v` goes from `lo` to `hi`.
fn interp(v: f32, lo: f32, hi: f32, a: f32, b: f32, method: u8) -> f32 {
    if a == b { a } else { a + (b - a) * shaped(v, lo, hi, method) }
}

/// USoundNode::GetDuration.
fn duration(bank: &SoundBank, n: &SoundNode) -> f32 {
    match n {
        SoundNode::Wave(w) => bank.waves.get(w).map_or(0.0, |d| d.duration),
        SoundNode::Random { children, .. } | SoundNode::Mixer { children, .. } => children.iter().map(|c| duration(bank, c)).fold(0.0, f32::max),
        SoundNode::Concatenator(cs) => cs.iter().map(|c| duration(bank, c)).sum(),
        SoundNode::Looping(_) => LOOPING_DURATION,
        SoundNode::Delay { delay, child } => delay.1 + duration(bank, child),
        SoundNode::Modulator { child, .. } | SoundNode::Velocity { child, .. } | SoundNode::Adsr { child, .. } | SoundNode::Pass(child) => duration(bank, child),
        SoundNode::Empty => 0.0,
    }
}

fn walk(sfx: &mut Sfx, n: &SoundNode, mut v: Voice, out: &mut Vec<Voice>) {
    match n {
        SoundNode::Wave(w) => {
            v.wave = w.clone();
            out.push(v);
        }
        SoundNode::Random { weights, children } => {
            let total: f32 = weights.iter().sum();
            let mut pick = sfx.rand() * total.max(1e-6);
            for (w, c) in weights.iter().zip(children) {
                if pick < *w {
                    return walk(sfx, c, v, out);
                }
                pick -= w;
            }
            if let Some(c) = children.last() {
                walk(sfx, c, v, out);
            }
        }
        SoundNode::Modulator { volume, pitch, child } => {
            v.volume *= sfx.range(*volume);
            v.pitch *= sfx.range(*pitch);
            walk(sfx, child, v, out);
        }
        SoundNode::Mixer { volumes, children } => {
            for (vol, c) in volumes.iter().zip(children) {
                let mut v2 = v.clone();
                v2.volume *= vol;
                walk(sfx, c, v2, out);
            }
        }
        SoundNode::Looping(c) => {
            v.looping = true;
            walk(sfx, c, v, out);
        }
        // concatenated sounds: the first part (the pawn's cues have none)
        SoundNode::Concatenator(cs) => {
            if let Some(c) = cs.first() {
                walk(sfx, c, v, out);
            }
        }
        SoundNode::Delay { delay, child } => {
            v.delay += sfx.range(*delay);
            walk(sfx, child, v, out);
        }
        SoundNode::Pass(child) => walk(sfx, child, v, out),
        SoundNode::Velocity { min_speed, max_speed, volume, pitch, modulate_volume, modulate_pitch, fade_in, fade_out, interp, speed_type, child } => {
            v.dyns.push(Dyn::Velocity {
                min: *min_speed,
                max: *max_speed,
                volume: *volume,
                pitch: *pitch,
                modulate_volume: *modulate_volume,
                modulate_pitch: *modulate_pitch,
                fade_in: *fade_in,
                fade_out: *fade_out,
                interp: *interp,
                speed_type: *speed_type,
                filtered: None,
            });
            walk(sfx, child, v, out);
        }
        SoundNode::Adsr { attack, decay, sustain, release, methods, modulate_volume, modulate_pitch, child } => {
            // first ParseNodes: sample the four distributions once
            let (a, mut d, mut s, mut r) = (sfx.range(*attack), sfx.range(*decay), sfx.range(*sustain), sfx.range(*release));
            let dur = duration(&sfx.bank, n);
            // fit the envelope into the sound
            if d + a + r > dur {
                if dur < a + d {
                    if dur > a {
                        d = 0.0;
                        s = 1.0;
                        r = dur - a;
                    }
                } else {
                    r = dur - a - d;
                }
            }
            v.dyns.push(Dyn::Adsr { a, d, s, r, methods: *methods, modulate_volume: *modulate_volume, modulate_pitch: *modulate_pitch, duration: dur });
            walk(sfx, child, v, out);
        }
        SoundNode::Empty => {}
    }
}

fn cue_voices(sfx: &mut Sfx, key: &str) -> Vec<Voice> {
    let Some(cue) = sfx.bank.cues.get(key).cloned() else { return Vec::new() };
    let mut out = Vec::new();
    let v = Voice { wave: WaveRef { package: String::new(), path: String::new() }, volume: cue.volume_multiplier, pitch: cue.pitch_multiplier, looping: false, delay: 0.0, dyns: Vec::new() };
    walk(sfx, &cue.root, v, &mut out);
    out
}

/// The dynamic nodes' volume and pitch multipliers this frame (their ParseNodes).
fn evaluate(dyns: &mut [Dyn], time: f32, dt: f32, listener: &Listener) -> (f32, f32) {
    let (mut vol, mut pitch) = (1.0f32, 1.0f32);
    for d in dyns {
        match d {
            Dyn::Velocity { min, max, volume, pitch: p, modulate_volume, modulate_pitch, fade_in, fade_out, interp: method, speed_type, filtered } => {
                let speed = match speed_type {
                    1 => listener.listener_velocity.length(),
                    2 => (listener.listener_velocity - listener.owner_velocity).length(),
                    3 => listener.custom,
                    _ => listener.owner_velocity.length(),
                };
                let f = filtered.get_or_insert(speed);
                // fade time filters: a linear rate limit across the Min..Max span
                let mut delta = speed - *f;
                if delta >= 0.0 {
                    if *fade_in > 0.0 {
                        delta = delta.min((*max - *min) / *fade_in * dt);
                    }
                } else if *fade_out > 0.0 {
                    delta = delta.max(-((*max - *min) / *fade_out * dt));
                }
                *f = (*f + delta).min(*max).max(*min);
                let (vm, pm) = if *f > *min && *f < *max {
                    (interp(*f, *min, *max, volume.0, volume.1, *method), interp(*f, *min, *max, p.0, p.1, *method))
                } else if *f >= *max {
                    (volume.1, p.1)
                } else {
                    (volume.0, p.0)
                };
                if *modulate_volume {
                    vol *= vm;
                }
                if *modulate_pitch {
                    pitch *= pm;
                }
            }
            Dyn::Adsr { a, d, s, r, methods, modulate_volume, modulate_pitch, duration } => {
                let t = time;
                let env = if t > *duration {
                    0.0
                } else if *a > t {
                    shaped(t, 0.0, *a, methods[0])
                } else if *a + *d > t {
                    interp(t, *a, *a + *d, 1.0, *s, methods[1])
                } else if *r > *duration - t && *r != 0.0 {
                    interp(t, *duration - *r, *duration, *s, 0.0, methods[2])
                } else {
                    *s
                };
                if *modulate_volume {
                    vol *= env;
                }
                if *modulate_pitch {
                    pitch *= env;
                }
            }
        }
    }
    (vol, pitch)
}

fn spawn_voice(commands: &mut Commands, sfx: &Sfx, mut v: Voice, force_loop: bool, listener: &Listener) -> Option<Entity> {
    let handle = sfx.handles.get(&v.wave)?.clone();
    let looping = v.looping || force_loop;
    let (dv, dp) = evaluate(&mut v.dyns, 0.0, 0.0, listener);
    let settings = PlaybackSettings {
        mode: if looping { PlaybackMode::Loop } else { PlaybackMode::Despawn },
        volume: Volume::Linear((v.volume * dv * master()).max(0.0)),
        speed: (v.pitch * dp).clamp(0.25, 4.0),
        ..PlaybackSettings::DESPAWN
    };
    let voice = ActiveVoice { volume: v.volume, pitch: v.pitch, dyns: v.dyns, time: 0.0, fade: 1.0 };
    Some(commands.spawn((AudioPlayer(handle), settings, voice)).id())
}

pub fn setup_sounds(mut commands: Commands, pending: Res<PendingSounds>, mut sources: ResMut<Assets<AudioSource>>) {
    let Some(bank) = pending.0.lock().unwrap().take() else { return };
    let mut handles = HashMap::new();
    for (w, data) in &bank.waves {
        handles.insert(w.clone(), sources.add(AudioSource { bytes: data.ogg.clone().into() }));
    }
    let mut sfx = Sfx { bank, handles, rng: 0x9E3779B97F4A7C15, delayed: Vec::new() };
    // TdPlayerPawn.Tick: the wind AudioComponent, always playing
    let listener = Listener::default();
    for v in cue_voices(&mut sfx, tdsim::sound::WIND_SOUND) {
        spawn_voice(&mut commands, &sfx, v, true, &listener);
    }
    commands.insert_resource(sfx);
}

/// The speeds the velocity nodes read: the pawn's velocity and the camera's (the listener).
pub fn update_listener(
    game: Res<crate::Game>,
    time: Res<Time>,
    cam: Single<&Transform, With<crate::PlayerCamera>>,
    mut listener: ResMut<Listener>,
) {
    let v = game.sim.pawn.velocity;
    listener.custom = game.sim.pawn.custom_sound_input;
    listener.owner_velocity = if game.sim.pawn.physics == tdsim::Physics::None { Vec3::ZERO } else { Vec3::new(v.x, v.y, v.z) };
    // Bevy metres (Y up) back to Unreal units (Z up)
    let t = cam.translation;
    let eye = Vec3::new(t.x, t.z, t.y) * 100.0;
    let dt = time.delta_secs();
    listener.listener_velocity = match listener.last_eye {
        Some(prev) if dt > 0.0 => (eye - prev) / dt,
        _ => Vec3::ZERO,
    };
    listener.last_eye = Some(eye);
}

fn start(commands: &mut Commands, sfx: &mut Sfx, v: Voice, slot: Option<LoopSlot>, listener: &Listener) -> Option<Entity> {
    if v.delay > 0.0 {
        sfx.delayed.push((v.delay, v, slot));
        return None;
    }
    let e = spawn_voice(commands, sfx, v, false, listener)?;
    if let Some(slot) = slot {
        commands.entity(e).try_insert(LoopVoice(slot));
    }
    Some(e)
}

pub fn play_sounds(
    mut commands: Commands,
    time: Res<Time>,
    mut queue: ResMut<SoundQueue>,
    sfx: Option<ResMut<Sfx>>,
    listener: Res<Listener>,
    loops: Query<(Entity, &LoopVoice)>,
) {
    let Some(mut sfx) = sfx else {
        queue.0.clear();
        return;
    };
    // voices whose delay ran out
    let dt = time.delta_secs();
    let mut ready = Vec::new();
    sfx.delayed.retain_mut(|(left, v, slot)| {
        *left -= dt;
        if *left <= 0.0 {
            let mut v = v.clone();
            v.delay = 0.0;
            ready.push((v, *slot));
            false
        } else {
            true
        }
    });
    for (v, slot) in ready {
        start(&mut commands, &mut sfx, v, slot, &listener);
    }
    // Entities this frame already stopped, so a later event doesn't touch them again.
    let mut stopped: Vec<Entity> = Vec::new();
    for e in queue.0.drain(..) {
        match e {
            SoundEvent::Cue(c) => {
                for v in cue_voices(&mut sfx, &c) {
                    start(&mut commands, &mut sfx, v, None, &listener);
                }
            }
            // another pawn's sound: played like the player's (no distance attenuation yet)
            SoundEvent::CueAt { name, .. } => {
                for v in cue_voices(&mut sfx, &name) {
                    start(&mut commands, &mut sfx, v, None, &listener);
                }
            }
            SoundEvent::Footstep { id, material } => {
                for c in sfx.bank.footstep_cues(material, id).to_vec() {
                    for v in cue_voices(&mut sfx, &c) {
                        start(&mut commands, &mut sfx, v, None, &listener);
                    }
                }
            }
            SoundEvent::LoopStart { slot, sound, fade_in } => {
                // one AudioComponent per slot: an old one fades out
                for (ent, lv) in &loops {
                    if lv.0 == slot && !stopped.contains(&ent) {
                        stopped.push(ent);
                        commands.entity(ent).try_remove::<LoopVoice>().try_insert(FadeOut { left: 0.1, total: 0.1 });
                    }
                }
                sfx.delayed.retain(|d| d.2 != Some(slot));
                let cues: Vec<String> = match sound {
                    LoopSound::Cue(c) => vec![c],
                    LoopSound::Footstep { id, material } => sfx.bank.footstep_cues(material, id).to_vec(),
                };
                for c in cues {
                    for v in cue_voices(&mut sfx, &c) {
                        if let Some(e) = start(&mut commands, &mut sfx, v, Some(slot), &listener) {
                            if fade_in > 0.0 {
                                commands.entity(e).try_insert(FadeIn { left: fade_in, total: fade_in });
                            }
                        }
                    }
                }
            }
            SoundEvent::MeleeImpact { impact, head } => {
                for v in cue_voices(&mut sfx, tdsim::sound::melee_impact_cue(impact, head)) {
                    start(&mut commands, &mut sfx, v, None, &listener);
                }
            }
            SoundEvent::LoopStop { slot, fade_out } => {
                sfx.delayed.retain(|d| d.2 != Some(slot));
                for (ent, lv) in &loops {
                    if lv.0 != slot || stopped.contains(&ent) {
                        continue;
                    }
                    stopped.push(ent);
                    if fade_out <= 0.0 {
                        commands.entity(ent).try_despawn();
                    } else {
                        commands.entity(ent).try_remove::<LoopVoice>().try_insert(FadeOut { left: fade_out, total: fade_out });
                    }
                }
            }
        }
    }
}

/// Every playing voice, every frame: its velocity / ADSR nodes and any fade-out.
pub fn update_voices(
    mut commands: Commands,
    time: Res<Time>,
    listener: Res<Listener>,
    mut q: Query<(Entity, &mut ActiveVoice, Option<&mut FadeIn>, Option<&mut FadeOut>, Option<&mut AudioSink>)>,
) {
    let dt = time.delta_secs();
    for (e, mut v, fade_in, fade, sink) in &mut q {
        v.time += dt;
        if let Some(mut f) = fade_in {
            f.left -= dt;
            v.fade = (1.0 - f.left / f.total).clamp(0.0, 1.0);
            if f.left <= 0.0 {
                commands.entity(e).try_remove::<FadeIn>();
            }
        }
        if let Some(mut f) = fade {
            f.left -= dt;
            if f.left <= 0.0 {
                commands.entity(e).try_despawn();
                continue;
            }
            v.fade = f.left / f.total;
        }
        let t = v.time;
        let (dv, dp) = evaluate(&mut v.dyns, t, dt, &listener);
        if let Some(mut s) = sink {
            s.set_volume(Volume::Linear((v.volume * dv * v.fade * master()).max(0.0)));
            s.set_speed((v.pitch * dp).clamp(0.25, 4.0));
        }
    }
}
