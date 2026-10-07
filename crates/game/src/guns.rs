//! Guns in the world: every TdWeapon class's mesh on WeaponSocket (bone RightWeapon) of the
//! player's arms and the enemies, lying around as pickups, and muzzle flashes.
//!
//! The gun mesh plays its owner's Custom_Weapon sequence (TdWeapon.PlayFiringAnimation plays
//! the same name on WeaponAnimationNode1p/3p): standfire moves Wep_Root (kick), the slide,
//! hammer and trigger; weaponposeempty locks the slide back. During a snatch the gun stays on
//! the disarmed enemy (his snatch anim carries it, TdWeapon.PlayCustomWeaponAnimation) until
//! Faith's move ends (TdMOVE_Disarm.StopMove -> AttachWeaponToHand).
//!
//! The muzzle flash is the handgun flash (PS_FX_MuzzleFlash1p_HandGun_FullAuto_01) reduced to
//! its sprites, at each mesh's Muzzleflash socket: the gradient blob and the barrel-aligned
//! flames (T_FX_MuzzleFlash_FP_Flame_01 2x2 sub-images), additive, 0.03 s, plus a short light.
//! Not ported: the per-weapon particle systems, tracers, shells.

use bevy::camera::visibility::RenderLayers;
use bevy::gltf::{GltfAssetLabel, GltfMaterialName};
use bevy::prelude::*;
use me_level::pose::PoseEvaluator;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tdsim::weapons::{WeaponClass, WEAPONS};

const FX_PKG: &str = "FX_TextureGeneric";
const FLAME: &str = "T_FX_MuzzleFlash_FP_Flame_01_D";
const BLOB: &str = "T_FX_MuzzleFlash_GradientBlob_01_D";

fn umodel(cooked: &Path, out: &Path, pkg: &Path, obj: &str, class: &str, fmt: &str) -> bool {
    let status = std::process::Command::new(crate::body::umodel_exe())
        .arg("-export")
        .arg(fmt)
        .arg(format!("-path={}", cooked.display()))
        .arg(format!("-out={}", out.display()))
        .arg(pkg)
        .arg(obj)
        .arg(class)
        .stdout(std::process::Stdio::null())
        .status();
    if !matches!(status, Ok(s) if s.success()) {
        eprintln!("umodel could not export {obj}: {status:?}");
        return false;
    }
    true
}

/// Export every gun mesh and the flash textures with umodel unless cached.
pub fn ensure_export(install: &Path) -> bool {
    let cache = crate::body::cache_dir();
    let cooked = install.join("TdGame").join("CookedPC");
    let mut ok = true;
    for c in WEAPONS.iter() {
        if cache.join("guns").join(c.package).join("SkeletalMesh3").join(format!("{}.gltf", c.mesh)).exists() {
            continue;
        }
        ok &= umodel(&cooked, &cache.join("guns"), &cooked.join("Weapons").join(format!("{}.upk", c.package)), c.mesh, "SkeletalMesh", "-gltf");
    }
    let fx = cooked.join("Effects").join(format!("{FX_PKG}.upk"));
    for t in [FLAME, BLOB] {
        if !cache.join("fx").join(FX_PKG).join("Texture2D").join(format!("{t}.png")).exists() {
            ok &= umodel(&cooked, &cache.join("fx"), &fx, t, "Texture2D", "-png");
        }
    }
    ok
}

/// One gun class's mesh data.
struct GunKind {
    /// WeaponSocket (RelativeRotation 16384, 16384, 0 on RightWeapon) times the mesh's
    /// RotOrigin / Origin, in glTF bone space; and the mesh's own RotOrigin / Origin.
    socket: Transform,
    mesh: Transform,
    skel: upk::skelmesh::SkelMesh,
    /// The Muzzleflash socket (bone, glTF bone-space offset).
    muzzle: Option<(String, Vec3)>,
    textures: Vec<(String, Option<me_level::TextureData>)>,
    images: HashMap<String, Handle<Image>>,
    /// The mesh posed with Faith's sets (CommonArmed + AnimationSetCharacter1p).
    player_pose: Option<PoseEvaluator>,
}

/// A gun entity and its class.
#[derive(Clone, Copy)]
struct Held {
    e: Entity,
    class: &'static WeaponClass,
}

