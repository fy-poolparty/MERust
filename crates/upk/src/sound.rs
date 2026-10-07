//! USoundNodeWave (ArVer 536): the PC copy of the sound is an Ogg Vorbis stream stored inline
//! in the CompressedPCData bulk block; and USoundCue node graphs, read into a tree the player
//! can evaluate (random picks, modulator ranges, mixers, loops, Mirror's Edge velocity nodes).

use crate::package::Package;
use crate::props::{export_props, find, read_props, Value};

#[derive(Clone, Debug)]
pub struct WaveData {
    /// Ogg Vorbis file bytes.
    pub ogg: Vec<u8>,
    pub channels: i32,
    pub sample_rate: i32,
    pub duration: f32,
}

/// A wave a cue refers to: by package name and object path inside it (the wave may live in
/// another package, through an import).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WaveRef {
    pub package: String,
    pub path: String,
}

#[derive(Clone, Debug)]
pub enum SoundNode {
    Wave(WaveRef),
    /// SoundNodeRandom: one child by weight.
    Random { weights: Vec<f32>, children: Vec<SoundNode> },
    /// SoundNodeModulator: volume and pitch picked uniformly in [min, max].
    Modulator { volume: (f32, f32), pitch: (f32, f32), child: Box<SoundNode> },
    /// SoundNodeMixer: all children at once.
    Mixer { volumes: Vec<f32>, children: Vec<SoundNode> },
    /// SoundNodeLooping (forever, as the cues here use it).
    Looping(Box<SoundNode>),
    /// SoundNodeConcatenator: children one after another.
    Concatenator(Vec<SoundNode>),
    /// SoundNodeDelay: wait [min, max] seconds first.
    Delay { delay: (f32, f32), child: Box<SoundNode> },
    /// TdSoundNodeVelocity: volume / pitch from the owner's speed between Min and MaxSpeed.
    /// `interp`: SoundInterpolationMethod (Linear, Smooth, Square, Fast); `speed_type`:
    /// SpeedType (Source, Listener, Relative, Custom).
    Velocity { min_speed: f32, max_speed: f32, volume: (f32, f32), pitch: (f32, f32), modulate_volume: bool, modulate_pitch: bool, fade_in: f32, fade_out: f32, interp: u8, speed_type: u8, child: Box<SoundNode> },
    /// TdSoundNodeADSR: an attack / decay / sustain / release envelope over the sound's length.
    /// The four ranges are sampled once when the sound starts; `methods` = attack, decay, release
    /// SoundInterpolationMethod.
    Adsr { attack: (f32, f32), decay: (f32, f32), sustain: (f32, f32), release: (f32, f32), methods: [u8; 3], modulate_volume: bool, modulate_pitch: bool, child: Box<SoundNode> },
    /// Nodes that only pass through here (attenuation, mix groups, slow motion, ...).
    Pass(Box<SoundNode>),
    Empty,
}

#[derive(Clone, Debug)]
pub struct Cue {
    pub volume_multiplier: f32,
    pub pitch_multiplier: f32,
    pub root: SoundNode,
}

fn bulk_header(data: &[u8], at: usize) -> Option<(u32, usize)> {
    let u = |o: usize| data.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    Some((u(at)?, u(at + 8)? as usize))
}

/// SoundNodeWave: properties, then the Raw / PC / Xbox360 / PS3 bulk blocks (flags, element
/// count, size on disk, offset), each stored inline unless flagged unused or external.
pub fn read_wave(pkg: &Package, export: usize) -> Option<WaveData> {
    let data = pkg.export_bytes(export);
    let (props, end) = export_props(pkg, export).ok()?;
    let int = |n: &str| match find(&props, n).map(|p| &p.value) {
        Some(Value::Int(v)) => *v,
        _ => 0,
    };
    let duration = match find(&props, "Duration").map(|p| &p.value) {
        Some(Value::Float(v)) => *v,
        _ => 0.0,
    };
    let mut at = end;
    let mut pc = None;
    for block in 0..4 {
        let (flags, size) = bulk_header(data, at)?;
        at += 16;
        let inline = flags & 0x21 == 0;
        if block == 1 && inline && size > 0 {
            pc = data.get(at..at + size).map(|b| b.to_vec());
        }
        if inline {
            at += size;
        }
    }
    let ogg = pc.filter(|b| b.starts_with(b"OggS"))?;
    Some(WaveData { ogg, channels: int("NumChannels"), sample_rate: int("SampleRate"), duration })
}

