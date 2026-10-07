//! UE3 (ArVer 536) AnimSet / AnimSequence decoding: ConstantKeyLerp tracks with the compressed
//! formats Mirror's Edge uses (rotation: Float96NoW, Fixed48NoW, IntervalFixed32NoW, Fixed32NoW,
//! Float32NoW; translation: None / Float96NoW). Before ArVer 761 every multi-key rotation track
//! starts with Mins/Ranges. Keys are spread evenly over SequenceLength.

use crate::package::Package;
use crate::props::{Value, export_props, find};
use crate::reader::Reader;

#[derive(Clone, Debug, Default)]
pub struct Track {
    /// Translation keys (uu, bone-local).
    pub pos: Vec<[f32; 3]>,
    /// Rotation keys as quaternions (x, y, z, w), Unreal convention.
    pub rot: Vec<[f32; 4]>,
}

#[derive(Clone, Debug, Default)]
pub struct AnimSeq {
    pub name: String,
    pub length: f32,
    pub num_frames: i32,
    pub rate_scale: f32,
    pub tracks: Vec<Track>,
    /// AnimSequence.Notifies (AnimNotifyEvent: Time, Notify), sorted by time.
    pub notifies: Vec<AnimNotify>,
}

#[derive(Clone, Debug)]
pub struct AnimNotify {
    pub time: f32,
    pub kind: NotifyKind,
}

#[derive(Clone, Debug)]
pub enum NotifyKind {
    /// AnimNotify_Footstep.FootDown (TdPawn.PlayFootStepSound trigger id, negative = left).
    Footstep(i32),
    /// AnimNotify_Sound.SoundCue as (package, object path).
    Sound { package: String, path: String },
    /// TdAnimNotify_CharacterSound.TriggerType (CharacterSoundTriggerType).
    CharacterSound(u8),
    /// AnimNotify_Script.NotifyName.
    Script(String),
    Other(String),
}

/// TdAnimNotify_CharacterSound.CharacterSoundTriggerType names, in enum order.
const CHARACTER_SOUND_TYPES: [&str; 22] = [
    "ECSBreath_Soft_Short", "ECSBreath_Soft_Long", "ECSBreath_Medium_Short", "ECSBreath_Medium_Long",
    "ECSBreath_Hard_Short", "ECSBreath_Hard_Long", "ECSBreath_Jump", "ECSBreath_Snatch",
    "ECSOral_Impact_Soft", "ECSOral_Impact_Medium", "ECSOral_Impact_Hard", "ECSOral_Strain_Soft",
    "ECSOral_Strain_Medium", "ECSOral_Strain_Hard", "ECSOral_Jump", "ECSOral_Snatch", "ECSOral_Vault",
    "ECSOral_Die", "ECSClothing_Crouch", "ECSClothing_Walk", "ECSClothing_Run", "ECSMisc_Vault",
];

fn read_notifies(pkg: &Package, data: &[u8], props: &[crate::props::Prop]) -> Vec<AnimNotify> {
    let Some(p) = find(props, "Notifies") else { return Vec::new() };
    let mut out = Vec::new();
    for ev in crate::props::struct_array(pkg, data, p) {
        let time = match find(&ev, "Time").map(|p| &p.value) {
            Some(Value::Float(t)) => *t,
            _ => 0.0,
        };
        let Some(Value::Object(o)) = find(&ev, "Notify").map(|p| p.value.clone()) else { continue };
        if o <= 0 {
            continue;
        }
        let class = pkg.class_of(o);
        let Ok((np, _)) = export_props(pkg, o as usize - 1) else { continue };
        let kind = match class.as_str() {
            "AnimNotify_Footstep" => NotifyKind::Footstep(match find(&np, "FootDown").map(|p| &p.value) {
                Some(Value::Int(v)) => *v,
                _ => 0,
            }),
            "AnimNotify_Sound" => match find(&np, "SoundCue").map(|p| &p.value) {
                Some(Value::Object(c)) if *c != 0 => {
                    let r = crate::sound::object_ref(pkg, *c);
                    NotifyKind::Sound { package: r.package, path: r.path }
                }
                _ => continue,
            },
            "TdAnimNotify_CharacterSound" => NotifyKind::CharacterSound(match find(&np, "TriggerType").map(|p| &p.value) {
                Some(Value::Name(n)) => CHARACTER_SOUND_TYPES.iter().position(|t| t == n).unwrap_or(1) as u8,
                // the class default, ECSBreath_Soft_Long
                _ => 1,
            }),
            "AnimNotify_Script" => NotifyKind::Script(match find(&np, "NotifyName").map(|p| &p.value) {
                Some(Value::Name(n)) => n.clone(),
                _ => String::new(),
            }),
            c => NotifyKind::Other(c.to_string()),
        };
        out.push(AnimNotify { time, kind });
    }
    out.sort_by(|a, b| a.time.total_cmp(&b.time));
    out
}