#[derive(Resource)]
pub struct Guns {
    install: PathBuf,
    kinds: HashMap<&'static str, GunKind>,
    /// The enemies' gun poses: (loadout, class) -> the gun skeleton with the loadout's AI sets.
    bot_poses: HashMap<(usize, &'static str), PoseEvaluator>,
    player: Option<Held>,
    /// Mesh3p's weapon (TdWeapon.Mesh3p on the third-person body's WeaponSocket): only the
    /// sun sees it, like the body itself, so the gun shows up in Faith's shadow.
    shadow: Option<Held>,
    shadow_poses: HashMap<&'static str, PoseEvaluator>,
    bots: Vec<Option<Held>>,
    /// A disarmed enemy's gun left in the world (his ragdoll took over) until the snatch ends.
    frozen: Vec<bool>,
    pickups: Vec<Held>,
    textured: Vec<Entity>,
    joints: HashMap<Entity, Vec<Option<Entity>>>,
    flash: Option<FlashAssets>,
}

struct FlashAssets {
    quad: Handle<Mesh>,
    blob: Handle<StandardMaterial>,
    /// One material per flame sub-image (2x2).
    flames: Vec<Handle<StandardMaterial>>,
}

#[derive(Resource, Default)]
pub struct ShotQueue(pub Vec<tdsim::weapons::Shot>);

#[derive(Component)]
pub struct Flash(f32);

/// A flash sprite: faces the camera; a flame keeps its long side along the barrel.
#[derive(Component)]
pub struct FlashSprite {
    along: Option<Vec3>,
    size: Vec2,
}

#[derive(Component)]
pub struct GunModel;

fn ue_quat(r: tdsim::Rotator) -> Quat {
    let (x, y, z) = r.axes();
    let ue = Quat::from_mat3(&Mat3::from_cols(Vec3::new(x.x, x.y, x.z), Vec3::new(y.x, y.y, y.z), Vec3::new(z.x, z.y, z.z))).normalize();
    // Unreal -> glTF space (swap Y/Z): (x, z, y, -w)
    Quat::from_xyzw(ue.x, ue.z, ue.y, -ue.w).normalize()
}

impl Guns {
    pub fn new(install: &Path) -> Self {
        Guns {
            install: install.to_path_buf(),
            kinds: HashMap::new(),
            bot_poses: HashMap::new(),
            player: None,
            shadow: None,
            shadow_poses: HashMap::new(),
            bots: Vec::new(),
            frozen: Vec::new(),
            pickups: Vec::new(),
            textured: Vec::new(),
            joints: HashMap::new(),
            flash: None,
        }
    }

    /// A gun class's mesh data, loaded on first use.
    fn kind(&mut self, class: &'static WeaponClass) -> Option<&mut GunKind> {
        if !self.kinds.contains_key(class.name) {
            let (skel, socket) = match me_level::anims::load_weapon_mesh(&self.install, class) {
                Ok(x) => x,
                Err(e) => {
                    eprintln!("no {} mesh ({e})", class.name);
                    return None;
                }
            };
            let r = skel.rot_origin;
            let rot = ue_quat(tdsim::Rotator::new(r[0], r[1], r[2]));
            let mesh = Transform { translation: rot * me_level::pose::ue_pos(skel.origin), rotation: rot, scale: Vec3::ONE };
            let socket_t = Transform::from_rotation(ue_quat(tdsim::Rotator::new(16384, 16384, 0))) * mesh;
            let pkg = self.install.join("TdGame").join("CookedPC").join("Weapons").join(format!("{}.upk", class.package));
            let textures = me_level::anims::skel_mesh_textures(&self.install, &pkg, class.mesh);
            let player_pose = me_level::anims::load_weapon_mesh_set(&self.install, class).ok().map(|set| PoseEvaluator::new(set, &skel));
            let muzzle = socket.map(|(b, l)| (b, me_level::pose::ue_pos(l)));
            self.kinds.insert(class.name, GunKind { socket: socket_t, mesh, skel, muzzle, textures, images: HashMap::new(), player_pose });
        }
        self.kinds.get_mut(class.name)
    }
}

fn spawn_gun(commands: &mut Commands, assets: &AssetServer, class: &WeaponClass, t: Transform) -> Entity {
    let path = format!("guns/{}/SkeletalMesh3/{}.gltf", class.package, class.mesh);
    commands.spawn((GunModel, WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))), t, Visibility::default())).id()
}

fn forget(guns: &mut Guns, e: Entity) {
    guns.textured.retain(|x| *x != e);
    guns.joints.remove(&e);
}

