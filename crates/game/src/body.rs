//! Faith's first-person arms and legs: SK_UpperBody / SK_LowerBody exported once with umodel
//! to glTF (cache/faith1p), posed every frame by `me_level::pose` from the sim's animation
//! state, with the camera at the mesh's EyeJoint like TdPlayerPawn.CalcCamera.

use bevy::gltf::{GltfAssetLabel, GltfMaterialName};
use bevy::prelude::*;
use me_level::pose::{mesh_to_world, PoseEvaluator};
use std::path::{Path, PathBuf};

const PKG_1P: &str = "CH_TKY_Crim_Fixer_1P";
const PKG_3P: &str = "CH_TKY_Crim_Fixer";
const PKG_CINE: &str = "CH_Faith_Cinematic";

/// What the body systems need beyond the sim.
#[derive(Resource)]
pub struct Body {
    pub pose: PoseEvaluator,
    pub rot_origin: tdsim::Rotator,
    pub origin: [f32; 3],
    /// Bone index -> joint entities (one per mesh), filled once the scenes have spawned.
    joints: Vec<Vec<Entity>>,
    mapped: bool,
    materials_done: bool,
    eye: Option<usize>,
    pub swan: me_level::camera::SwanNeck,
    /// Mesh3p: the full third-person Faith, invisible but casting the shadow (head included).
    pub shadow: Option<ShadowBody>,
    /// Mesh1p (arms) and Mesh1pLowerBody mesh entities, and the depth groups they are in now.
    upper_meshes: Vec<Entity>,
    lower_meshes: Vec<Entity>,
    dpg: Option<(tdsim::pawn::Dpg, tdsim::pawn::Dpg)>,
}

pub struct ShadowBody {
    pub pose: PoseEvaluator,
    joints: Vec<Option<Entity>>,
    mapped: bool,
}

#[derive(Component)]
pub struct BodyRoot;

#[derive(Component)]
pub struct ShadowRoot;

/// Layer no camera renders but the sun does: Mesh3p only shows up as a shadow.
pub const SHADOW_LAYER: usize = 2;

/// SDPG_Foreground: Mesh1p (the arms) draws in a second pass over the world, so it never
/// clips into walls or ledges. Mesh1pLowerBody stays in the world pass.
pub const FOREGROUND_LAYER: usize = 1;

impl Body {
    /// Mesh1p's joint entity for a bone.
    pub fn joint_entity(&self, bone: &str) -> Option<Entity> {
        let i = self.pose.bone_index(bone)?;
        self.joints.get(i)?.first().copied()
    }

    /// One of Mesh1p's mesh entities (its depth group's layer).
    pub fn upper_mesh(&self) -> Option<Entity> {
        self.upper_meshes.first().copied()
    }
}

pub fn umodel_exe() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/umodel/umodel_64.exe")
}

pub fn cache_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cache")
}

fn mesh_gltf(name: &str) -> String {
    format!("faith1p/{PKG_1P}/SkeletalMesh3/{name}.gltf")
}

/// Export the 1p meshes and their diffuse textures with umodel unless already cached.
pub fn ensure_export(install: &Path) -> bool {
    let cache = cache_dir();
    let out = cache.join("faith1p");
    let cooked = install.join("TdGame").join("CookedPC");
    let jobs: [(&str, &str, &str, &[&str]); 7] = [
        (PKG_3P, "SK_TKY_Crim_Fixer", "SkeletalMesh", &["-gltf"]),
        (PKG_1P, "SK_UpperBody", "SkeletalMesh", &["-gltf"]),
        (PKG_1P, "SK_LowerBody", "SkeletalMesh", &["-gltf"]),
        (PKG_1P, "Female_1p_C", "Texture2D", &["-png"]),
        (PKG_1P, "Faith_Glove_C", "Texture2D", &["-png"]),
        (PKG_3P, "Asia_Fixer_Upper_C_shaded", "Texture2D", &["-png"]),
        (PKG_CINE, "Faith_Cine_Lower_C", "Texture2D", &["-png"]),
    ];
    for (pkg, obj, class, fmt) in jobs {
        let dir = if class == "SkeletalMesh" { "SkeletalMesh3" } else { "Texture2D" };
        let ext = if class == "SkeletalMesh" { "gltf" } else { "png" };
        if out.join(pkg).join(dir).join(format!("{obj}.{ext}")).exists() {
            continue;
        }
        let status = std::process::Command::new(umodel_exe())
            .arg("-export")
            .args(fmt)
            .arg(format!("-path={}", cooked.display()))
            .arg(format!("-out={}", out.display()))
            .arg(cooked.join("Characters").join(format!("{pkg}.upk")))
            .arg(obj)
            .arg(class)
            .stdout(std::process::Stdio::null())
            .status();
        if !matches!(status, Ok(s) if s.success()) {
            eprintln!("umodel could not export {pkg}.{obj}: {status:?}");
            return false;
        }
    }
    true
}

