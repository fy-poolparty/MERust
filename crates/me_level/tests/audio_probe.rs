#[test]
#[ignore]
fn wave_probe() {
    let p = &me_level::install().join("TdGame").join("CookedPC").join("Audio").join("A_Material_Footstep.upk");
    let pkg = upk::Package::open(p).unwrap();
    let idx = pkg.find_export("ConcreteDirtFootStepWalk_01", None).unwrap();
    let data = pkg.export_bytes(idx);
    let (_, end) = upk::props::export_props(&pkg, idx).unwrap();
    let mut at = end;
    for name in ["Raw", "PC", "Xbox", "PS3"] {
        let u = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
        let (flags, count, size, off) = (u(at), u(at + 4), u(at + 8), u(at + 12));
        at += 16;
        let head: Vec<u8> = data[at..(at + 8).min(data.len())].to_vec();
        println!("{name}: flags {flags:#x} count {count} size {size} off {off} head {:?} {:?}", head, String::from_utf8_lossy(&head));
        if flags & 0x21 == 0 {
            at += size as usize;
        }
    }
    println!("end {at} of {}", data.len());
}

#[test]
#[ignore]
fn dist_probe() {
    use upk::props::{export_props, find, read_props, Value};
    let p = &me_level::install().join("TdGame").join("CookedPC").join("Audio").join("A_Material_Footstep.upk");
    let pkg = upk::Package::open(p).unwrap();
    let idx: usize = std::env::var("IDX").unwrap().parse::<usize>().unwrap() - 1;
    let data = pkg.export_bytes(idx);
    let (props, _) = export_props(&pkg, idx).unwrap();
    for pr in &props {
        if pr.struct_name == "RawDistributionFloat" {
            let (sub, _) = read_props(&pkg, data, pr.start).unwrap();
            for s in &sub {
                match &s.value {
                    Value::Raw => {
                        let fl: Vec<f32> = (0..s.size / 4).map(|i| f32::from_le_bytes(data[s.start + i * 4..s.start + i * 4 + 4].try_into().unwrap())).collect();
                        println!("{}.{} ({}): {:?}", pr.name, s.name, s.ty, fl);
                    }
                    v => println!("{}.{} = {:?}", pr.name, s.name, v),
                }
            }
        } else {
            println!("{} = {:?}", pr.name, pr.value);
        }
    }
    let _ = find(&props, "x");
}

#[test]
#[ignore]
fn notify_probe() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    for n in ["runfwd", "runfwdstiff", "walkfwd", "SprintFwd", "Stand", "JumpLand", "crouchfwd", "WallrunLeft", "HangTurnJump", "LadderClimbUpLeftHand"] {
        if let Some(s) = a.set.seqs.iter().find(|s| s.name.eq_ignore_ascii_case(n)) {
            println!("{n} ({:.2}s): {:?}", s.length, s.notifies.iter().map(|x| (format!("{:.2}", x.time), format!("{:?}", x.kind))).collect::<Vec<_>>());
        }
    }
    let p = &me_level::install().join("TdGame").join("CookedPC").join("Audio").join("A_Material_Footstep.upk");
    let pkg = upk::Package::open(p).unwrap();
    let i = pkg.exports.iter().enumerate().position(|(i, _)| pkg.object_path(i as i32 + 1) == "Concrete._03_Female_FootStepRun").unwrap();
    let cue = upk::sound::read_cue(&pkg, i).unwrap();
    let mut w = Vec::new();
    upk::sound::waves(&cue.root, &mut w);
    println!("cue vol {} waves {} first {:?}\n{:?}", cue.volume_multiplier, w.len(), w.first(), cue.root);
}

#[test]
fn sound_bank_loads() {
    let install = me_level::install();
    let Ok(a) = me_level::anims::load_player_anims(install) else { return };
    let t = std::time::Instant::now();
    let bank = me_level::sounds::load_sound_bank(install, &me_level::sounds::notify_cues(&a.set));
    println!("{} cues, {} waves ({:.1} MB) in {:?}", bank.cues.len(), bank.waves.len(), bank.waves.values().map(|w| w.ogg.len()).sum::<usize>() as f32 / 1e6, t.elapsed());
    for (m, id) in [("PM_Concrete", 3), ("PM_Concrete", 8), ("PM_Concrete", 36), ("PM_Plastic_PropLarge", 9), ("PM_Plastic_PropLarge", 3)] {
        println!("{m} {id}: {:?}", bank.footstep_cues(m, id));
    }
    assert!(!bank.footstep_cues("PM_Concrete", 3).is_empty());
    assert!(bank.cues.contains_key(tdsim::sound::WIND_SOUND));
    let missing: Vec<_> = tdsim::sound::CHARACTER_SOUND_CUES.iter().flatten().filter(|c| !bank.cues.contains_key(**c)).collect();
    assert!(missing.is_empty(), "missing {missing:?}");
}

#[test]
#[ignore]
fn notify_cue_list() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    for c in me_level::sounds::notify_cues(&a.set) { println!("{c}"); }
    for s in &a.set.seqs {
        if s.notifies.iter().any(|n| matches!(&n.kind, upk::anim::NotifyKind::Sound { path, .. } if path.contains("Plastic"))) { println!("plastic in {}", s.name); }
    }
}

/// Cooked LookupTable vs the Distribution object, for every modulator in a package (PKG env).
#[test]
#[ignore]
fn modulator_tables() {
    use upk::props::{export_props, find, read_props, Value};
    let dir = &me_level::install().join("TdGame").join("CookedPC").join("Audio");
    let name = std::env::var("PKG").unwrap_or("A_Material_Footstep".into());
    let pkg = upk::Package::open(dir.join(format!("{name}.upk"))).unwrap();
    let mut shown = 0;
    for i in 0..pkg.exports.len() {
        if pkg.export_class(i) != "SoundNodeModulator" || shown > 25 {
            continue;
        }
        let data = pkg.export_bytes(i);
        let (props, _) = export_props(&pkg, i).unwrap();
        for which in ["VolumeModulation", "PitchModulation"] {
            let Some(p) = find(&props, which) else { continue };
            let (sub, _) = read_props(&pkg, data, p.start).unwrap();
            let table: Vec<f32> = find(&sub, "LookupTable").map(|t| {
                let n = i32::from_le_bytes(data[t.start..t.start + 4].try_into().unwrap()) as usize;
                (0..n).map(|k| f32::from_le_bytes(data[t.start + 4 + k * 4..t.start + 8 + k * 4].try_into().unwrap())).collect()
            }).unwrap_or_default();
            let others: Vec<String> = sub.iter().filter(|s| s.name != "LookupTable").map(|s| format!("{}={:?}", s.name, s.value)).collect();
            let dist = match find(&sub, "Distribution").map(|d| d.value.clone()) {
                Some(Value::Object(o)) if o > 0 => {
                    let (dp, _) = export_props(&pkg, o as usize - 1).unwrap();
                    format!("{} {:?}", pkg.export_class(o as usize - 1), dp.iter().map(|x| format!("{}={:?}", x.name, x.value)).collect::<Vec<_>>())
                }
                _ => "-".into(),
            };
            println!("{} {which}: table {table:?} {others:?} dist {dist}", pkg.object_path(i as i32 + 1));
        }
        shown += 1;
    }
}
