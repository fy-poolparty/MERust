//! The enemies' bodies, one per sim bot: its loadout's AITemplate mesh (and head) exported once
//! with umodel, textured from the mesh's own materials, posed every frame from the bot
//! (me_level::pose::update_bot) and ragdolled with the package's PhysicsAsset on death.

use bevy::gltf::{GltfAssetLabel, GltfMaterialName};
use bevy::prelude::*;
use me_level::pose::{bot_mesh_to_world, PoseEvaluator};
use me_level::ragdoll::{to_ue, Ragdoll, Xform};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tdsim::weapons::{Loadout, LOADOUTS};

/// One enemy's mesh root (index into the sim's bots).
#[derive(Component)]
pub struct CopRoot(pub usize);

/// What every enemy of one loadout shares: animations and skeleton, ragdoll, mesh textures.
pub struct Kind {
    pub anims: me_level::anims::BotAnims,
    physics: Option<upk::physics::PhysicsAsset>,
    rot_origin: tdsim::Rotator,
    origin: [f32; 3],
    /// Material name -> its diffuse texture (decoded from the package).
    textures: Vec<(String, Option<me_level::TextureData>)>,
    images: HashMap<String, Handle<Image>>,
}

struct Cop {
    loadout: usize,
    pose: PoseEvaluator,
    root: Entity,
    /// Per bone: the joint entities of every mesh (body, head).
    joints: Vec<Vec<Entity>>,
    meshes: usize,
    mapped: bool,
    textured: bool,
    /// The death ragdoll, and the mesh-to-world it started from (the root stays there).
    ragdoll: Option<Ragdoll>,
    frozen: Option<Mat4>,
}

#[derive(Resource)]
pub struct Cops {
    install: PathBuf,
    kinds: HashMap<usize, Kind>,
    cops: Vec<Cop>,
}

fn umodel(install: &Path, package: &str, mesh: &str) -> bool {
    let out = crate::body::cache_dir().join("npcs");
    if out.join(package).join("SkeletalMesh3").join(format!("{mesh}.gltf")).exists() {
        return true;
    }
    let cooked = install.join("TdGame").join("CookedPC");
    let status = std::process::Command::new(crate::body::umodel_exe())
        .arg("-export")
        .arg("-gltf")
        .arg(format!("-path={}", cooked.display()))
        .arg(format!("-out={}", out.display()))
        .arg(cooked.join("Characters").join(format!("{package}.upk")))
        .arg(mesh)
        .arg("SkeletalMesh")
        .stdout(std::process::Stdio::null())
        .status();
    if !matches!(status, Ok(s) if s.success()) {
        eprintln!("umodel could not export {package}.{mesh}: {status:?}");
        return false;
    }
    true
}

/// Export every enemy body and head with umodel unless already cached.
pub fn ensure_export(install: &Path) -> bool {
    let mut ok = true;
    for l in LOADOUTS.iter() {
        ok &= umodel(install, l.body.package, l.body.mesh);
        if let Some(h) = l.body.head {
            ok &= umodel(install, l.body.package, h);
        }
    }
    ok
}

/// The index of a bot's loadout in LOADOUTS.
pub fn loadout_index(l: &Loadout) -> usize {
    LOADOUTS.iter().position(|x| std::ptr::eq(x, l)).unwrap_or(0)
}

impl Cops {
    pub fn new(install: &Path) -> Self {
        Cops { install: install.to_path_buf(), kinds: HashMap::new(), cops: Vec::new() }
    }

    /// A loadout's shared data, loaded on first use.
    pub fn kind(&mut self, idx: usize) -> Option<&Kind> {
        if !self.kinds.contains_key(&idx) {
            let l = &LOADOUTS[idx];
            let anims = match me_level::anims::load_npc_anims(&self.install, l) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("no {} ({e})", l.label);
                    return None;
                }
            };
            let physics = me_level::anims::load_physics_asset(&self.install, l.body.package).map_err(|e| eprintln!("no ragdoll for {} ({e})", l.label)).ok();
            let pkg = self.install.join("TdGame").join("CookedPC").join("Characters").join(format!("{}.upk", l.body.package));
            let mut textures = me_level::anims::skel_mesh_textures(&self.install, &pkg, l.body.mesh);
            if let Some(h) = l.body.head {
                textures.extend(me_level::anims::skel_mesh_textures(&self.install, &pkg, h));
            }
            let r = anims.skel.rot_origin;
            let origin = anims.skel.origin;
            self.kinds.insert(idx, Kind { anims, physics, rot_origin: tdsim::Rotator::new(r[0], r[1], r[2]), origin, textures, images: HashMap::new() });
        }
        self.kinds.get(&idx)
    }

    /// The sim's animation library for a loadout (for spawning its bot).
    pub fn lib(&mut self, idx: usize) -> Option<tdsim::anim::AnimLib> {
        self.kind(idx).map(|k| k.anims.lib.clone())
    }

    /// An enemy's joint entity (the body mesh's) for a bone.
    pub fn joint(&self, cop: usize, bone: &str) -> Option<Entity> {
        let c = self.cops.get(cop)?;
        let i = c.pose.bone_index(bone)?;
        c.joints.get(i)?.first().copied()
    }
}