impl Body {
    pub fn new(pose: PoseEvaluator, rot_origin: tdsim::Rotator, origin: [f32; 3]) -> Self {
        let eye = pose.bone_index("EyeJoint");
        Body { pose, rot_origin, origin, joints: Vec::new(), mapped: false, materials_done: false, eye, swan: Default::default(), shadow: None, upper_meshes: Vec::new(), lower_meshes: Vec::new(), dpg: None }
    }

    /// A joint of Mesh3p (the shadow body) by bone name.
    pub fn shadow_joint(&self, bone: &str) -> Option<Entity> {
        let sh = self.shadow.as_ref()?;
        let i = sh.pose.bone_index(bone)?;
        sh.joints.get(i).copied().flatten()
    }

    pub fn with_shadow(mut self, mut pose: PoseEvaluator) -> Self {
        pose.drives_phase = false;
        self.shadow = Some(ShadowBody { pose, joints: Vec::new(), mapped: false });
        self
    }

    /// EyeJoint in Bevy world space for the current pose, plus the swan neck offset.
    pub fn eye_location(&self, sim: &tdsim::Sim) -> Option<Vec3> {
        let i = self.eye?;
        let m = mesh_to_world(sim, self.rot_origin, self.origin) * self.pose.globals()[i];
        // CalcCamera takes the swan neck's frame from the view after the camera animation
        let swan = self.swan.offset(self.view_rotation(sim));
        Some(m.w_axis.truncate() + crate::ue_to_bevy(swan))
    }

    /// View rotation with the CameraJoint's camera animation (rolls, landings).
    pub fn view_rotation(&self, sim: &tdsim::Sim) -> tdsim::Rotator {
        me_level::camera::camera_rotation(sim.pc.rotation, self.pose.camera_animation())
    }
}

pub fn spawn_body(mut commands: Commands, assets: Res<AssetServer>) {
    commands
        .spawn((BodyRoot, Transform::default(), Visibility::default()))
        .with_children(|p| {
            for name in ["SK_UpperBody", "SK_LowerBody"] {
                p.spawn(WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(mesh_gltf(name)))));
            }
        });
    let shadow = format!("faith1p/{PKG_3P}/SkeletalMesh3/SK_TKY_Crim_Fixer.gltf");
    commands.spawn((ShadowRoot, Transform::default(), Visibility::default())).with_children(|p| {
        p.spawn(WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(shadow))));
    });
}

/// Mesh3p: map its joints, put its meshes on the shadow-only layer.
pub fn setup_shadow_body(
    mut body: ResMut<Body>,
    root: Single<Entity, With<ShadowRoot>>,
    children: Query<&Children>,
    names: Query<&Name>,
    meshes: Query<(), With<Mesh3d>>,
    mut commands: Commands,
) {
    let Some(sh) = body.shadow.as_mut() else { return };
    if sh.mapped {
        return;
    }
    let mut joints = vec![None; sh.pose.bone_names.len()];
    let mut mesh_entities = Vec::new();
    for e in children.iter_descendants(*root) {
        if let Ok(n) = names.get(e) {
            if let Some(i) = sh.pose.bone_index(n.as_str()) {
                joints[i] = Some(e);
            }
        }
        if meshes.contains(e) {
            mesh_entities.push(e);
        }
    }
    if mesh_entities.is_empty() || joints.iter().filter(|j| j.is_some()).count() < 60 {
        return;
    }
    for e in mesh_entities {
        commands
            .entity(e)
            .insert((bevy::camera::visibility::RenderLayers::layer(SHADOW_LAYER), bevy::camera::visibility::NoFrustumCulling));
    }
    sh.joints = joints;
    sh.mapped = true;
}

