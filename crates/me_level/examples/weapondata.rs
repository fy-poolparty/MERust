//! Generate tdsim's weapon table from the game: each TdSharedContent weapon class's defaults
//! (merged down TdWeapon -> TdWeapon_Light / _Heavy -> the class) and [Weapons] config.
//! Prints Rust source for `tdsim::weapons::WEAPONS`.

use std::collections::HashMap;
use upk::{export_props, find, Package, Prop, Value};

const CLASSES: [&str; 11] = [
    "TdWeapon_Pistol_Glock18c",
    "TdWeapon_Pistol_Colt1911",
    "TdWeapon_Pistol_BerettaM93R",
    "TdWeapon_SMG_SteyrTMP",
    "TdWeapon_AssaultRifle_MP5K",
    "TdWeapon_AssaultRifle_FNSCARL",
    "TdWeapon_AssaultRifle_HKG36",
    "TdWeapon_Machinegun_FNMinimi",
    "TdWeapon_Shotgun_Remington870",
    "TdWeapon_Shotgun_Neostead",
    "TdWeapon_Sniper_BarretM95",
];

struct Obj<'a> {
    pkg: &'a Package,
    props: Vec<Prop>,
    data: &'a [u8],
}

fn obj<'a>(pkg: &'a Package, name: &str) -> Option<Obj<'a>> {
    let i = pkg.find_export(name, None)?;
    let (props, _) = export_props(pkg, i).ok()?;
    Some(Obj { pkg, props, data: pkg.export_bytes(i) })
}

fn ini(path: &std::path::Path) -> HashMap<String, HashMap<String, String>> {
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut sec = String::new();
    for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
        let l = line.trim();
        if l.starts_with('#') || l.starts_with(';') || l.is_empty() {
            continue;
        }
        if l.starts_with('[') {
            sec = l.trim_matches(|c| c == '[' || c == ']').to_string();
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            let v = v.split('#').next().unwrap().trim().trim_end_matches(';').to_string();
            out.entry(sec.clone()).or_default().insert(k.trim().to_string(), v);
        }
    }
    out
}

fn num(s: &str) -> f32 {
    s.trim().trim_end_matches('f').trim_end_matches('.').parse::<f32>().unwrap_or_else(|_| s.trim().trim_end_matches('f').parse().unwrap_or(0.0))
}

fn fields(s: &str) -> HashMap<String, f32> {
    s.trim_matches(|c| c == '(' || c == ')')
        .split(',')
        .filter_map(|kv| kv.split_once('=').map(|(k, v)| (k.trim().to_string(), num(v))))
        .collect()
}