#[derive(Clone, Debug, Default)]
pub struct AnimSet {
    pub name: String,
    /// bAnimRotationOnly: tracks other than the root only supply rotation; translation comes
    /// from the skeleton's reference pose.
    pub anim_rotation_only: bool,
    pub track_bone_names: Vec<String>,
    pub seqs: Vec<AnimSeq>,
}

fn format_code(name: &str) -> u8 {
    match name {
        "ACF_None" => 0,
        "ACF_Float96NoW" => 1,
        "ACF_Fixed48NoW" => 2,
        "ACF_IntervalFixed32NoW" => 3,
        "ACF_Fixed32NoW" => 4,
        "ACF_Float32NoW" => 5,
        "ACF_Identity" => 6,
        _ => 255,
    }
}

fn quat_w(x: f32, y: f32, z: f32) -> [f32; 4] {
    let w2 = 1.0 - x * x - y * y - z * z;
    [x, y, z, if w2 > 0.0 { w2.sqrt() } else { 0.0 }]
}

fn read_rot(r: &mut Reader, fmt: u8, mins: [f32; 3], ranges: [f32; 3]) -> Option<[f32; 4]> {
    Some(match fmt {
        0 => {
            let (x, y, z, w) = (r.f32().ok()?, r.f32().ok()?, r.f32().ok()?, r.f32().ok()?);
            [x, y, z, w]
        }
        1 => {
            let v = r.vec3().ok()?;
            quat_w(v[0], v[1], v[2])
        }
        2 => {
            let (a, b, c) = (r.u16().ok()?, r.u16().ok()?, r.u16().ok()?);
            let f = |v: u16| (v as i32 - 32767) as f32 / 32767.0;
            quat_w(f(a), f(b), f(c))
        }
        3 => {
            // IntervalFixed32NoW: 11/11/10 bits mapped onto Mins..Mins+Ranges
            let v = r.u32().ok()?;
            let x = ((v >> 21) as i32 - 1023) as f32 / 1023.0;
            let y = (((v >> 10) & 0x7FF) as i32 - 1023) as f32 / 1023.0;
            let z = ((v & 0x3FF) as i32 - 511) as f32 / 511.0;
            quat_w(x * ranges[0] + mins[0], y * ranges[1] + mins[1], z * ranges[2] + mins[2])
        }
        4 => {
            let v = r.u32().ok()?;
            let x = ((v >> 21) as i32 - 1023) as f32 / 1023.0;
            let y = (((v >> 10) & 0x7FF) as i32 - 1023) as f32 / 1023.0;
            let z = ((v & 0x3FF) as i32 - 511) as f32 / 511.0;
            quat_w(x, y, z)
        }
        5 => {
            // Float32NoW: 11/11/10-bit floats
            let v = r.u32().ok()?;
            let unpack = |bits: u32, mant: u32| -> f32 {
                // UE3 FQuatFloat32NoW: sign + 3-bit exponent + mantissa per component
                let m = bits & ((1 << mant) - 1);
                let e = (bits >> mant) & 7;
                let s = bits >> (mant + 3);
                if e == 0 && m == 0 {
                    return 0.0;
                }
                let exp = e as i32 - 3 + 127;
                let f = f32::from_bits(((s & 1) << 31) | ((exp as u32) << 23) | (m << (23 - mant)));
                f
            };
            quat_w(unpack(v >> 21, 7), unpack((v >> 10) & 0x7FF, 7), unpack(v & 0x3FF, 6))
        }
        6 => [0.0, 0.0, 0.0, 1.0],
        _ => return None,
    })
}