/// Who holds which gun this frame: the player, the enemies, and the pickups.
#[allow(clippy::too_many_arguments)]
pub fn sync_guns(
    mut commands: Commands,
    assets: Res<AssetServer>,
    game: Res<crate::Game>,
    mut guns: ResMut<Guns>,
    body: Option<Res<crate::body::Body>>,
    cops: Option<Res<crate::cops::Cops>>,
    layers: Query<&RenderLayers>,
    globals: Query<&GlobalTransform>,
) {
    let sim = &game.sim;
    let snatch = if sim.pawn.movement_state == tdsim::Move::Snatch { sim.moves.disarm.target } else { None };
    // the player's gun on Mesh1p's WeaponSocket (once a snatch is over, and not put away)
    let want: Option<&'static WeaponClass> = sim.weapon.as_ref().map(|w| w.class).filter(|_| snatch.is_none() && sim.weapon_anim_state != tdsim::weapons::WeaponAnimState::Unarmed);
    let joint = body.as_ref().and_then(|b| b.joint_entity("RightWeapon"));
    if let Some(h) = guns.player {
        if want.is_none_or(|c| !std::ptr::eq(c, h.class)) {
            commands.entity(h.e).despawn();
            guns.player = None;
            forget(&mut guns, h.e);
        }
    }
    if let (Some(class), None, Some(j)) = (want, guns.player, joint) {
        if let Some(k) = guns.kind(class) {
            let socket = k.socket;
            let e = spawn_gun(&mut commands, &assets, class, socket);
            commands.entity(e).insert(ChildOf(j));
            guns.player = Some(Held { e, class });
        }
    }
    // Mesh3p's gun on the shadow body
    let joint3p = body.as_ref().and_then(|b| b.shadow_joint("RightWeapon"));
    if let Some(h) = guns.shadow {
        if want.is_none_or(|c| !std::ptr::eq(c, h.class)) {
            commands.entity(h.e).despawn();
            guns.shadow = None;
            forget(&mut guns, h.e);
        }
    }
    if let (Some(class), None, Some(j)) = (want, guns.shadow, joint3p) {
        if let Some(k) = guns.kind(class) {
            let socket = k.socket;
            let e = spawn_gun(&mut commands, &assets, class, socket);
            commands.entity(e).insert((ChildOf(j), RenderLayers::layer(crate::body::SHADOW_LAYER)));
            guns.shadow = Some(Held { e, class });
        }
    }
    // the arms' depth group (foreground pass) for the held gun
    if let (Some(h), Some(b)) = (guns.player, body.as_ref()) {
        if let Some(m) = b.upper_mesh() {
            let l = layers.get(m).cloned().unwrap_or_default();
            commands.entity(h.e).insert(l);
        }
    }
    // the enemies' guns
    let n = sim.bots.len();
    for i in n..guns.bots.len() {
        if let Some(h) = guns.bots[i] {
            commands.entity(h.e).despawn();
            forget(&mut guns, h.e);
        }
    }
    guns.bots.resize(n, None);
    guns.frozen.resize(n, false);
    for (i, b) in sim.bots.iter().enumerate() {
        let snatching = snatch == Some(i);
        let class = b.weapon.as_ref().map(|w| w.class).or_else(|| if snatching { guns.bots[i].map(|h| h.class) } else { None });
        let joint = cops.as_ref().and_then(|c| c.joint(i, "RightWeapon"));
        match (class, guns.bots[i], joint) {
            (Some(class), None, Some(j)) => {
                if let Some(k) = guns.kind(class) {
                    // the mesh's WeaponSocket: SK_TKY_Cop_Support has its own (RelativeLocation
                    // 2, 3.25, 4, RelativeRotation 16384, 0, -16384); the rest 16384, 16384, 0
                    let socket = if b.loadout.body.package == "CH_TKY_Cop_Support" {
                        Transform { translation: me_level::pose::ue_pos([2.0, 3.25, 4.0]), rotation: ue_quat(tdsim::Rotator::new(16384, 0, -16384)), scale: Vec3::ONE } * k.mesh
                    } else {
                        k.socket
                    };
                    let e = spawn_gun(&mut commands, &assets, class, socket);
                    commands.entity(e).insert(ChildOf(j));
                    guns.bots[i] = Some(Held { e, class });
                    guns.frozen[i] = false;
                }
            }
            (Some(_), Some(h), _) if b.weapon.is_none() && !guns.frozen[i] => {
                // his snatch anim ended before Faith's: leave the gun where his hand had it
                if let Ok(g) = globals.get(h.e) {
                    commands.entity(h.e).remove::<ChildOf>().insert(Transform::from(*g));
                }
                guns.frozen[i] = true;
            }
            (None, Some(h), _) => {
                commands.entity(h.e).despawn();
                guns.bots[i] = None;
                guns.frozen[i] = false;
                forget(&mut guns, h.e);
            }
            _ => {}
        }
    }
    // pickups (respawned on change: they're few)
    let same = guns.pickups.len() == sim.pickups.len() && guns.pickups.iter().zip(&sim.pickups).all(|(h, k)| std::ptr::eq(h.class, k.class));
    if !same {
        for h in std::mem::take(&mut guns.pickups) {
            commands.entity(h.e).despawn();
            forget(&mut guns, h.e);
        }
        for k in &sim.pickups {
            if let Some(g) = guns.kind(k.class) {
                let mesh = g.mesh;
                let e = spawn_gun(&mut commands, &assets, k.class, mesh);
                guns.pickups.push(Held { e, class: k.class });
            }
        }
    }
    for (k, h) in sim.pickups.iter().zip(guns.pickups.clone()) {
        let Some(g) = guns.kinds.get(k.class.name) else { continue };
        // lying on its side, along its yaw
        let yaw = ue_quat(tdsim::Rotator::new(0, k.rotation.yaw, 16384));
        let t = Transform { translation: crate::ue_to_bevy(k.location), rotation: yaw, scale: Vec3::ONE } * g.mesh;
        commands.entity(h.e).insert(t);
    }
}

