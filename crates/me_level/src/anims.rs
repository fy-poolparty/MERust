//! The player's first-person animations from the install: AS_C1P_Unarmed decoded with `upk`,
//! turned into the timing + root-motion library `tdsim` needs.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tdsim::anim::{AnimLib, AnimSeqInfo, RootTrack};
use tdsim::{Rotator, Vec3};
use upk::Package;

pub struct PlayerAnims {
    pub lib: AnimLib,
    pub set: upk::anim::AnimSet,
    pub upper: upk::skelmesh::SkelMesh,
}

fn cooked(install: &Path) -> std::path::PathBuf {
    install.join("TdGame").join("CookedPC")
}

pub fn load_player_anims(install: &Path) -> Result<PlayerAnims, Box<dyn std::error::Error>> {
    let c = cooked(install);
    let anim_pkg = Package::open(c.join("Animations").join("AS_C1P_Unarmed.upk"))?;
    let set_idx = anim_pkg.find_export("AS_C1P_Unarmed", None).ok_or("AS_C1P_Unarmed not found")?;
    let set = upk::anim::read_anim_set(&anim_pkg, set_idx).ok_or("could not decode AS_C1P_Unarmed")?;
    let mesh_pkg = Package::open(c.join("Characters").join("CH_TKY_Crim_Fixer_1P.upk"))?;
    let mesh_idx = mesh_pkg.find_export("SK_UpperBody", Some("SkeletalMesh")).ok_or("SK_UpperBody not found")?;
    let (upper, _) = upk::skelmesh::read_skeleton(&mesh_pkg, mesh_idx)?;
    let lib = anim_lib(&set, &upper);
    Ok(PlayerAnims { lib, set, upper })
}

/// A patrol cop's animations and skeleton (AS_AI_PatrolCop_OneHanded, SK_TKY_Cop_Patrol).
pub struct BotAnims {
    pub lib: AnimLib,
    pub set: upk::anim::AnimSet,
    pub skel: upk::skelmesh::SkelMesh,
}

pub fn load_bot_anims(install: &Path) -> Result<BotAnims, Box<dyn std::error::Error>> {
    let c = cooked(install);
    let anim_pkg = Package::open(c.join("Animations").join("AS_AI_PatrolCop_OneHanded.upk"))?;
    let set_idx = anim_pkg.find_export("AS_AI_PatrolCop_OneHanded", Some("TdAnimSet")).ok_or("AS_AI_PatrolCop_OneHanded not found")?;
    let set = upk::anim::read_anim_set(&anim_pkg, set_idx).ok_or("could not decode AS_AI_PatrolCop_OneHanded")?;
    let mesh_pkg = Package::open(c.join("Characters").join("CH_TKY_Cop_Patrol.upk"))?;
    let mesh_idx = mesh_pkg.find_export("SK_TKY_Cop_Patrol", Some("SkeletalMesh")).ok_or("SK_TKY_Cop_Patrol not found")?;
    let (skel, _) = upk::skelmesh::read_skeleton(&mesh_pkg, mesh_idx)?;
    let lib = anim_lib(&set, &skel);
    Ok(BotAnims { lib, set, skel })
}

/// The sim's view of an AnimSet: lengths, root tracks (actor-space yaw through the mesh's
/// RotOrigin) and notifies.
pub fn anim_lib(set: &upk::anim::AnimSet, mesh: &upk::skelmesh::SkelMesh) -> AnimLib {
    let root_track = set.track_bone_names.iter().position(|b| b.eq_ignore_ascii_case("root")).unwrap_or(0);
    let r = mesh.rot_origin;
    let mesh_rot = Rotator::new(r[0], r[1], r[2]);
    let mut seqs = HashMap::new();
    for s in &set.seqs {
        let root = s.tracks.get(root_track).map(|t| {
            let key_times = |n: usize| -> Vec<f32> { (0..n).map(|i| if n > 1 { i as f32 * s.length / (n - 1) as f32 } else { 0.0 }).collect() };
            RootTrack {
                times: key_times(t.pos.len()),
                translation: t.pos.iter().map(|p| Vec3::from(*p)).collect(),
                yaw_times: key_times(t.rot.len()),
                yaw: root_yaw_track(&t.rot, mesh_rot),
            }
        });
        seqs.insert(s.name.to_ascii_lowercase(), AnimSeqInfo { length: s.length, rate_scale: s.rate_scale, root, notifies: notifies(s) });
    }
    AnimLib { seqs: Arc::new(seqs), mesh_rot }
}