/// A RawDistributionFloat's range: the cooked lookup table (min/max pairs), else the
/// Distribution object's Min/Max or Constant, else `default`.
fn distribution(pkg: &Package, data: &[u8], props: &[crate::props::Prop], name: &str, default: (f32, f32)) -> (f32, f32) {
    let Some(p) = find(props, name) else { return default };
    let Ok((sub, _)) = read_props(pkg, data, p.start) else { return default };
    if let Some(t) = find(&sub, "LookupTable") {
        let n = i32::from_le_bytes(data[t.start..t.start + 4].try_into().unwrap()).max(0) as usize;
        let fl: Vec<f32> = (0..n).map(|i| f32::from_le_bytes(data[t.start + 4 + i * 4..t.start + 8 + i * 4].try_into().unwrap())).collect();
        // samples of (min, max) over the distribution's time range
        if fl.len() >= 2 {
            return (fl[0], fl[1]);
        }
    }
    if let Some(Value::Object(o)) = find(&sub, "Distribution").map(|p| p.value.clone()) {
        if o > 0 {
            if let Ok((dp, _)) = export_props(pkg, o as usize - 1) {
                let f = |n: &str| match find(&dp, n).map(|p| &p.value) {
                    Some(Value::Float(v)) => Some(*v),
                    _ => None,
                };
                if let Some(c) = f("Constant") {
                    return (c, c);
                }
                // An unset Min or Max keeps the value of the node class's default subobject.
                return (f("Min").unwrap_or(default.0), f("Max").unwrap_or(default.1));
            }
        }
    }
    default
}

fn floats(data: &[u8], p: &crate::props::Prop) -> Vec<f32> {
    let n = i32::from_le_bytes(data[p.start..p.start + 4].try_into().unwrap()).max(0) as usize;
    (0..n).map(|i| f32::from_le_bytes(data[p.start + 4 + i * 4..p.start + 8 + i * 4].try_into().unwrap())).collect()
}

fn objects(data: &[u8], p: &crate::props::Prop) -> Vec<i32> {
    let n = i32::from_le_bytes(data[p.start..p.start + 4].try_into().unwrap()).max(0) as usize;
    (0..n).map(|i| i32::from_le_bytes(data[p.start + 4 + i * 4..p.start + 8 + i * 4].try_into().unwrap())).collect()
}

/// Package name and path inside it for an object index (exports live in `pkg`).
pub fn object_ref(pkg: &Package, o: i32) -> WaveRef {
    if o > 0 {
        WaveRef { package: pkg.name.clone(), path: pkg.object_path(o) }
    } else {
        let full = pkg.object_path(o);
        match full.split_once('.') {
            Some((p, rest)) => WaveRef { package: p.to_string(), path: rest.to_string() },
            None => WaveRef { package: String::new(), path: full },
        }
    }
}