/// The scenes' meshes get their materials' textures (and the arms' layer for the held one);
/// their joints are mapped for the guns' own animation.
#[allow(clippy::too_many_arguments)]
pub fn texture_guns(
    mut guns: ResMut<Guns>,
    assets: Res<AssetServer>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut mats: Query<(&GltfMaterialName, &mut MeshMaterial3d<StandardMaterial>)>,
    layers: Query<&RenderLayers>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    if guns.flash.is_none() {
        let tex = |n: &str| assets.load::<Image>(format!("fx/{FX_PKG}/Texture2D/{n}.png"));
        // (the emitters' colour-over-life curves aren't ported: a dim glow, brighter flames)
        let mat = |m: &mut Assets<StandardMaterial>, t: Handle<Image>, uv: bevy::math::Affine2, c: Color| {
            m.add(StandardMaterial { base_color: c, base_color_texture: Some(t), unlit: true, alpha_mode: AlphaMode::Add, cull_mode: None, uv_transform: uv, ..default() })
        };
        let blob = mat(&mut materials, tex(BLOB), bevy::math::Affine2::IDENTITY, Color::linear_rgb(0.3, 0.2, 0.1));
        let flame = tex(FLAME);
        let flames = (0..4)
            .map(|k| {
                let off = Vec2::new((k % 2) as f32 * 0.5, (k / 2) as f32 * 0.5);
                mat(&mut materials, flame.clone(), bevy::math::Affine2::from_scale_angle_translation(Vec2::splat(0.5), 0.0, off), Color::linear_rgb(1.0, 0.65, 0.3))
            })
            .collect();
        guns.flash = Some(FlashAssets { quad: meshes.add(Rectangle::new(1.0, 1.0)), blob, flames });
    }
    let all: Vec<Held> = guns.bots.iter().flatten().copied().chain(guns.player).chain(guns.shadow).chain(guns.pickups.iter().copied()).filter(|h| !guns.textured.contains(&h.e)).collect();
    for h in all {
        let is_player = guns.player.is_some_and(|p| p.e == h.e);
        let Some(k) = guns.kinds.get_mut(h.class.name) else { continue };
        let bone_names: Vec<String> = k.skel.bones.iter().map(|b| b.name.clone()).collect();
        let mut done = 0;
        let layer = layers.get(h.e).ok().cloned();
        let mut map = vec![None; bone_names.len()];
        for e in children.iter_descendants(h.e) {
            if let Some(l) = layer.clone() {
                commands.entity(e).insert(l);
            }
            if let Ok(n) = names.get(e) {
                if let Some(i) = bone_names.iter().position(|b| b.eq_ignore_ascii_case(n.as_str())) {
                    map[i] = Some(e);
                }
            }
            let Ok((name, mut mat)) = mats.get_mut(e) else { continue };
            commands.entity(e).insert(bevy::camera::visibility::NoFrustumCulling);
            // the first-person gun is like the 1p arms: no shadow (the shadow is the 3p body's)
            if is_player {
                commands.entity(e).insert(bevy::light::NotShadowCaster);
            }
            if !k.images.contains_key(&name.0) {
                if let Some(img) = k.textures.iter().find(|(n, _)| n.eq_ignore_ascii_case(&name.0)).and_then(|(_, t)| t.as_ref()).and_then(crate::map::to_image) {
                    let handle = images.add(img);
                    k.images.insert(name.0.clone(), handle);
                }
            }
            if let Some(t) = k.images.get(&name.0) {
                mat.0 = materials.add(StandardMaterial { base_color_texture: Some(t.clone()), perceptual_roughness: 0.5, metallic: 0.3, ..default() });
            }
            done += 1;
        }
        if done > 0 {
            guns.textured.push(h.e);
            guns.joints.insert(h.e, map);
        }
    }
    // keep the held gun's meshes on the arms' layer as the depth group changes
    if let Some(h) = guns.player {
        if let Ok(l) = layers.get(h.e) {
            let l = l.clone();
            for e in children.iter_descendants(h.e) {
                commands.entity(e).insert(l.clone());
            }
        }
    }
}

