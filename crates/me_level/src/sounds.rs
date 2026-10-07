//! The pawn's sounds from the install: SoundCues and their Ogg waves (A_* audio packages) and
//! the footstep tables of the physical materials the course uses (TDPhysicalMaterials).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use upk::props::{export_props, find, Value};
use upk::sound::{Cue, WaveData, WaveRef};
use upk::Package;

#[derive(Default)]
pub struct SoundBank {
    /// By "Package.Path".
    pub cues: HashMap<String, Cue>,
    pub waves: HashMap<WaveRef, WaveData>,
    /// TdPawn.GetFootStepSounds for (physical material, trigger id): the cues to play.
    pub footsteps: HashMap<(String, i32), Vec<String>>,
}

impl SoundBank {
    pub fn footstep_cues(&self, material: &str, id: i32) -> &[String] {
        self.footsteps.get(&(material.to_string(), id)).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

struct Packages {
    files: HashMap<String, PathBuf>,
    open: HashMap<String, Option<Package>>,
}

impl Packages {
    fn new(cooked: &Path) -> Self {
        let mut files = HashMap::new();
        let mut stack = vec![cooked.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("upk")) {
                    if let Some(stem) = p.file_stem() {
                        files.entry(stem.to_string_lossy().to_ascii_lowercase()).or_insert(p);
                    }
                }
            }
        }
        Packages { files, open: HashMap::new() }
    }

    fn get(&mut self, name: &str) -> Option<&Package> {
        let key = name.to_ascii_lowercase();
        if !self.open.contains_key(&key) {
            let pkg = self.files.get(&key).and_then(|p| Package::open(p).ok());
            self.open.insert(key.clone(), pkg);
        }
        self.open.get(&key).and_then(|p| p.as_ref())
    }
}

fn find_by_path(pkg: &Package, path: &str) -> Option<usize> {
    let last = path.rsplit('.').next().unwrap_or(path);
    (0..pkg.exports.len()).find(|&i| pkg.object_name(i as i32 + 1).eq_ignore_ascii_case(last) && pkg.object_path(i as i32 + 1).eq_ignore_ascii_case(path))
}

/// A physical material's footstep fields (`_NN_Female_*` -> cue), its parent's name and
/// bPlayOnTopOfParent.
struct FootStepTable {
    cues: HashMap<i32, String>,
    on_top_of_parent: bool,
    parent: Option<String>,
}

fn footstep_table(pkg: &Package, material: &str) -> Option<FootStepTable> {
    let pm = pkg.find_export(material, Some("PhysicalMaterial"))?;
    let (props, _) = export_props(pkg, pm).ok()?;
    let parent = match find(&props, "Parent").map(|p| &p.value) {
        Some(Value::Object(o)) if *o != 0 => Some(pkg.object_name(*o)),
        _ => None,
    };
    let mut table = FootStepTable { cues: HashMap::new(), on_top_of_parent: false, parent };
    let Some(Value::Object(prop)) = find(&props, "PhysicalMaterialProperty").map(|p| p.value.clone()) else { return Some(table) };
    if prop <= 0 {
        return Some(table);
    }
    let (pp, _) = export_props(pkg, prop as usize - 1).ok()?;
    let Some(Value::Object(fs)) = find(&pp, "TdPhysicalMaterialFootSteps").map(|p| p.value.clone()) else { return Some(table) };
    if fs <= 0 {
        return Some(table);
    }
    let (fp, _) = export_props(pkg, fs as usize - 1).ok()?;
    for p in &fp {
        if p.name == "bPlayOnTopOfParent" {
            table.on_top_of_parent = matches!(p.value, Value::Bool(true));
        }
        // TdPlayerPawn.GetSpecificFootStepSound: trigger id NN -> _NN_Female_*
        let Some(rest) = p.name.strip_prefix('_') else { continue };
        let Some((num, kind)) = rest.split_once('_') else { continue };
        if !kind.starts_with("Female_") {
            continue;
        }
        let (Ok(id), Value::Object(o)) = (num.parse::<i32>(), &p.value) else { continue };
        if *o != 0 {
            let r = upk::sound::object_ref(pkg, *o);
            table.cues.insert(id, format!("{}.{}", r.package, r.path));
        }
    }
    Some(table)
}