/// An AnimSequence's notifies in the sim's terms (sound triggers only).
pub fn notifies(s: &upk::anim::AnimSeq) -> Vec<(f32, tdsim::anim::Notify)> {
    use upk::anim::NotifyKind as K;
    s.notifies
        .iter()
        .filter_map(|n| {
            let k = match &n.kind {
                K::Footstep(id) => tdsim::anim::Notify::Footstep(*id),
                K::Sound { package, path } => tdsim::anim::Notify::Cue(format!("{package}.{path}")),
                K::CharacterSound(t) => tdsim::anim::Notify::CharacterSound(*t),
                _ => return None,
            };
            Some((n.time, k))
        })
        .collect()
}

/// Rotate `v` by the Unreal quaternion `q` (x, y, z, w).
fn quat_rotate(q: [f32; 4], v: Vec3) -> Vec3 {
    let u = Vec3::new(q[0], q[1], q[2]);
    let w = q[3];
    let t = u.cross(v) * 2.0;
    v + t * w + u.cross(t)
}

/// Actor-space yaw of the root bone per rotation key: the mesh's forward axis (+Z, which
/// RotOrigin maps to actor +X) rotated by the key, then through RotOrigin. Unwrapped so a
/// 180-degree turn accumulates instead of wrapping.
fn root_yaw_track(rot: &[[f32; 4]], mesh_rot: Rotator) -> Vec<i32> {
    let (mx, my, mz) = mesh_rot.axes();
    let mut out: Vec<i32> = Vec::with_capacity(rot.len());
    for q in rot {
        let f = quat_rotate(*q, Vec3::new(0.0, 0.0, 1.0));
        let a = mx * f.x + my * f.y + mz * f.z;
        let yaw = (a.y.atan2(a.x) * 32768.0 / std::f32::consts::PI) as i32;
        let yaw = match out.last() {
            Some(&prev) => prev + tdsim::math::norm_axis(yaw - prev),
            None => yaw,
        };
        out.push(yaw);
    }
    // relative to the first key
    if let Some(&y0) = out.first() {
        for y in &mut out {
            *y -= y0;
        }
    }
    out
}

/// Faith's third-person rig for the shadow: AS_F3P_Unarmed and SK_TKY_Crim_Fixer's skeleton
/// (TdPlayerPawn.Mesh3p, bOwnerNoSee with hidden shadows).
pub fn load_player_3p(install: &Path) -> Result<(upk::anim::AnimSet, upk::skelmesh::SkelMesh), Box<dyn std::error::Error>> {
    let c = cooked(install);
    let anim_pkg = Package::open(c.join("Animations").join("AS_F3P_Unarmed.upk"))?;
    let set_idx = anim_pkg.find_export("AS_F3P_Unarmed", None).ok_or("AS_F3P_Unarmed not found")?;
    let set = upk::anim::read_anim_set(&anim_pkg, set_idx).ok_or("could not decode AS_F3P_Unarmed")?;
    let mesh_pkg = Package::open(c.join("Characters").join("CH_TKY_Crim_Fixer.upk"))?;
    let mesh_idx = mesh_pkg.find_export("SK_TKY_Crim_Fixer", Some("SkeletalMesh")).ok_or("SK_TKY_Crim_Fixer not found")?;
    let (skel, _) = upk::skelmesh::read_skeleton(&mesh_pkg, mesh_idx)?;
    Ok((set, skel))
}

/// One AimComponent of an AnimNodeAimOffset profile: the bone and its mesh-space offsets for
/// the nine aim directions LU, LC, LD, CU, CC, CD, RU, RC, RD (Unreal quaternion x, y, z, w
/// and translation).
#[derive(Clone, Debug)]
pub struct AimComponent {
    pub bone: String,
    pub rot: [[f32; 4]; 9],
    pub trans: [[f32; 3]; 9],
}