/// The guns' own bones from their owner's slots (Custom_Weapon; a disarmed enemy's snatch).
pub fn pose_guns(game: Res<crate::Game>, time: Res<Time>, mut guns: ResMut<Guns>, mut cops: Option<ResMut<crate::cops::Cops>>, mut joints: Query<&mut Transform, With<Name>>) {
    let dt = time.delta_secs().clamp(1e-4, 0.05);
    let guns = &mut *guns;
    let mut apply = |pose: &PoseEvaluator, root: Entity, map: &HashMap<Entity, Vec<Option<Entity>>>| {
        let Some(m) = map.get(&root) else { return };
        for (i, e) in m.iter().enumerate() {
            let (Some(e), Some(p)) = (e, pose.pose.get(i)) else { continue };
            if let Ok(mut t) = joints.get_mut(*e) {
                t.translation = p.pos;
                t.rotation = p.rot;
            }
        }
    };
    if let Some(h) = guns.player {
        if let Some(pe) = guns.kinds.get_mut(h.class.name).and_then(|k| k.player_pose.as_mut()) {
            pe.update_weapon_mesh(&game.sim.anim, tdsim::pawn::Slot::Weapon, Some(PoseEvaluator::gun_base_clip_1p(&game.sim)), dt);
            apply(pe, h.e, &guns.joints);
        }
    }
    if let Some(h) = guns.shadow {
        if !guns.shadow_poses.contains_key(h.class.name) {
            if let Some(skel) = guns.kinds.get(h.class.name).map(|k| k.skel.clone()) {
                let empty = upk::anim::AnimSet { name: String::new(), anim_rotation_only: false, track_bone_names: Vec::new(), seqs: Vec::new() };
                let set = me_level::anims::bot_weapon_pose_set(&guns.install, &empty, h.class);
                guns.shadow_poses.insert(h.class.name, PoseEvaluator::new(set, &skel));
            }
        }
        if let Some(pe) = guns.shadow_poses.get_mut(h.class.name) {
            pe.update_weapon_mesh(&game.sim.anim, tdsim::pawn::Slot::Weapon, Some("WeaponPose"), dt);
            apply(pe, h.e, &guns.joints);
        }
    }
    for i in 0..guns.bots.len() {
        let (Some(h), Some(b)) = (guns.bots[i], game.sim.bots.get(i)) else { continue };
        let li = crate::cops::loadout_index(b.loadout);
        let key = (li, h.class.name);
        if !guns.bot_poses.contains_key(&key) {
            let set = cops.as_mut().and_then(|c| c.kind(li)).map(|k| me_level::anims::bot_weapon_pose_set(&guns.install, &k.anims.set, h.class));
            let skel = guns.kinds.get(h.class.name).map(|k| k.skel.clone());
            if let (Some(set), Some(skel)) = (set, skel) {
                guns.bot_poses.insert(key, PoseEvaluator::new(set, &skel));
            }
        }
        let Some(pe) = guns.bot_poses.get_mut(&key) else { continue };
        let slot = if b.movement_state == tdsim::bots::BotMove::Disarmed { tdsim::pawn::Slot::Canned } else { tdsim::pawn::Slot::Weapon };
        // AT_Weapon_Default: WeaponPose under the Custom_Weapon slot
        let base = (slot == tdsim::pawn::Slot::Weapon).then_some("WeaponPose");
        pe.update_weapon_mesh(&b.anim, slot, base, dt);
        apply(pe, h.e, &guns.joints);
    }
}