/// Find the joint entities by bone name once both scenes are in the world.
pub fn map_joints(
    mut body: ResMut<Body>,
    root: Single<Entity, With<BodyRoot>>,
    children: Query<&Children>,
    names: Query<&Name>,
) {
    if body.mapped {
        return;
    }
    let mut joints = vec![Vec::new(); body.pose.bone_names.len()];
    for e in children.iter_descendants(*root) {
        if let Ok(n) = names.get(e) {
            if let Some(i) = body.pose.bone_index(n.as_str()) {
                joints[i].push(e);
            }
        }
    }
    if std::env::var("BODY_DEBUG").is_ok() {
        let n = children.iter_descendants(*root).count();
        let found = joints.iter().filter(|j| !j.is_empty()).count();
        eprintln!("map_joints: {n} descendants, {found} bones found, min per bone {:?}", joints.iter().map(|j| j.len()).min());
    }
    // both meshes carry the full skeleton
    if joints.iter().all(|j| j.len() >= 2) {
        body.joints = joints;
        body.mapped = true;
    }
}

/// Swap umodel's untextured materials for the diffuse textures the material instances use.
pub fn texture_body(
    mut body: ResMut<Body>,
    assets: Res<AssetServer>,
    root: Single<Entity, With<BodyRoot>>,
    children: Query<&Children>,
    mut q: Query<(&GltfMaterialName, &mut MeshMaterial3d<StandardMaterial>, &mut Visibility)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    if body.materials_done || !body.mapped {
        return;
    }
    let tex = |pkg: &str, n: &str| assets.load::<Image>(format!("faith1p/{pkg}/Texture2D/{n}.png"));
    let mut done = 0;
    for e in children.iter_descendants(*root) {
        let Ok((name, mut mat, _vis)) = q.get_mut(e) else { continue };
        // skinned bounds come from the bind pose, which is far from the posed arms; the shadow
        // comes from Mesh3p, not the 1p meshes
        commands.entity(e).insert((bevy::camera::visibility::NoFrustumCulling, bevy::light::NotShadowCaster));
        if matches!(name.0.as_str(), "MI_Female_Arm_Skinn" | "MI_Female_Arm_Glove") {
            body.upper_meshes.push(e);
        } else {
            body.lower_meshes.push(e);
        }
        let texture = match name.0.as_str() {
            "MI_Female_Arm_Skinn" => Some(tex(PKG_1P, "Female_1p_C")),
            "MI_Female_Arm_Glove" => Some(tex(PKG_1P, "Faith_Glove_C")),
            "MI_Faith_Lowres_Upper" => Some(tex(PKG_3P, "Asia_Fixer_Upper_C_shaded")),
            "MI_SHtest" => Some(tex(PKG_CINE, "Faith_Cine_Lower_C")),
            _ => None,
        };
        if let Some(t) = texture {
            let tint = if std::env::var("BODY_DEBUG").is_ok() { Color::srgb(1.0, 0.1, 0.1) } else { Color::WHITE };
            mat.0 = materials.add(StandardMaterial { base_color: tint, base_color_texture: Some(t), perceptual_roughness: 0.8, ..default() });
        }
        done += 1;
    }
    if done > 0 {
        body.materials_done = true;
    }
}

/// TdPlayerPawn.SetFirstPersonDPG / SetFirstPersonLowerBodyDPG: SDPG_Foreground draws over the
/// world (the foreground camera pass), SDPG_Intermediate is depth-tested against it (world pass),
/// so e.g. a ledge hides the fingertips while hanging.
pub fn apply_dpg(game: Res<crate::Game>, mut body: ResMut<Body>, mut commands: Commands) {
    let p = &game.sim.pawn;
    let want = (p.first_person_dpg, p.first_person_lower_body_dpg);
    if !body.materials_done || body.dpg == Some(want) {
        return;
    }
    let layer = |d: tdsim::pawn::Dpg| {
        bevy::camera::visibility::RenderLayers::layer(if d == tdsim::pawn::Dpg::Foreground { FOREGROUND_LAYER } else { 0 })
    };
    for &e in &body.upper_meshes {
        commands.entity(e).insert(layer(want.0));
    }
    for &e in &body.lower_meshes {
        commands.entity(e).insert(layer(want.1));
    }
    body.dpg = Some(want);
}