/// The first profile of an AimOffset-type node (TdAnimNodeAimOffset / TdAnimNodeDirBone) in
/// AT_C1P.
pub fn load_aim_profile(install: &Path, node: &str) -> Result<Vec<AimComponent>, Box<dyn std::error::Error>> {
    load_aim_profile_in(install, "AT_C1P", node)
}

/// `load_aim_profile` from another AnimTree package (AT_C3P for the third-person body).
pub fn load_aim_profile_in(install: &Path, tree: &str, node: &str) -> Result<Vec<AimComponent>, Box<dyn std::error::Error>> {
    use upk::props::{export_props, find, read_props, struct_array, Value};
    let pkg = Package::open(cooked(install).join("Characters").join(format!("{tree}.upk")))?;
    let idx = pkg.find_export(node, None).ok_or("aim node not found")?;
    let data = pkg.export_bytes(idx);
    let (props, _) = export_props(&pkg, idx)?;
    let profiles = struct_array(&pkg, data, find(&props, "Profiles").ok_or("no Profiles")?);
    let profile = profiles.first().ok_or("no profile")?;
    let comps = struct_array(&pkg, data, find(profile, "AimComponents").ok_or("no AimComponents")?);
    let floats = |d: &[u8], at: usize, n: usize| -> Vec<f32> { (0..n).map(|i| f32::from_le_bytes(d[at + i * 4..at + i * 4 + 4].try_into().unwrap())).collect() };
    let mut out = Vec::new();
    for c in comps {
        let bone = match find(&c, "BoneName").map(|p| &p.value) {
            Some(Value::Name(n)) => n.clone(),
            _ => continue,
        };
        let mut rot = [[0.0, 0.0, 0.0, 1.0]; 9];
        let mut trans = [[0.0; 3]; 9];
        for (i, key) in ["LU", "LC", "LD", "CU", "CC", "CD", "RU", "RC", "RD"].iter().enumerate() {
            let Some(t) = find(&c, key) else { continue };
            let (tp, _) = read_props(&pkg, data, t.start)?;
            if let Some(q) = find(&tp, "Quaternion") {
                let v = floats(data, q.start, 4);
                rot[i] = [v[0], v[1], v[2], v[3]];
            }
            if let Some(q) = find(&tp, "Translation") {
                let v = floats(data, q.start, 3);
                trans[i] = [v[0], v[1], v[2]];
            }
        }
        out.push(AimComponent { bone, rot, trans });
    }
    Ok(out)
}

/// Re-key an AnimSet's sequences to another set's TrackBoneNames (missing bones get empty
/// tracks: they keep the reference pose).
pub fn remap_seqs(from: &upk::anim::AnimSet, to_tracks: &[String]) -> Vec<upk::anim::AnimSeq> {
    let map: Vec<Option<usize>> = to_tracks.iter().map(|n| from.track_bone_names.iter().position(|t| t.eq_ignore_ascii_case(n))).collect();
    from.seqs
        .iter()
        .map(|s| {
            let mut s2 = s.clone();
            s2.tracks = map.iter().map(|m| m.and_then(|k| s.tracks.get(k).cloned()).unwrap_or(upk::anim::Track { pos: Vec::new(), rot: Vec::new() })).collect();
            s2
        })
        .collect()
}

/// Merge `over`'s sequences into `set` (same names replaced), adding the tracks only `over`
/// has (the Minimi set's Belt_Joint tracks aren't in the common set) as empty tracks of the
/// existing sequences.
fn merge_set(set: &mut upk::anim::AnimSet, over: &upk::anim::AnimSet) {
    for n in &over.track_bone_names {
        if !set.track_bone_names.iter().any(|t| t.eq_ignore_ascii_case(n)) {
            set.track_bone_names.push(n.clone());
            for s in set.seqs.iter_mut() {
                s.tracks.push(upk::anim::Track { pos: Vec::new(), rot: Vec::new() });
            }
        }
    }
    for q in remap_seqs(over, &set.track_bone_names) {
        match set.seqs.iter_mut().find(|e| e.name.eq_ignore_ascii_case(&q.name)) {
            Some(e) => *e = q,
            None => set.seqs.push(q),
        }
    }
}