fn node(pkg: &Package, o: i32, depth: usize) -> SoundNode {
    if depth > 32 || o == 0 {
        return SoundNode::Empty;
    }
    let class = pkg.class_of(o);
    if class == "SoundNodeWave" || o < 0 {
        return SoundNode::Wave(object_ref(pkg, o));
    }
    let idx = o as usize - 1;
    let data = pkg.export_bytes(idx);
    let Ok((props, _)) = export_props(pkg, idx) else { return SoundNode::Empty };
    let children: Vec<SoundNode> = find(&props, "ChildNodes").map(|p| objects(data, p)).unwrap_or_default().into_iter().map(|c| node(pkg, c, depth + 1)).collect();
    let first = || Box::new(children.first().cloned().unwrap_or(SoundNode::Empty));
    let fval = |n: &str, d: f32| match find(&props, n).map(|p| &p.value) {
        Some(Value::Float(v)) => *v,
        _ => d,
    };
    let bval = |n: &str, d: bool| match find(&props, n).map(|p| &p.value) {
        Some(Value::Bool(v)) => *v,
        _ => d,
    };
    // Enum bytes are stored by name in Mirror's Edge packages.
    let eval = |n: &str, names: &[&str], d: u8| match find(&props, n).map(|p| &p.value) {
        Some(Value::Name(v)) => names.iter().position(|x| x == v).map_or(d, |i| i as u8),
        Some(Value::Byte(b)) => *b,
        _ => d,
    };
    const INTERP: [&str; 4] = ["INTERPOLATION_Linear", "INTERPOLATION_Smooth", "INTERPOLATION_Square", "INTERPOLATION_Fast"];
    match class.as_str() {
        "SoundNodeRandom" => {
            let mut weights = find(&props, "Weights").map(|p| floats(data, p)).unwrap_or_default();
            weights.resize(children.len(), 1.0);
            SoundNode::Random { weights, children }
        }
        "SoundNodeModulator" => SoundNode::Modulator {
            // USoundNodeModulator's class defaults: 0.95 .. 1.05 for both
            volume: distribution(pkg, data, &props, "VolumeModulation", (0.95, 1.05)),
            pitch: distribution(pkg, data, &props, "PitchModulation", (0.95, 1.05)),
            child: first(),
        },
        "SoundNodeMixer" => {
            let mut volumes = find(&props, "InputVolume").map(|p| floats(data, p)).unwrap_or_default();
            volumes.resize(children.len(), 1.0);
            SoundNode::Mixer { volumes, children }
        }
        "SoundNodeLooping" => SoundNode::Looping(first()),
        "SoundNodeConcatenator" => SoundNode::Concatenator(children),
        "SoundNodeDelay" => SoundNode::Delay { delay: distribution(pkg, data, &props, "DelayDuration", (0.0, 0.0)), child: first() },
        "TdSoundNodeVelocity" => SoundNode::Velocity {
            min_speed: fval("MinSpeed", 0.0),
            max_speed: fval("MaxSpeed", 0.0),
            volume: (fval("VolumeAtMinSpeed", 0.0), fval("VolumeAtMaxSpeed", 1.0)),
            pitch: (fval("PitchAtMinSpeed", 1.0), fval("PitchAtMaxSpeed", 1.2)),
            modulate_volume: bval("bModulateVolume", true),
            modulate_pitch: bval("bModulatePitch", true),
            fade_in: fval("FadeInTimeFilter", 0.0),
            fade_out: fval("FadeOutTimeFilter", 0.0),
            interp: eval("InterpolationMethod", &INTERP, 2),
            speed_type: eval("TypeOfSpeed", &["SPEEDTYPE_Source", "SPEEDTYPE_Listener", "SPEEDTYPE_Relative", "SPEEDTYPE_Custom"], 2),
            child: first(),
        },
        "TdSoundNodeADSR" => SoundNode::Adsr {
            // class defaults: DistributionAttack/Decay/Release 0, DistributionSustain 1
            attack: distribution(pkg, data, &props, "Attack", (0.0, 0.0)),
            decay: distribution(pkg, data, &props, "Decay", (0.0, 0.0)),
            sustain: distribution(pkg, data, &props, "Sustain", (1.0, 1.0)),
            release: distribution(pkg, data, &props, "Release", (0.0, 0.0)),
            methods: [eval("AttackInterpolationMethod", &INTERP, 0), eval("DecayInterpolationMethod", &INTERP, 0), eval("ReleaseInterpolationMethod", &INTERP, 0)],
            modulate_volume: bval("bModulateVolume", true),
            modulate_pitch: bval("bModulatePitch", false),
            child: first(),
        },
        _ if children.len() > 1 => SoundNode::Mixer { volumes: vec![1.0; children.len()], children },
        _ if children.len() == 1 => SoundNode::Pass(first()),
        _ => SoundNode::Empty,
    }
}

pub fn read_cue(pkg: &Package, export: usize) -> Option<Cue> {
    let (props, _) = export_props(pkg, export).ok()?;
    let f = |n: &str, d: f32| match find(&props, n).map(|p| &p.value) {
        Some(Value::Float(v)) => *v,
        _ => d,
    };
    let root = match find(&props, "FirstNode").map(|p| &p.value) {
        Some(Value::Object(o)) => node(pkg, *o, 0),
        _ => SoundNode::Empty,
    };
    Some(Cue { volume_multiplier: f("VolumeMultiplier", 0.75), pitch_multiplier: f("PitchMultiplier", 1.0), root })
}

/// Every wave a node tree can play.
pub fn waves(n: &SoundNode, out: &mut Vec<WaveRef>) {
    match n {
        SoundNode::Wave(w) => out.push(w.clone()),
        SoundNode::Random { children, .. } | SoundNode::Mixer { children, .. } | SoundNode::Concatenator(children) => {
            for c in children {
                waves(c, out);
            }
        }
        SoundNode::Modulator { child, .. } | SoundNode::Delay { child, .. } | SoundNode::Velocity { child, .. } | SoundNode::Adsr { child, .. } => waves(child, out),
        SoundNode::Looping(c) | SoundNode::Pass(c) => waves(c, out),
        SoundNode::Empty => {}
    }
}