/// Pose the skeleton for this frame and place the meshes.
pub fn pose_body(
    mut game: ResMut<crate::Game>,
    mut body: ResMut<Body>,
    time: Res<Time>,
    mut root: Single<&mut Transform, (With<BodyRoot>, Without<ShadowRoot>)>,
    mut shadow_root: Single<&mut Transform, (With<ShadowRoot>, Without<BodyRoot>)>,
    mut joints: Query<&mut Transform, (Without<BodyRoot>, Without<ShadowRoot>)>,
) {
    let dt = time.delta_secs().clamp(1e-4, 0.05);
    let body = &mut *body;
    // TdSkelControlAim1p adds the swan neck for last frame's PlayerCameraRotation
    body.pose.swan_world = body.swan.offset(body.view_rotation(&game.sim));
    body.pose.update(&mut game.sim, dt);
    body.swan.update(&game.sim, dt);
    // the melee hit detection bone (Mesh.GetBoneLocation) from the posed mesh
    let ms = game.sim.pawn.movement_state;
    game.sim.hit_bone_world = game.sim.moves.melee.base(ms).and_then(|b| {
        let bone = b.hit_detection_bone;
        let i = body.pose.bone_index(bone)?;
        let w = mesh_to_world(&game.sim, body.rot_origin, body.origin) * body.pose.globals()[i];
        let t = w.w_axis;
        Some((bone.to_string(), tdsim::Vec3::new(t.x, t.z, t.y) * 100.0))
    });
    // TdPlayerPawn.CalcCamera -> Moves[MovementState].CheckForCameraCollision on the eye
    if let Some(eye) = body.eye_location(&game.sim) {
        let ue = tdsim::Vec3::new(eye.x, eye.z, eye.y) * 100.0;
        let rot = body.view_rotation(&game.sim);
        game.sim.check_for_camera_collision(ue, rot);
    }
    let m = mesh_to_world(&game.sim, body.rot_origin, body.origin);
    **root = Transform::from_matrix(m);
    **shadow_root = Transform::from_matrix(m);
    if let Some(sh) = body.shadow.as_mut() {
        sh.pose.update(&mut game.sim, dt);
        if sh.mapped {
            for (i, p) in sh.pose.pose.iter().enumerate() {
                if let Some(e) = sh.joints[i] {
                    if let Ok(mut t) = joints.get_mut(e) {
                        t.translation = p.pos;
                        t.rotation = p.rot;
                    }
                }
            }
        }
    }
    if !body.mapped {
        return;
    }
    for (i, p) in body.pose.pose.iter().enumerate() {
        for &e in &body.joints[i] {
            if let Ok(mut t) = joints.get_mut(e) {
                t.translation = p.pos;
                t.rotation = p.rot;
            }
        }
    }
}

/// BODY_DEBUG: where things ended up.
pub fn debug_body(
    game: Res<crate::Game>,
    body: Res<Body>,
    time: Res<Time>,
    root: Single<(&Transform, &GlobalTransform), (With<BodyRoot>, Without<ShadowRoot>)>,
    meshes: Query<(&GlobalTransform, &Visibility, Option<&bevy::mesh::skinning::SkinnedMesh>), With<Mesh3d>>,
) {
    if std::env::var("BODY_DEBUG").is_err() || (time.elapsed_secs() * 2.0) as u32 % 2 != 0 || time.delta_secs() == 0.0 {
        return;
    }
    let eye = body.eye_location(&game.sim);
    eprintln!("root local {:?} global {:?} eye {:?}", root.0.translation, root.1.translation(), eye);
    for (g, v, sk) in meshes.iter().filter(|m| m.2.is_some()).take(6) {
        eprintln!("  mesh at {:?} vis {:?} skinned {}", g.translation(), v, sk.is_some());
    }
}