/// A bot's gun pose set: the cop's anim set (its Custom_Weapon / snatch anims carry the Wep_
/// tracks) with WeaponPose from the gun's own third-person set (TdWeapon.UpdateAnimSets:
/// Mesh3p.AnimSets[1] = AnimationSetFemale3p, AS_F3P_<grip>_<gun>; bots have no
/// CommonArmed*3p), which AT_Weapon_Default plays under the slot.
pub fn bot_weapon_pose_set(install: &Path, cop_set: &upk::anim::AnimSet, class: &tdsim::weapons::WeaponClass) -> upk::anim::AnimSet {
    let mut set = cop_set.clone();
    let f3p = class.anim_set_1p.replacen("AS_C1P_", "AS_F3P_", 1);
    if let Ok(mut s) = load_set(&cooked(install), &f3p, &f3p) {
        s.seqs.retain(|q| q.name.eq_ignore_ascii_case("WeaponPose"));
        merge_set(&mut set, &s);
    }
    set
}

fn load_set(c: &Path, pkg: &str, set: &str) -> Result<upk::anim::AnimSet, Box<dyn std::error::Error>> {
    let p = Package::open(c.join("Animations").join(format!("{pkg}.upk")))?;
    let i = p.find_export(set, Some("TdAnimSet")).or_else(|| p.find_export(set, None)).ok_or(format!("{set} not found"))?;
    Ok(upk::anim::read_anim_set(&p, i).ok_or(format!("could not decode {set}"))?)
}

/// TdPawn.UpdateAnimSets(Weapon) for the first-person mesh: AS_C1P_OneHanded_Common and the
/// gun's AnimationSetCharacter1p, searched before AS_C1P_Unarmed. Returns the sequences keyed
/// to the unarmed set's tracks (the gun's own set last, so its names win) and the sim library
/// with them over the unarmed ones.
pub fn load_armed_anims(install: &Path, base: &PlayerAnims, class: &tdsim::weapons::WeaponClass) -> Result<(Vec<upk::anim::AnimSeq>, AnimLib), Box<dyn std::error::Error>> {
    let c = cooked(install);
    let mut seqs = Vec::new();
    // CommonArmedLight1p / CommonArmedHeavy1p, then the gun's AnimationSetCharacter1p
    let common = if class.heavy { "AS_C1P_TwoHanded_Common" } else { "AS_C1P_OneHanded_Common" };
    let weapon_set = class.anim_set_1p;
    for (pkg, set) in [(common, common), (weapon_set, weapon_set)] {
        let s = load_set(&c, pkg, set)?;
        seqs.extend(remap_seqs(&s, &base.set.track_bone_names));
    }
    let armed = upk::anim::AnimSet { name: weapon_set.to_string(), anim_rotation_only: base.set.anim_rotation_only, track_bone_names: base.set.track_bone_names.clone(), seqs: seqs.clone() };
    let over = anim_lib(&armed, &base.upper);
    let mut all = (*base.lib.seqs).clone();
    for (k, v) in over.seqs.iter() {
        all.insert(k.clone(), v.clone());
    }
    Ok((seqs, AnimLib { seqs: Arc::new(all), mesh_rot: base.lib.mesh_rot }))
}

/// The armed sets' notify sound cues (the snatch moves' grunts and gun handling).
pub fn armed_notify_cues(seqs: &[upk::anim::AnimSeq]) -> Vec<String> {
    let set = upk::anim::AnimSet { name: String::new(), anim_rotation_only: false, track_bone_names: Vec::new(), seqs: seqs.to_vec() };
    crate::sounds::notify_cues(&set)
}