/// One body per sim bot: new bots get theirs, removed ones lose theirs.
pub fn sync_cops(mut commands: Commands, assets: Res<AssetServer>, game: Res<crate::Game>, mut cops: ResMut<Cops>) {
    let bots = &game.sim.bots;
    // drop bodies past the bots, or whose bot changed loadout
    while cops.cops.len() > bots.len() {
        let c = cops.cops.pop().unwrap();
        commands.entity(c.root).despawn();
    }
    for i in 0..cops.cops.len() {
        if cops.cops[i].loadout != loadout_index(bots[i].loadout) {
            let c = cops.cops.drain(i..).collect::<Vec<_>>();
            for c in c {
                commands.entity(c.root).despawn();
            }
            break;
        }
    }
    for i in cops.cops.len()..bots.len() {
        let idx = loadout_index(bots[i].loadout);
        let Some(k) = cops.kind(idx) else { return };
        let pose = PoseEvaluator::new(k.anims.set.clone(), &k.anims.skel);
        let l = &LOADOUTS[idx];
        let meshes: Vec<&str> = std::iter::once(l.body.mesh).chain(l.body.head).collect();
        let n = meshes.len();
        let root = commands
            .spawn((CopRoot(i), Transform::default(), Visibility::default()))
            .with_children(|p| {
                for mesh in meshes {
                    let path = format!("npcs/{}/SkeletalMesh3/{mesh}.gltf", l.body.package);
                    p.spawn(WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))));
                }
            })
            .id();
        cops.cops.push(Cop { loadout: idx, pose, root, joints: Vec::new(), meshes: n, mapped: false, textured: false, ragdoll: None, frozen: None });
    }
}

/// Find each body's joint entities by bone name once its scenes are in the world, and give the
/// meshes their materials' diffuse textures.
pub fn setup_cops(
    mut cops: ResMut<Cops>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut mats: Query<(&GltfMaterialName, &mut MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    let cops = &mut *cops;
    for c in cops.cops.iter_mut() {
        let root = c.root;
        if !c.mapped {
            let mut joints = vec![Vec::new(); c.pose.bone_names.len()];
            for e in children.iter_descendants(root) {
                if let Ok(n) = names.get(e) {
                    if let Some(i) = c.pose.bone_index(n.as_str()) {
                        joints[i].push(e);
                    }
                }
            }
            if joints.iter().all(|j| j.len() >= c.meshes) {
                c.joints = joints;
                c.mapped = true;
            }
        }
        if c.mapped && !c.textured {
            let Some(k) = cops.kinds.get_mut(&c.loadout) else { continue };
            let mut done = 0;
            for e in children.iter_descendants(root) {
                let Ok((name, mut mat)) = mats.get_mut(e) else { continue };
                // skinned bounds come from the bind pose
                commands.entity(e).insert(bevy::camera::visibility::NoFrustumCulling);
                if !k.images.contains_key(&name.0) {
                    let img = k.textures.iter().find(|(n, _)| n.eq_ignore_ascii_case(&name.0)).and_then(|(_, t)| t.as_ref()).and_then(crate::map::to_image);
                    if let Some(img) = img {
                        let h = images.add(img);
                        k.images.insert(name.0.clone(), h);
                    }
                }
                mat.0 = match k.images.get(&name.0) {
                    Some(t) => materials.add(StandardMaterial { base_color_texture: Some(t.clone()), perceptual_roughness: 0.8, ..default() }),
                    None => materials.add(StandardMaterial { base_color: Color::srgb(0.12, 0.11, 0.1), perceptual_roughness: 0.4, ..default() }),
                };
                done += 1;
            }
            c.textured = done > 0;
        }
    }
}

/// Pose every body from its bot.
pub fn pose_cops(
    game: Res<crate::Game>,
    mut cops: ResMut<Cops>,
    time: Res<Time>,
    mut roots: Query<(&CopRoot, &mut Transform), Without<Name>>,
    mut joints: Query<&mut Transform, With<Name>>,
) {
    let dt = time.delta_secs().clamp(1e-4, 0.05);
    let cops = &mut *cops;
    for (r, mut t) in &mut roots {
        let Some(bot) = game.sim.bots.get(r.0) else { continue };
        let Some(c) = cops.cops.get_mut(r.0) else { continue };
        let Some(k) = cops.kinds.get(&c.loadout) else { continue };
        if bot.movement_state != tdsim::bots::BotMove::Dying {
            c.ragdoll = None;
            c.frozen = None;
        }
        let m = bot_mesh_to_world(bot, k.rot_origin, k.origin);
        c.pose.update_bot(bot, dt, k.rot_origin);
        // the death ragdoll: start at the moment of death from the animated pose
        if let (Some(d), Some(pa), None) = (bot.death.filter(|_| bot.movement_state == tdsim::bots::BotMove::Dying), k.physics.as_ref(), c.ragdoll.as_ref()) {
            let anim: Vec<Xform> = c.pose.globals().into_iter().map(|g| to_ue(m * g)).collect();
            let vel = vec![Vec3::ZERO; anim.len()];
            let (hl, hm) = bot.death_hit;
            let pose = &c.pose;
            c.ragdoll = Some(Ragdoll::new(pa, |n| pose.bone_index(n), &anim, &vel, d, (Vec3::new(hl.x, hl.y, hl.z), Vec3::new(hm.x, hm.y, hm.z)), game.sim.pawn.world_gravity_z));
            c.frozen = Some(m);
        }
        let m0 = c.frozen.unwrap_or(m);
        *t = Transform::from_matrix(m0);
        if let Some(rd) = c.ragdoll.as_mut() {
            rd.drive(&mut c.pose, m, m0, dt, &game.sim.world, Vec3::new(bot.velocity.x, bot.velocity.y, bot.velocity.z));
        }
        if !c.mapped {
            continue;
        }
        for (i, p) in c.pose.pose.iter().enumerate() {
            for &e in &c.joints[i] {
                if let Ok(mut jt) = joints.get_mut(e) {
                    jt.translation = p.pos;
                    jt.rotation = p.rot;
                }
            }
        }
    }
}