fn main() {
    let install = me_level::install();
    let cooked = install.join("TdGame").join("CookedPC");
    let shared = Package::open(cooked.join("TdSharedContent.u")).unwrap();
    let tdgame = Package::open(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/unpacked/TdGame.u")).unwrap();
    let cfg = ini(&install.join("TdGame").join("Config").join("DefaultWeapons.ini"));
    let decomp = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/decomp/TdSharedContent");
    for class in CLASSES {
        let src = std::fs::read_to_string(decomp.join(format!("{class}.uc"))).unwrap_or_default();
        let heavy = src.lines().next().unwrap_or("").contains("TdWeapon_Heavy");
        let chain: Vec<Obj> = [
            obj(&shared, &format!("Default__{class}")),
            obj(&tdgame, if heavy { "Default__TdWeapon_Heavy" } else { "Default__TdWeapon_Light" }),
            obj(&tdgame, "Default__TdWeapon"),
        ]
        .into_iter()
        .flatten()
        .collect();
        let get = |n: &str| chain.iter().find_map(|o| find(&o.props, n).map(|p| (o, p)));
        let f = |n: &str, d: f32| match get(n).map(|(_, p)| p.value.clone()) {
            Some(Value::Float(x)) => x,
            Some(Value::Int(x)) => x as f32,
            _ => d,
        };
        let i = |n: &str, d: i32| match get(n).map(|(_, p)| p.value.clone()) {
            Some(Value::Int(x)) => x,
            _ => d,
        };
        let arr0 = |n: &str| get(n).and_then(|(o, p)| p.as_i32_array(o.data).first().map(|b| f32::from_bits(*b as u32)));
        let path = |n: &str| match get(n) {
            Some((o, p)) => match &p.value {
                Value::Object(r) if *r != 0 => o.pkg.object_path(*r),
                _ => {
                    let a = p.as_i32_array(o.data);
                    a.first().filter(|r| **r != 0).map(|r| o.pkg.object_path(*r)).unwrap_or_default()
                }
            },
            None => String::new(),
        };
        let raw = |n: &str| get(n).map(|(o, p)| p.bytes(o.data).to_vec()).unwrap_or_default();
        let fsa = raw("FiringStatesArray");
        let firing_state = if fsa.len() >= 12 {
            let o = get("FiringStatesArray").unwrap().0;
            o.pkg.name(upk::FName { index: i32::from_le_bytes(fsa[4..8].try_into().unwrap()), number: 0 })
        } else {
            "WeaponFiring".into()
        };
        let auto = raw("bAutomaticReFire");
        let auto_refire = auto.len() >= 5 && auto[4] != 0;
        // the 1p mesh: Mesh1p's SkeletalMesh
        let mesh = chain.iter().find_map(|o| {
            ["Mesh1p", "Mesh3p"].iter().find_map(|k| {
                let r = match find(&o.props, k).map(|p| &p.value) {
                    Some(Value::Object(r)) if *r > 0 => *r,
                    _ => return None,
                };
                let (props, _) = export_props(o.pkg, r as usize - 1).ok()?;
                match find(&props, "SkeletalMesh").map(|p| &p.value) {
                    Some(Value::Object(m)) if *m != 0 => Some(o.pkg.object_path(*m)),
                    _ => None,
                }
            })
        });
        // config
        let mut c: HashMap<String, String> = cfg.get("TdGame.TdWeapon").cloned().unwrap_or_default();
        if let Some(s) = cfg.get(&format!("TdSharedContent.{class}")) {
            c.extend(s.clone());
        }
        let cf = |k: &str, d: f32| c.get(k).map(|v| num(v)).unwrap_or(d);
        let burst = |k: &str| {
            let m = c.get(k).map(|v| fields(v)).unwrap_or_default();
            format!(
                "Burst {{ length_min: {}, length_max: {}, pause_min: {:?}, pause_max: {:?} }}",
                *m.get("Length_Min").unwrap_or(&1.0) as i32,
                *m.get("Length_Max").unwrap_or(&1.0) as i32,
                m.get("Pause_Min").copied().unwrap_or(0.5),
                m.get("Pause_Max").copied().unwrap_or(1.0)
            )
        };
        let rr = c.get("ReloadReadyTime").map(|v| fields(v)).unwrap_or_default();
        let (mesh_pkg, mesh_name) = mesh.as_deref().map(|m| (m.split('.').next().unwrap_or("").to_string(), m.rsplit('.').next().unwrap_or("").to_string())).unwrap_or_default();
        let set1p = path("AnimationSetCharacter1p");
        let ident = class.trim_start_matches("TdWeapon_").to_ascii_uppercase();
        println!("pub static {ident}: WeaponClass = WeaponClass {{");
        println!("    name: {class:?},");
        println!("    package: {mesh_pkg:?},");
        println!("    mesh: {mesh_name:?},");
        println!("    anim_set_1p: {:?},", set1p.split('.').next().unwrap_or(""));
        println!("    heavy: {heavy},");
        println!("    firing_state: {firing_state:?},");
        println!("    automatic_refire: {auto_refire},");
        println!("    burst_max: {},", i("BurstMax", 3));
        println!("    pellets: {},", i("PelletCount", 1));
        println!("    fire_interval: {:?},", arr0("FireInterval").unwrap_or(0.1));
        println!("    damage: {:?},", arr0("InstantHitDamage").unwrap_or(10.0));
        println!("    momentum: {:?},", arr0("InstantHitMomentum").unwrap_or(1.0));
        println!("    spread: {:?},", arr0("Spread").unwrap_or(0.0));
        println!("    fall_off_distance: {:?},", f("FallOffDistance", 1000.0));
        println!("    weapon_range: {:?},", f("WeaponRange", 12000.0));
        println!("    death_anim_type: {},", cf("DeathAnimType", i("DeathAnimType", 0) as f32) as i32);
        println!("    max_ammo: {},", i("MaxAmmo", 33));
        println!("    reload_time: {:?},", f("ReloadTime", 2.3));
        println!("    equip_time: {:?},", f("EquipTime", 0.33));
        println!("    pose_profile: {:?},", match get("WeaponPoseProfileName").map(|(_, p)| p.value.clone()) { Some(Value::Name(n)) => n, _ => "Default".to_string() });
        println!("    sniper_bullet: {},", path("InstantHitDamageTypes").contains("TdDmgType_Sniper_Bullet"));
        println!("    recoil_amount: {:?},", cf("RecoilAmount", f("RecoilAmount", 0.8)));
        println!("    recoil_recover_time: {:?},", cf("RecoilRecoverTime", f("RecoilRecoverTime", 0.1)));
        println!("    max_recoil: {:?},", cf("MaxRecoil", f("MaxRecoil", 6.0)));
        println!("    kickback_amount: {:?},", cf("KickbackAmount", f("KickbackAmount", 20.0)));
        println!("    combat_range_max: {:?},", cf("CombatRange_Max", f("CombatRange_Max", 3000.0)));
        println!("    bursts: [{}, {}, {}],", burst("AimedBurst_Near"), burst("AimedBurst_Mid"), burst("AimedBurst_Far"));
        println!("    pre_reload_time: {:?},", cf("PreReloadTime", f("PreReloadTime", 0.0)));
        println!("    reload_ready_time: ({:?}, {:?}),", rr.get("Time_Min").copied().unwrap_or(1.0), rr.get("Time_Max").copied().unwrap_or(1.0));
        println!("    ai_damage_multiplier: {:?},", cf("AIDamageMultiplier", f("AIDamageMultiplier", 1.0)));
        println!("    out_of_ammo_anim: {:?},", match get("OutOfAmmoAnimName").map(|(_, p)| p.value.clone()) { Some(Value::Name(n)) => n, _ => String::new() });
        println!("    fire_1p: {:?},", path("WeaponFireSnd1p"));
        println!("    fire_3p: {:?},", path("WeaponFireSnd3p"));
        println!("    reverb_1p: {:?},", path("WeaponReverbSnd1p"));
        println!("    reverb_3p: {:?},", path("WeaponReverbSnd3p"));
        println!("    click: {:?},", path("WeaponClickSnd"));
        println!("    drop: {:?},", path("WeaponCollisionSnd"));
        println!("    pickup: {:?},", path("PickupSound"));
        println!("}};\n");
    }
}