/// Muzzle flashes at the shooter's gun: the flash sprites (additive, camera-facing) and a short
/// light.
#[allow(clippy::too_many_arguments)]
pub fn muzzle_flashes(
    mut commands: Commands,
    mut shots: ResMut<ShotQueue>,
    time: Res<Time>,
    guns: Res<Guns>,
    globals: Query<&GlobalTransform>,
    layers: Query<&RenderLayers>,
    mut flashes: Query<(Entity, &mut Flash)>,
) {
    for s in shots.0.drain(..) {
        let gun = match s.bot {
            None => guns.player,
            Some(i) => guns.bots.get(i).copied().flatten(),
        };
        let muzzle = gun.and_then(|h| {
            let k = guns.kinds.get(h.class.name)?;
            let (bone, off) = k.muzzle.as_ref()?;
            let bi = k.skel.bones.iter().position(|b| b.name.eq_ignore_ascii_case(bone))?;
            let j = guns.joints.get(&h.e)?.get(bi)?.as_ref()?;
            let gt = globals.get(*j).ok()?;
            // the barrel runs from the bone to the socket
            Some((gt.transform_point(*off), (gt.rotation() * off.normalize_or(Vec3::Y)).normalize_or_zero()))
        });
        let (at, dir) = muzzle.unwrap_or((crate::ue_to_bevy(s.start), (crate::ue_to_bevy(s.end) - crate::ue_to_bevy(s.start)).normalize_or_zero()));
        let layer = gun.and_then(|h| layers.get(h.e).ok().cloned()).unwrap_or_default();
        let mut e = commands.spawn((
            Flash(0.03),
            PointLight { color: Color::srgb(1.0, 0.8, 0.5), intensity: 3_000.0, range: 2.5, shadow_maps_enabled: false, ..default() },
            Transform::from_translation(at),
            Visibility::default(),
        ));
        if let Some(fx) = guns.flash.as_ref() {
            let r = time.elapsed_secs().to_bits() as usize;
            e.with_children(|p| {
                // the blob (scaled down: see the module note)
                p.spawn((FlashSprite { along: None, size: Vec2::splat(0.14) }, Mesh3d(fx.quad.clone()), MeshMaterial3d(fx.blob.clone()), Transform::default(), layer.clone(), bevy::camera::visibility::NoFrustumCulling));
                // the flames along the barrel (PSA_Velocity), a little ahead
                for k in 0..2 {
                    let m = fx.flames[(r + k) % 4].clone();
                    p.spawn((
                        FlashSprite { along: Some(dir), size: Vec2::new(0.07, 0.15) },
                        Mesh3d(fx.quad.clone()),
                        MeshMaterial3d(m),
                        Transform::from_translation(dir * (0.05 + 0.03 * k as f32)),
                        layer.clone(),
                        bevy::camera::visibility::NoFrustumCulling,
                    ));
                }
            });
        }
    }
    for (e, mut f) in &mut flashes {
        f.0 -= time.delta_secs();
        if f.0 <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

/// Flash sprites face the camera (the flames rotate about the barrel to do so).
pub fn face_flashes(cam: Query<&Transform, (With<crate::PlayerCamera>, Without<FlashSprite>)>, mut sprites: Query<(&mut Transform, &GlobalTransform, &FlashSprite)>) {
    let Some(eye) = cam.iter().next().map(|t| t.translation) else { return };
    for (mut t, g, s) in &mut sprites {
        let pos = g.translation();
        let to_cam = (eye - pos).normalize_or_zero();
        let rot = match s.along {
            None => Transform::from_translation(pos).looking_at(eye, Vec3::Y).rotation * Quat::from_rotation_y(std::f32::consts::PI),
            Some(d) => {
                // long side (local Y) along the barrel, the face as square to the camera as it gets
                let y = d;
                let z = (to_cam - y * to_cam.dot(y)).normalize_or(Vec3::Z);
                let x = y.cross(z);
                Quat::from_mat3(&Mat3::from_cols(x, y, z))
            }
        };
        // the parent (Flash) has no rotation: local = world here
        t.rotation = rot;
        t.scale = Vec3::new(s.size.x, s.size.y, 1.0);
    }
}