/// TdPawn.GetFootStepSounds + ActuallyPlayFootStepSound's parent walk and default material.
fn footstep_cues(pkg: &Package, material: &str, id: i32) -> Vec<String> {
    let collect = |start: &str| {
        let mut out = Vec::new();
        let mut m = Some(start.to_string());
        while let Some(name) = m {
            let Some(t) = footstep_table(pkg, &name) else { break };
            if let Some(c) = t.cues.get(&id) {
                out.push(c.clone());
            }
            m = if t.on_top_of_parent { t.parent } else { None };
        }
        out
    };
    let mut m = Some(material.to_string());
    while let Some(name) = m {
        let v = collect(&name);
        if !v.is_empty() {
            return v;
        }
        m = footstep_table(pkg, &name).and_then(|t| t.parent);
    }
    collect(tdsim::sound::DEFAULT_FOOTSTEP_MATERIAL)
}

/// Load every cue the pawn can play: the course materials' footsteps, the character sounds, the
/// moves' sounds and `extra` (e.g. the AnimNotify_Sound cues of the animation set).
pub fn load_sound_bank(install: &Path, extra: &[String]) -> SoundBank {
    let cooked = install.join("TdGame").join("CookedPC");
    let mut pk = Packages::new(&cooked);
    let mut bank = SoundBank::default();
    let mut wanted: Vec<String> = extra.to_vec();
    if let Some(pm) = pk.get("TDPhysicalMaterials") {
        for mat in [tdsim::sound::DEFAULT_FOOTSTEP_MATERIAL, tdsim::sound::SOFT_LANDING_MATERIAL] {
            for id in (1..=11).chain(21..=26).chain(31..=36) {
                let cues = footstep_cues(pm, mat, id);
                wanted.extend(cues.iter().cloned());
                bank.footsteps.insert((mat.to_string(), id), cues);
            }
        }
    }
    wanted.extend(tdsim::sound::CHARACTER_SOUND_CUES.iter().flatten().map(|s| s.to_string()));
    for c in [
        tdsim::sound::CLIMB_DOWN_LADDER_FAST_SOUND,
        tdsim::sound::CLIMB_DOWN_PIPE_FAST_SOUND,
        tdsim::sound::DEATH_FALL_SOUND,
        tdsim::sound::DEATH_IMPACT_SOUND,
        tdsim::sound::WIND_SOUND,
        tdsim::moves::swing::SWING_SOUND,
        tdsim::moves::zipline::ZIPPING_SOUND,
    ] {
        wanted.push(c.to_string());
    }
    for c in tdsim::sound::MELEE_IMPACT_SOUNDS.iter().flatten() {
        wanted.push(c.to_string());
    }
    wanted.sort();
    wanted.dedup();
    for key in wanted {
        let Some((pname, path)) = key.split_once('.') else { continue };
        let Some(pkg) = pk.get(pname) else { continue };
        let Some(i) = find_by_path(pkg, path) else { continue };
        let Some(cue) = upk::sound::read_cue(pkg, i) else { continue };
        let mut ws = Vec::new();
        upk::sound::waves(&cue.root, &mut ws);
        bank.cues.insert(key.clone(), cue);
        for w in ws {
            if bank.waves.contains_key(&w) {
                continue;
            }
            let Some(wp) = pk.get(&w.package) else { continue };
            let Some(wi) = find_by_path(wp, &w.path) else { continue };
            if let Some(data) = upk::sound::read_wave(wp, wi) {
                bank.waves.insert(w, data);
            }
        }
    }
    bank
}

/// The AnimNotify_Sound cues of an animation set.
pub fn notify_cues(set: &upk::anim::AnimSet) -> Vec<String> {
    let mut v: Vec<String> = set
        .seqs
        .iter()
        .flat_map(|s| s.notifies.iter())
        .filter_map(|n| match &n.kind {
            upk::anim::NotifyKind::Sound { package, path } => Some(format!("{package}.{path}")),
            _ => None,
        })
        .collect();
    v.sort();
    v.dedup();
    v
}