/// Decode one AnimSequence export.
pub fn read_anim_sequence(pkg: &Package, export: usize, num_tracks: usize) -> Option<AnimSeq> {
    let (props, end) = export_props(pkg, export).ok()?;
    let data = pkg.export_bytes(export);
    let name = match find(&props, "SequenceName").map(|p| &p.value) {
        Some(Value::Name(n)) => n.clone(),
        _ => pkg.object_name(export as i32 + 1),
    };
    let f = |n: &str, d: f32| match find(&props, n).map(|p| &p.value) {
        Some(Value::Float(v)) => *v,
        _ => d,
    };
    let length = f("SequenceLength", 0.0);
    let rate_scale = f("RateScale", 1.0);
    let num_frames = match find(&props, "NumFrames").map(|p| &p.value) {
        Some(Value::Int(v)) => *v,
        _ => 0,
    };
    let fmt = |n: &str| match find(&props, n).map(|p| &p.value) {
        Some(Value::Name(s)) => format_code(s),
        Some(Value::Byte(b)) => *b,
        _ => 0,
    };
    let rot_fmt = fmt("RotationCompressionFormat");
    let trans_fmt = fmt("TranslationCompressionFormat");
    let offsets = find(&props, "CompressedTrackOffsets").map(|p| p.as_i32_array(data)).unwrap_or_default();
    // CompressedByteStream: TArray<BYTE> right after the properties
    let mut r = Reader::at(data, end);
    let n = r.count(1 << 26).ok()?;
    let stream = r.bytes(n).ok()?;

    let mut tracks = Vec::with_capacity(num_tracks);
    for t in 0..num_tracks {
        let o = &offsets[(t * 4).min(offsets.len())..];
        if o.len() < 4 {
            tracks.push(Track::default());
            continue;
        }
        let (toff, tkeys, roff, rkeys) = (o[0] as usize, o[1] as usize, o[2] as usize, o[3] as usize);
        let mut track = Track::default();
        if tkeys > 0 {
            let mut tr = Reader::at(stream, toff);
            let tf = if tkeys == 1 { 0 } else { trans_fmt };
            for _ in 0..tkeys {
                match tf {
                    0 | 1 => track.pos.push(tr.vec3().ok()?),
                    6 => track.pos.push([0.0; 3]),
                    _ => return None,
                }
            }
        }
        if rkeys > 0 {
            let mut rr = Reader::at(stream, roff);
            let (mut mins, mut ranges) = ([0.0; 3], [0.0; 3]);
            let rf = if rkeys == 1 {
                1
            } else {
                mins = rr.vec3().ok()?;
                ranges = rr.vec3().ok()?;
                rot_fmt
            };
            for _ in 0..rkeys {
                track.rot.push(read_rot(&mut rr, rf, mins, ranges)?);
            }
        }
        tracks.push(track);
    }
    let notifies = read_notifies(pkg, pkg.export_bytes(export), &props);
    Some(AnimSeq { name, length, num_frames, rate_scale, tracks, notifies })
}

/// Read an AnimSet export (TrackBoneNames + every sequence it lists).
pub fn read_anim_set(pkg: &Package, export: usize) -> Option<AnimSet> {
    let (props, _) = export_props(pkg, export).ok()?;
    let data = pkg.export_bytes(export);
    let mut bones = Vec::new();
    if let Some(p) = find(&props, "TrackBoneNames") {
        let mut r = Reader::at(data, p.start);
        let n = r.count(4096).ok()?;
        for _ in 0..n {
            let idx = r.i32().ok()?;
            let num = r.i32().ok()?;
            let mut s = pkg.names.get(idx as usize).cloned().unwrap_or_default();
            if num > 0 {
                s = format!("{s}_{}", num - 1);
            }
            bones.push(s);
        }
    }
    let seq_refs = find(&props, "Sequences").map(|p| p.as_i32_array(data)).unwrap_or_default();
    let mut seqs = Vec::new();
    for o in seq_refs {
        if o > 0 {
            if let Some(s) = read_anim_sequence(pkg, o as usize - 1, bones.len()) {
                seqs.push(s);
            }
        }
    }
    let anim_rotation_only = find(&props, "bAnimRotationOnly").map(|p| matches!(p.value, crate::props::Value::Bool(true))).unwrap_or(false);
    Some(AnimSet { name: pkg.object_name(export as i32 + 1), anim_rotation_only, track_bone_names: bones, seqs })
}

impl AnimSeq {
    /// Sample a track at `time` (seconds): ConstantKeyLerp between evenly spaced keys.
    pub fn sample(&self, track: usize, time: f32) -> ([f32; 3], [f32; 4]) {
        let t = &self.tracks[track];
        let pos = sample_keys(&t.pos, time, self.length, |a, b, f| [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f]).unwrap_or([0.0; 3]);
        let rot = sample_keys(&t.rot, time, self.length, |a, b, f| {
            // nlerp along the shortest arc
            let d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
            let s = if d < 0.0 { -1.0 } else { 1.0 };
            let q = [a[0] + (s * b[0] - a[0]) * f, a[1] + (s * b[1] - a[1]) * f, a[2] + (s * b[2] - a[2]) * f, a[3] + (s * b[3] - a[3]) * f];
            let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-8);
            [q[0] / l, q[1] / l, q[2] / l, q[3] / l]
        })
        .unwrap_or([0.0, 0.0, 0.0, 1.0]);
        (pos, rot)
    }
}

fn sample_keys<T: Copy>(keys: &[T], time: f32, length: f32, lerp: impl Fn(T, T, f32) -> T) -> Option<T> {
    match keys.len() {
        0 => None,
        1 => Some(keys[0]),
        n => {
            let rel = if length > 0.0 { (time / length).clamp(0.0, 1.0) } else { 0.0 };
            let kp = rel * (n - 1) as f32;
            let i = (kp.floor() as usize).min(n - 2);
            Some(lerp(keys[i], keys[i + 1], kp - i as f32))
        }
    }
}