/// An enemy's animations and skeleton for a loadout: AnimationSets[0] with AnimationSets[1]
/// over it (re-keyed to the first set's tracks), on the loadout's body mesh.
pub fn load_npc_anims(install: &Path, l: &tdsim::weapons::Loadout) -> Result<BotAnims, Box<dyn std::error::Error>> {
    let c = cooked(install);
    let mut set = load_set(&c, l.anim_package, l.anim_set)?;
    if let Some(sub) = l.anim_subset {
        let s = load_set(&c, l.anim_package, sub)?;
        merge_set(&mut set, &s);
    }
    let mesh_pkg = Package::open(c.join("Characters").join(format!("{}.upk", l.body.package)))?;
    let mesh_idx = mesh_pkg.find_export(l.body.mesh, Some("SkeletalMesh")).ok_or(format!("{} not found", l.body.mesh))?;
    let (skel, _) = upk::skelmesh::read_skeleton(&mesh_pkg, mesh_idx)?;
    let lib = anim_lib(&set, &skel);
    Ok(BotAnims { lib, set, skel })
}

/// AT_C1P TdAnimNodeWeaponPoseOffset_0's Profiles (BuildPoseOffsets' cooked result): per
/// profile name, (SK_UpperBody bone name, translation, rotation as a decoded-key quaternion
/// x,y,z,w). The node takes each bone's atom to Q * atom rotation and atom translation - T
/// (checked: the common WeaponPose with the TwoHanded-Neostead offset is the Neostead's).
/// The cooked FMatrix planes are stored W,X,Y,Z.
pub fn load_weapon_pose_profiles(install: &Path) -> Result<HashMap<String, Vec<(String, [f32; 3], [f32; 4])>>, Box<dyn std::error::Error>> {
    use upk::{export_props, find, struct_array, Value};
    let c = cooked(install);
    let pkg = Package::open(c.join("Characters").join("AT_C1P.upk"))?;
    let mesh = Package::open(c.join("Characters").join("CH_TKY_Crim_Fixer_1P.upk"))?;
    let mi = mesh.find_export("SK_UpperBody", Some("SkeletalMesh")).ok_or("no SK_UpperBody")?;
    let (skel, _) = upk::skelmesh::read_skeleton(&mesh, mi)?;
    let i = pkg.find_export("TdAnimNodeWeaponPoseOffset_0", None).ok_or("no TdAnimNodeWeaponPoseOffset_0")?;
    let (props, _) = export_props(&pkg, i)?;
    let data = pkg.export_bytes(i);
    let mut out = HashMap::new();
    for el in struct_array(&pkg, data, find(&props, "Profiles").ok_or("no Profiles")?) {
        let Some(Value::Name(name)) = el.iter().find(|p| p.name == "Name").map(|p| p.value.clone()) else { continue };
        let Some(bones) = el.iter().find(|p| p.name == "BoneIndices").map(|p| p.as_i32_array(data)) else { continue };
        let Some(b) = el.iter().find(|p| p.name == "MatrixTransforms").map(|p| p.bytes(data)) else { continue };
        let f = |k: usize| f32::from_le_bytes(b[4 + k * 4..8 + k * 4].try_into().unwrap());
        let mut v = Vec::new();
        for (m, &bi) in bones.iter().enumerate() {
            if 4 + (m + 1) * 64 > b.len() {
                break;
            }
            let Some(bone) = skel.bones.get(bi as usize) else { continue };
            let row = |r: usize| glam::Vec3::new(f(m * 16 + r * 4 + 1), f(m * 16 + r * 4 + 2), f(m * 16 + r * 4 + 3));
            let q = glam::Quat::from_mat3(&glam::Mat3::from_cols(row(0), row(1), row(2))).normalize();
            v.push((bone.name.clone(), row(3).to_array(), q.to_array()));
        }
        out.insert(name, v);
    }
    Ok(out)
}

/// The first PhysicsAsset in a character package (the ragdoll). The cops all use the shared
/// Male3p_Physics, cooked into each package that needs it; CH_TKY_Cop_Support has no copy,
/// so it falls back to CH_TKY_Cop_Patrol's.
pub fn load_physics_asset(install: &Path, package: &str) -> Result<upk::physics::PhysicsAsset, Box<dyn std::error::Error>> {
    for p in [package, "CH_TKY_Cop_Patrol"] {
        let pkg = Package::open(cooked(install).join("Characters").join(format!("{p}.upk")))?;
        if let Some(i) = (0..pkg.exports.len()).find(|&i| pkg.export_class(i) == "PhysicsAsset") {
            return Ok(upk::physics::read_physics_asset(&pkg, i)?);
        }
    }
    Err("no PhysicsAsset".into())
}

/// A skeletal mesh's materials (by object name, as umodel names the glTF materials) and each
/// one's diffuse texture, decoded (Materials::resolve, as the level's meshes get theirs).
pub fn skel_mesh_textures(install: &Path, package_path: &Path, mesh: &str) -> Vec<(String, Option<crate::TextureData>)> {
    let cooked = cooked(install);
    let Ok(pkg) = Package::open(package_path) else { return Vec::new() };
    let Some(i) = pkg.find_export(mesh, Some("SkeletalMesh")) else { return Vec::new() };
    let Ok((m, _)) = upk::skelmesh::read_skeleton(&pkg, i) else { return Vec::new() };
    let mut files = upk::texture::FileIndex::new(&cooked);
    let mut mats = crate::materials::Materials::new(1024);
    let mut out = Vec::new();
    for &mat in &m.materials {
        let name = if mat == 0 { "None".to_string() } else { pkg.object_name(mat) };
        // a material imported from another package (SK_TKY_Cop_Patrol_PK's MI_TKY_Cop_Riot): resolve it there
        let path = pkg.object_path(mat);
        let imported = (mat < 0)
            .then(|| path.split_once('.'))
            .flatten()
            .and_then(|(p, _)| files.path_of(p).cloned())
            .and_then(|f| Package::open(f).ok())
            .and_then(|p| p.find_export(&name, None).map(|i| (p, i)));
        let idx = match imported {
            Some((p, i)) => mats.resolve(&p, &mut files, i as i32 + 1),
            None => mats.resolve(&pkg, &mut files, mat),
        };
        let tex = mats.infos[idx].diffuse;
        out.push((name, tex));
    }
    out.into_iter().map(|(n, t)| (n, t.and_then(|k| mats.textures.get(k).cloned()))).collect()
}

/// AITemplate_PatrolCop_Glock's AnimationSets[1] (AS_AI_PatrolCop_Onehanded_Glock18) over the
/// patrol cop's set.
pub fn add_bot_weapon_set(install: &Path, anims: &mut BotAnims, set: &str) -> Result<(), Box<dyn std::error::Error>> {
    let c = cooked(install);
    let s = load_set(&c, "AS_AI_PatrolCop_OneHanded", set)?;
    let seqs = remap_seqs(&s, &anims.set.track_bone_names);
    for q in seqs {
        if let Some(e) = anims.set.seqs.iter_mut().find(|e| e.name.eq_ignore_ascii_case(&q.name)) {
            *e = q;
        } else {
            anims.set.seqs.push(q);
        }
    }
    anims.lib = anim_lib(&anims.set, &anims.skel);
    Ok(())
}

/// The anims a gun mesh plays in Faith's hands: CommonArmedLight1p / Heavy1p with the gun's
/// AnimationSetCharacter1p over it (the weapon mesh's AnimSets; they carry the Wep_* tracks).
pub fn load_weapon_mesh_set(install: &Path, class: &tdsim::weapons::WeaponClass) -> Result<upk::anim::AnimSet, Box<dyn std::error::Error>> {
    let c = cooked(install);
    let common = if class.heavy { "AS_C1P_TwoHanded_Common" } else { "AS_C1P_OneHanded_Common" };
    let mut set = load_set(&c, common, common)?;
    let s = load_set(&c, class.anim_set_1p, class.anim_set_1p)?;
    merge_set(&mut set, &s);
    Ok(set)
}

/// A weapon mesh's skeleton and its "Muzzleflash" socket (bone, Unreal relative location).
pub fn load_weapon_mesh(install: &Path, class: &tdsim::weapons::WeaponClass) -> Result<(upk::skelmesh::SkelMesh, Option<(String, [f32; 3])>), Box<dyn std::error::Error>> {
    use upk::{export_props, find, Value};
    let pkg = Package::open(cooked(install).join("Weapons").join(format!("{}.upk", class.package)))?;
    let i = pkg.find_export(class.mesh, Some("SkeletalMesh")).ok_or(format!("{} not found", class.mesh))?;
    let (skel, _) = upk::skelmesh::read_skeleton(&pkg, i)?;
    let mut socket = None;
    for e in 0..pkg.exports.len() {
        if pkg.export_class(e) != "SkeletalMeshSocket" {
            continue;
        }
        let Ok((props, _)) = export_props(&pkg, e) else { continue };
        let data = pkg.export_bytes(e);
        let name = |n: &str| match find(&props, n).map(|p| &p.value) {
            Some(Value::Name(s)) => s.clone(),
            _ => String::new(),
        };
        if name("SocketName").eq_ignore_ascii_case("Muzzleflash") {
            let loc = find(&props, "RelativeLocation").and_then(|p| p.as_vec3(data)).unwrap_or([0.0; 3]);
            socket = Some((name("BoneName"), loc));
            break;
        }
    }
    Ok((skel, socket))
}

/// load_armed_anims for every gun, reading each CommonArmed set once.
/// TdPawn.UpdateAnimSets for Mesh3p: per gun, AS_F3P_<grip>_Common (CommonArmedLight3p /
/// Heavy3p) then its AnimationSetFemale3p, keyed to the third-person body's tracks, and how
/// many came from the common set.
pub fn load_all_armed_anims_3p(install: &Path, base3p: &upk::anim::AnimSet) -> Vec<(&'static tdsim::weapons::WeaponClass, Vec<upk::anim::AnimSeq>, usize)> {
    let c = cooked(install);
    let mut out = Vec::new();
    for class in tdsim::weapons::WEAPONS.iter() {
        let common = if class.heavy { "AS_F3P_TwoHanded_Common" } else { "AS_F3P_OneHanded_Common" };
        let mut seqs = load_set(&c, common, common).map(|s| remap_seqs(&s, &base3p.track_bone_names)).unwrap_or_default();
        let n = seqs.len();
        let own = class.anim_set_1p.replacen("AS_C1P_", "AS_F3P_", 1);
        if let Ok(s) = load_set(&c, &own, &own) {
            seqs.extend(remap_seqs(&s, &base3p.track_bone_names));
        }
        out.push((*class, seqs, n));
    }
    out
}

/// Each gun: its armed sequences (the CommonArmed set's first, then the gun's own, which win
/// by name), how many of them came from the common set, and the sim library.
pub fn load_all_armed_anims(install: &Path, base: &PlayerAnims) -> Vec<(&'static tdsim::weapons::WeaponClass, Vec<upk::anim::AnimSeq>, AnimLib, usize)> {
    let c = cooked(install);
    let mut commons: HashMap<&str, Vec<upk::anim::AnimSeq>> = HashMap::new();
    let mut out = Vec::new();
    for class in tdsim::weapons::WEAPONS.iter() {
        let common = if class.heavy { "AS_C1P_TwoHanded_Common" } else { "AS_C1P_OneHanded_Common" };
        if !commons.contains_key(common) {
            match load_set(&c, common, common) {
                Ok(s) => {
                    commons.insert(common, remap_seqs(&s, &base.set.track_bone_names));
                }
                Err(e) => eprintln!("no {common} ({e})"),
            }
        }
        let mut seqs = commons.get(common).cloned().unwrap_or_default();
        let n_common = seqs.len();
        match load_set(&c, class.anim_set_1p, class.anim_set_1p) {
            Ok(s) => seqs.extend(remap_seqs(&s, &base.set.track_bone_names)),
            Err(e) => eprintln!("no {} ({e})", class.anim_set_1p),
        }
        let armed = upk::anim::AnimSet { name: class.anim_set_1p.to_string(), anim_rotation_only: base.set.anim_rotation_only, track_bone_names: base.set.track_bone_names.clone(), seqs: seqs.clone() };
        let over = anim_lib(&armed, &base.upper);
        let mut all = (*base.lib.seqs).clone();
        for (k, v) in over.seqs.iter() {
            all.insert(k.clone(), v.clone());
        }
        out.push((*class, seqs, AnimLib { seqs: Arc::new(all), mesh_rot: base.lib.mesh_rot }, n_common));
    }
    out
}
