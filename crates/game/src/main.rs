//! Mirror's Edge, standalone Rust port. Bevy renders; all movement is `tdsim` (Unreal units).
//! Unreal space (X forward, Y right, Z up, cm) maps to Bevy space (Y up, metres) by `ue_to_bevy`.
//!
//! Play: cargo run -p game --release            (startup menu: map and settings)
//!       cargo run -p game --release -- --map test       (the test course, no menu)
//!       cargo run -p game --release -- --map tutorial   (Tutorial_p from your install; WIP, not in the menu yet)

mod audio;
mod body;
mod cops;
mod guns;
mod course;
mod map;
mod menu;
mod settings;
mod style;

use bevy::{
    input::mouse::AccumulatedMouseMotion,
    light::CascadeShadowConfigBuilder,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PresentMode},
};
use std::sync::Mutex;
use tdsim::anim::AnimLib;
use tdsim::config::Config;

const UU_TO_M: f32 = 0.01;

pub fn ue_to_bevy(v: tdsim::Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, v.y) * UU_TO_M
}

struct Spawn {
    feet: tdsim::Vec3,
    yaw: i32,
    label: String,
}

#[derive(Resource)]
pub struct Game {
    sim: tdsim::Sim,
    spawns: Vec<Spawn>,
    current: usize,
    last_fall: f32,
    log: Vec<String>,
}

impl Game {
    fn respawn(&mut self) {
        let s = &self.spawns[self.current];
        self.sim.spawn(s.feet, s.yaw);
        // a checkpoint reload: no gun in hand, none lying around
        self.sim.weapon = None;
        self.sim.pickups.clear();
        self.sim.weapon_anim_state = tdsim::weapons::WeaponAnimState::Unarmed;
        self.sim.update_anim_sets_pub();
        // the enemies come back too, as a checkpoint reload brings them back
        for b in self.sim.bots.iter_mut() {
            b.reset();
        }
        self.last_fall = 0.0;
        self.log.push(format!("spawned at {}", s.label));
    }
}

#[derive(Resource)]
struct PendingLevel(Mutex<Option<map::RenderLevel>>);

/// Game meshes placed in the test course (ladder, landing bags), drawn next to its blocks.
#[derive(Resource)]
struct PendingProps(Mutex<Option<map::RenderLevel>>);

#[derive(Resource)]
struct Mouse {
    /// Mouse counts per pixel of motion, scaled before MouseSensitivity (18 uu/count in ME).
    scale: f32,
}

#[derive(Component)]
pub struct PlayerCamera;

#[derive(Component)]
struct ForegroundCamera;

/// `--shot <png>`: save a screenshot after `frames` frames and quit (for checking the view).
#[derive(Resource)]
struct AutoShot {
    path: String,
    frames: u32,
    pitch: Option<i32>,
    run: bool,
    /// `--auto-strafe S`: hold A/D (-1 / +1)
    strafe: f32,
    /// jump once when the pawn passes this X, then let go of forward
    jump_at: Option<f32>,
    jumped: bool,
    /// view yaw relative to the pawn once hanging (turn-around check)
    hang_yaw: Option<i32>,
    /// `--shot-every K`: after the jump, also save `<path>_NN.png` every K frames
    every: Option<u32>,
    since_jump: u32,
    skipped: u32,
    taken: u32,
}

#[derive(Component)]
struct Hud;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned());
    let install = me_level::find_install(arg("--me-dir").as_deref());
    let cfg = install.as_deref().map(Config::load).unwrap_or_default();
    if install.is_none() {
        eprintln!("Mirror's Edge install not found: using built-in defaults instead of your ini files");
    }

    // no map on the command line (and not a scripted shot): the startup menu picks one
    if arg("--map").is_none() && !args.iter().any(|a| a == "--tutorial" || a == "--shot") {
        menu::run(args[1..].to_vec(), install.clone());
        return;
    }
    let settings = settings::Settings::load();
    settings::set_volume(settings.volume);
    let tutorial = args.iter().any(|a| a == "--tutorial") || arg("--map").as_deref() == Some("tutorial");
    let (world, spawns, render) = if tutorial {
        let dir = install.clone().expect("--tutorial needs your Mirror's Edge install (--me-dir)");
        let level = me_level::load_map(&dir, me_level::TUTORIAL, 1024).expect("load Tutorial_p");
        let spawns = level
            .starts
            .iter()
            .map(|s| Spawn {
                feet: s.location - tdsim::Vec3::new(0.0, 0.0, 90.0),
                yaw: (s.yaw * 32768.0 / std::f32::consts::PI) as i32,
                label: format!("{} {}", s.name, s.challenges.join(",")),
            })
            .collect();
        let render = map::RenderLevel { batches: level.batches, materials: level.materials, textures: level.textures };
        (level.collision, spawns, Some(render))
    } else {
        let spawns = tdsim::testmap::spawns()
            .into_iter()
            .map(|s| Spawn { feet: s.feet, yaw: s.yaw, label: s.name.to_string() })
            .collect();
        (tdsim::testmap::world(), spawns, None)
    };
    // the course's game props (ladder, landing bags) with their collision
    let mut props_render = None;
    let world = match install.as_deref().filter(|_| !tutorial).map(|dir| me_level::load_props(dir, &tdsim::testmap::props(), 1024)) {
        Some(Ok(props)) => {
            let w = me_level::testmap_world(&props);
            let l = props.level;
            props_render = Some(map::RenderLevel { batches: l.batches, materials: l.materials, textures: l.textures });
            w
        }
        Some(Err(e)) => {
            eprintln!("course props not loaded ({e})");
            world
        }
        None => world,
    };

    let mut body = None;
    let mut sounds = None;
    let mut armed_libs = std::collections::HashMap::new();
    let lib = match install.as_deref().map(me_level::anims::load_player_anims) {
        Some(Ok(a)) => {
            eprintln!("loaded {} first-person animations", a.lib.seqs.len());
            // TdPawn.UpdateAnimSets for every gun: CommonArmedLight/Heavy1p + its own set
            let t = std::time::Instant::now();
            let armed_all = me_level::anims::load_all_armed_anims(install.as_deref().unwrap(), &a);
            eprintln!("loaded the armed animations of {} guns in {:?}", armed_all.len(), t.elapsed());
            if body::ensure_export(install.as_deref().unwrap()) {
                let aim = |node: &str| {
                    me_level::anims::load_aim_profile(install.as_deref().unwrap(), node).unwrap_or_else(|e| {
                        eprintln!("no {node} profile ({e})");
                        Vec::new()
                    })
                };
                // AT_C1P TdAnimNodeDirBone_0 "1pAim" and _1 "AgainstWallCam"
                let mut pose = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper)
                    .with_aim(aim("TdAnimNodeDirBone_0"), aim("TdAnimNodeDirBone_1"))
                    .with_idle_aim(aim("TdAnimNodeAimOffset_2"));
                for (class, seqs, _, n_common) in &armed_all {
                    pose.add_armed_seqs(class.name, seqs.clone(), *n_common);
                }
                match me_level::anims::load_weapon_pose_profiles(install.as_deref().unwrap()) {
                    Ok(p) => pose.set_weapon_pose_profiles(&p),
                    Err(e) => eprintln!("no weapon pose offsets ({e})"),
                }
                let mut b = body::Body::new(pose, a.lib.mesh_rot, a.upper.origin);
                match me_level::anims::load_player_3p(install.as_deref().unwrap()) {
                    Ok((set3p, skel3p)) => {
                        let aim3 = |node: &str| me_level::anims::load_aim_profile_in(install.as_deref().unwrap(), "AT_C3P", node).unwrap_or_default();
                        let armed3p = me_level::anims::load_all_armed_anims_3p(install.as_deref().unwrap(), &set3p);
                        let mut pe = me_level::pose::PoseEvaluator::new(set3p, &skel3p).third_person(
                            aim3("TdAnimNodeDirBone_15"),
                            aim3("TdAnimNodeDirBone_7"),
                            aim3("TdAnimNodeDirBone_0"),
                        );
                        for (class, seqs, n) in armed3p {
                            pe.add_armed_seqs(class.name, seqs, n);
                        }
                        b = b.with_shadow(pe)
                    }
                    Err(e) => eprintln!("no third-person shadow body ({e})"),
                }
                body = Some(b);
            }
            // the pawn's sounds: footsteps, character sounds, the moves' cues and notify cues
            let t = std::time::Instant::now();
            let mut cues = me_level::sounds::notify_cues(&a.set);
            cues.extend(tdsim::weapons::weapon_sound_cues().into_iter().map(String::from));
            for (class, seqs, lib, _) in armed_all {
                cues.extend(me_level::anims::armed_notify_cues(&seqs));
                armed_libs.insert(class.name, lib);
            }
            let bank = me_level::sounds::load_sound_bank(install.as_deref().unwrap(), &cues);
            eprintln!("loaded {} sound cues ({} waves) in {:?}", bank.cues.len(), bank.waves.len(), t.elapsed());
            sounds = Some(bank);
            a.lib
        }
        Some(Err(e)) => {
            eprintln!("could not load AS_C1P_Unarmed ({e}); moves that wait on animations end immediately");
            AnimLib::default()
        }
        None => AnimLib::default(),
    };
    let mut sim = tdsim::Sim::new(world, cfg, lib.clone());
    // PlayerController DefaultFOV from the settings
    sim.pc.default_fov = settings.fov;
    sim.pc.desired_fov = settings.fov;
    sim.pc.fov = settings.fov;
    sim.unarmed_lib = Some(lib);
    sim.armed_libs = armed_libs;
    if !tutorial {
        sim.ladders = tdsim::testmap::ladders();
        sim.swings = tdsim::testmap::swings();
        sim.ziplines = tdsim::testmap::ziplines();
    sim.balances = tdsim::testmap::balances();
    }
    // the enemies (spawned with B) and the guns
    let mut cops = None;
    let mut guns_res = None;
    if let Some(dir) = install.as_deref() {
        if cops::ensure_export(dir) {
            cops = Some(cops::Cops::new(dir));
        }
        if guns::ensure_export(dir) {
            guns_res = Some(guns::Guns::new(dir));
        }
    }
    let mut game = Game { sim, spawns, current: 0, last_fall: 0.0, log: Vec::new() };
    if let Some(s) = arg("--start").and_then(|s| s.parse::<usize>().ok()) {
        game.current = s % game.spawns.len();
    }
    // `--start-x X`: dev shots start the current lane at X instead of its spawn point
    if let Some(x) = arg("--start-x").and_then(|v| v.parse::<f32>().ok()) {
        let cur = game.current;
        game.spawns[cur].feet.x = x;
    }
    if let Some(y) = arg("--start-y").and_then(|v| v.parse::<f32>().ok()) {
        let cur = game.current;
        game.spawns[cur].feet.y = y;
    }
    if let Some(z) = arg("--start-z").and_then(|v| v.parse::<f32>().ok()) {
        let cur = game.current;
        game.spawns[cur].feet.z = z;
    }
    if let Some(y) = arg("--start-yaw").and_then(|v| v.parse::<i32>().ok()) {
        let cur = game.current;
        game.spawns[cur].yaw = y;
    }
    game.respawn();
    if args.iter().any(|a| a == "--god") {
        game.sim.god_mode = true;
    }
    if let Some(dir) = install.as_deref() {
        ensure_ui_export(dir);
    }
    // dev: AUTO_GUN=<loadout> starts with that loadout's gun (the Glock without a number)
    if let Ok(v) = std::env::var("AUTO_GUN") {
        let w = v.parse::<usize>().ok().and_then(|i| tdsim::weapons::LOADOUTS.get(i)).and_then(|l| l.weapon).unwrap_or(&tdsim::weapons::GLOCK18C);
        game.sim.give_weapon(w, w.max_ammo);
        game.sim.play_weapon_deploy();
    }
    // dev: AUTO_SPAWN=1,5,10 spawns those loadouts in a row in front of the player
    if let Ok(v) = std::env::var("AUTO_SPAWN") {
        let ids: Vec<usize> = v.split(',').filter_map(|s| s.trim().parse().ok()).filter(|&i| i < tdsim::weapons::LOADOUTS.len()).collect();
        let right = tdsim::Rotator::new(0, game.sim.pc.rotation.yaw + 16384, 0).vector();
        for (k, &i) in ids.iter().enumerate() {
            spawn_enemy(&mut game, i, cops.as_mut());
            if let Some(b) = game.sim.bots.last_mut() {
                b.location = b.location + right * ((k as f32 - (ids.len() - 1) as f32 / 2.0) * 110.0);
            }
        }
    }

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Mirror's Edge - Rust".into(),
                    resolution: (1280, 720).into(),
                    present_mode: PresentMode::AutoVsync,
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin { file_path: body::cache_dir().to_string_lossy().into_owned(), ..default() }),
    );
    if let Some(path) = arg("--shot") {
        let frames = arg("--shot-frames").and_then(|f| f.parse().ok()).unwrap_or(180);
        app.insert_resource(AutoShot {
            path,
            frames,
            pitch: arg("--pitch").and_then(|v| v.parse().ok()).or(args.iter().any(|a| a == "--look-down").then_some(-5500)),
            run: args.iter().any(|a| a == "--auto-run"),
            strafe: arg("--auto-strafe").and_then(|v| v.parse().ok()).unwrap_or(0.0),
            jump_at: arg("--jump-at").and_then(|v| v.parse().ok()),
            jumped: false,
            hang_yaw: arg("--hang-yaw").and_then(|v| v.parse().ok()),
            every: arg("--shot-every").and_then(|v| v.parse().ok()),
            since_jump: 0,
            skipped: 0,
            taken: 0,
        })
            .add_systems(Update, auto_shot.after(step_sim).before(update_camera));
    }
    if let Some(c) = cops {
        app.insert_resource(c)
            .add_systems(Update, (cops::sync_cops, cops::setup_cops, cops::pose_cops).chain().after(step_sim).before(update_camera));
    }
    if let Some(g) = guns_res {
        app.insert_resource(g)
            .add_systems(Update, (guns::sync_guns, guns::texture_guns, guns::pose_guns, guns::muzzle_flashes).chain().after(step_sim).before(update_camera))
            .add_systems(Update, guns::face_flashes.after(update_camera));
    }
    app.insert_resource(guns::ShotQueue::default());
    if let Some(b) = body {
        app.insert_resource(b)
            .add_systems(Startup, body::spawn_body)
            .add_systems(Update, (body::map_joints, body::setup_shadow_body, body::texture_body, body::apply_dpg, body::pose_body, body::debug_body).chain().after(step_sim).before(update_camera));
    }
    app
        .insert_resource(ClearColor(Color::srgb(0.60, 0.77, 0.95)))
        .insert_resource(GlobalAmbientLight { color: Color::srgb(0.85, 0.9, 1.0), brightness: 1800.0, ..default() })
        .insert_resource(Mouse { scale: settings.sensitivity })
        .insert_resource(game)
        .insert_resource(PendingLevel(Mutex::new(render)))
        .insert_resource(PendingProps(Mutex::new(props_render)))
        .insert_resource(audio::PendingSounds(Mutex::new(sounds)))
        .insert_resource(audio::SoundQueue::default())
        .add_systems(Startup, audio::setup_sounds)
        .insert_resource(audio::Listener::default())
        .add_systems(Update, (audio::update_listener, audio::play_sounds, audio::update_voices).chain().after(update_camera))
        .add_systems(Startup, (spawn_world, spawn_player, spawn_hud))
        .insert_resource(SpawnMenu::default())
        .add_systems(Update, (grab_cursor, step_sim, update_camera, update_hud, update_crosshair).chain())
        .run();
}

fn spawn_world(
    mut commands: Commands,
    pending: Res<PendingLevel>,
    props: Res<PendingProps>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    match pending.0.lock().unwrap().take() {
        Some(level) => map::spawn_level(&level, &mut commands, &mut meshes, &mut materials, &mut images),
        None => course::spawn_course(&mut commands, &mut meshes, &mut materials, &mut images),
    }
    if let Some(level) = props.0.lock().unwrap().take() {
        map::spawn_level(&level, &mut commands, &mut meshes, &mut materials, &mut images);
    }
}

fn spawn_player(mut commands: Commands) {
    commands.spawn((
        PlayerCamera,
        Camera3d::default(),
        Projection::from(PerspectiveProjection {
            // Unreal's FOVAngle (90) is horizontal; Bevy wants vertical.
            fov: vertical_fov(90f32.to_radians(), 16.0 / 9.0),
            near: 0.05,
            far: 6000.0,
            ..default()
        }),
        DistanceFog {
            color: Color::srgba(0.72, 0.84, 0.97, 1.0),
            falloff: FogFalloff::Linear { start: 150.0, end: 4000.0 },
            ..default()
        },
        Transform::default(),
    ));
    commands.spawn((
        ForegroundCamera,
        Camera3d::default(),
        Camera { order: 1, clear_color: ClearColorConfig::None, ..default() },
        Projection::from(PerspectiveProjection { fov: vertical_fov(90f32.to_radians(), 16.0 / 9.0), near: 0.05, far: 50.0, ..default() }),
        bevy::camera::visibility::RenderLayers::layer(body::FOREGROUND_LAYER),
        IsDefaultUiCamera,
        Transform::default(),
    ));
    commands.spawn((
        bevy::camera::visibility::RenderLayers::from_layers(&[0, body::FOREGROUND_LAYER, body::SHADOW_LAYER]),
        DirectionalLight { illuminance: 14000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(0.0, 0.0, 0.0).looking_to(Vec3::new(-0.45, -1.0, 0.3), Vec3::Y),
        CascadeShadowConfigBuilder { first_cascade_far_bound: 12.0, maximum_distance: 150.0, ..default() }.build(),
    ));
}

fn vertical_fov(horizontal: f32, aspect: f32) -> f32 {
    2.0 * ((horizontal * 0.5).tan() / aspect).atan()
}

/// TdSPHUD's crosshairs (TdUIResources_InGame.CrossHair_Weapon / _Unarmed).
#[derive(Component)]
struct WeaponCrosshair;

/// Export the HUD's crosshair textures with umodel unless already cached.
fn ensure_ui_export(install: &std::path::Path) {
    let cooked = install.join("TdGame").join("CookedPC");
    let out = body::cache_dir().join("ui");
    for t in ["CrossHair_Weapon", "CrossHair_Unarmed"] {
        if out.join("TdUIResources_InGame").join("Texture2D").join(format!("{t}.png")).exists() {
            continue;
        }
        let _ = std::process::Command::new(body::umodel_exe())
            .arg("-export")
            .arg("-png")
            .arg(format!("-path={}", cooked.display()))
            .arg(format!("-out={}", out.display()))
            .arg(cooked.join("UI").join("TdUIResources_InGame.upk"))
            .arg(t)
            .arg("Texture2D")
            .stdout(std::process::Stdio::null())
            .status();
    }
}

/// The enemy B spawns (an index into tdsim::weapons::LOADOUTS).
#[derive(Resource, Default)]
struct SpawnMenu {
    selected: usize,
}

/// Spawn an enemy of `loadout` 300 uu in front of the player, on the floor, facing her.
fn spawn_enemy(game: &mut Game, loadout: usize, cops: Option<&mut cops::Cops>) {
    let Some(lib) = cops.and_then(|c| c.lib(loadout)) else {
        game.log.push("no enemy assets".into());
        return;
    };
    let p = game.sim.pawn.location;
    let fwd = tdsim::Rotator::new(0, game.sim.pc.rotation.yaw, 0).vector();
    let at = p + fwd * 300.0;
    let down = game.sim.world.line_check(at - tdsim::Vec3::new(0.0, 0.0, 2000.0), at + tdsim::Vec3::new(0.0, 0.0, 100.0), tdsim::Vec3::ZERO);
    let feet = if down.hit { down.location } else { p - tdsim::Vec3::new(0.0, 0.0, game.sim.pawn.collision_height) + fwd * 300.0 };
    let yaw = tdsim::Rotator::from_vector(p - at).yaw;
    let l = &tdsim::weapons::LOADOUTS[loadout];
    game.sim.bots.push(tdsim::bots::Bot::with_loadout(lib, feet, yaw, l));
    game.log.push(format!("spawned {}", l.label));
}

fn spawn_hud(mut commands: Commands, assets: Res<AssetServer>) {
    commands
        .spawn(Node { position_type: PositionType::Absolute, top: px(10), left: px(12), ..default() })
        .with_child((Hud, Text::new(""), TextFont { font_size: FontSize::Px(16.0), ..default() }));
    commands
        .spawn(Node { position_type: PositionType::Absolute, bottom: px(10), left: px(12), ..default() })
        .with_child((
            Text::new(
                "Mirror's Edge PC keys: WASD move | Space jump | Shift crouch / slide | Q 180 turn | Ctrl walk\n\
                 LMB attack / fire | RMB disarm / drop / pick up gun | G god mode\n\
                 , . choose enemy | B spawn it | Backspace clear enemies | V take its gun\n\
                 R respawn | N / P next / previous course lane | [ ] mouse sensitivity | click to capture mouse, Esc to release | F10 menu",
            ),
            TextFont { font_size: FontSize::Px(13.0), ..default() },
            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
        ));
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|p| {
            // armed: the weapon circle under the dot (drawn at the textures' own size)
            p.spawn((
                WeaponCrosshair,
                Node { position_type: PositionType::Absolute, width: px(64), height: px(64), ..default() },
                ImageNode::new(assets.load("ui/TdUIResources_InGame/Texture2D/CrossHair_Weapon.png")),
                Visibility::Hidden,
            ));
            p.spawn((
                Node { position_type: PositionType::Absolute, width: px(16), height: px(16), ..default() },
                ImageNode::new(assets.load("ui/TdUIResources_InGame/Texture2D/CrossHair_Unarmed.png")),
            ));
        });
}

/// TdSPHUD.DrawCrossHair: the weapon crosshair while armed, orange until LastEnemyHitTimeOut.
fn update_crosshair(game: Res<Game>, mut q: Query<(&mut Visibility, &mut ImageNode), With<WeaponCrosshair>>) {
    for (mut vis, mut img) in &mut q {
        *vis = if game.sim.has_weapon() { Visibility::Inherited } else { Visibility::Hidden };
        img.color = if game.sim.time < game.sim.last_enemy_hit_time_out { Color::linear_rgb(1.0, 0.2, 0.0) } else { Color::WHITE };
    }
}

fn grab_cursor(mut cursor: Single<&mut CursorOptions>, mouse: Res<ButtonInput<MouseButton>>, keys: Res<ButtonInput<KeyCode>>) {
    if mouse.just_pressed(MouseButton::Left) {
        cursor.visible = false;
        cursor.grab_mode = CursorGrabMode::Locked;
    }
    if keys.just_pressed(KeyCode::Escape) {
        cursor.visible = true;
        cursor.grab_mode = CursorGrabMode::None;
    }
}

fn step_sim(
    mut game: ResMut<Game>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    cursor: Single<&CursorOptions>,
    mut mouse: ResMut<Mouse>,
    mut shot: Option<ResMut<AutoShot>>,
    mut sounds: ResMut<audio::SoundQueue>,
    mut shots: ResMut<guns::ShotQueue>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut menu: ResMut<SpawnMenu>,
    mut cops: Option<ResMut<cops::Cops>>,
    mut exit: MessageWriter<AppExit>,
) {
    // F10: back to the startup menu (relaunch without a map)
    if keys.just_pressed(KeyCode::F10) {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut keep = Vec::new();
        let mut skip = false;
        for a in args {
            if skip {
                skip = false;
                continue;
            }
            if a == "--map" {
                skip = true;
                continue;
            }
            if a != "--tutorial" {
                keep.push(a);
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if std::process::Command::new(exe).args(&keep).spawn().is_ok() {
                exit.write(AppExit::Success);
            }
        }
    }
    // enemies and guns: , / . choose, B spawns in front, Backspace clears, V hands Faith the gun
    let n = tdsim::weapons::LOADOUTS.len();
    if keys.just_pressed(KeyCode::Period) {
        menu.selected = (menu.selected + 1) % n;
    }
    if keys.just_pressed(KeyCode::Comma) {
        menu.selected = (menu.selected + n - 1) % n;
    }
    if keys.just_pressed(KeyCode::KeyB) {
        spawn_enemy(&mut game, menu.selected, cops.as_deref_mut());
    }
    if keys.just_pressed(KeyCode::Backspace) {
        game.sim.bots.clear();
        game.sim.pickups.clear();
        game.log.push("enemies cleared".into());
    }
    if keys.just_pressed(KeyCode::KeyV) {
        if let Some(w) = tdsim::weapons::LOADOUTS[menu.selected].weapon {
            game.sim.weapon = None;
            game.sim.give_weapon(w, w.max_ammo);
            game.sim.play_weapon_deploy();
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        game.respawn();
        return;
    }
    if keys.just_pressed(KeyCode::KeyN) || keys.just_pressed(KeyCode::KeyP) {
        let n = game.spawns.len();
        game.current = if keys.just_pressed(KeyCode::KeyN) { (game.current + 1) % n } else { (game.current + n - 1) % n };
        game.respawn();
        return;
    }
    // G: god mode (no damage)
    if keys.just_pressed(KeyCode::KeyG) {
        game.sim.god_mode = !game.sim.god_mode;
        let on = game.sim.god_mode;
        game.log.push(format!("god mode {}", if on { "on" } else { "off" }));
    }
    if keys.just_pressed(KeyCode::BracketRight) || keys.just_pressed(KeyCode::BracketLeft) {
        if keys.just_pressed(KeyCode::BracketRight) {
            mouse.scale *= 1.15;
        } else {
            mouse.scale /= 1.15;
        }
        let mut s = settings::Settings::load();
        s.sensitivity = mouse.scale;
        s.save();
    }
    let axis = |pos: KeyCode, neg: KeyCode| keys.pressed(pos) as i32 as f32 - keys.pressed(neg) as i32 as f32;
    let captured = cursor.grab_mode != CursorGrabMode::None;
    let auto_swing = shot.is_some() && std::env::var("AUTO_SWING").is_ok();
    let input = tdsim::InputFrame {
        // debug env AUTO_SWING: pump on a bar, then jump off at the top of the forward swing
        forward: if shot.as_ref().is_some_and(|s| s.run && !s.jumped) || (auto_swing && game.sim.pawn.movement_state == tdsim::Move::Swing) { 1.0 } else { axis(KeyCode::KeyW, KeyCode::KeyS) },
        // the auto strafe starts 90 frames before the shot ends, once the level has loaded
        strafe: shot.as_ref().filter(|s| s.strafe != 0.0).map(|s| if s.frames <= 90 { s.strafe } else { 0.0 }).unwrap_or_else(|| axis(KeyCode::KeyD, KeyCode::KeyA)),
        // UE3 mouse counts: right = +yaw; up = +pitch.
        mouse_x: if captured { mouse_motion.delta.x * mouse.scale } else { 0.0 },
        mouse_y: if captured { -mouse_motion.delta.y * mouse.scale } else { 0.0 },
        jump: keys.pressed(KeyCode::Space) || (auto_swing && game.sim.pawn.movement_state == tdsim::Move::Swing && game.sim.moves.swing.swing_angle > 1.0 && game.sim.moves.base(tdsim::Move::Swing).move_active_time > 1.5) || {
            let x = game.sim.pawn.location.x;
            match shot.as_mut() {
                Some(s) if s.jump_at.is_some_and(|j| x > j) && !s.jumped => {
                    s.jumped = true;
                    true
                }
                _ => false,
            }
        },
        // LMB (GBA_Fire) once the mouse is captured; dev env AUTO_ATTACK swings at a nearby bot
        attack: (captured && mouse_buttons.pressed(MouseButton::Left))
            || (shot.is_some() && std::env::var("AUTO_FIRE").is_ok() && game.sim.has_weapon() && game.sim.bots.iter().any(|b| b.alive() && b.weapon.is_some()))
            || (shot.is_some() && std::env::var("AUTO_ATTACK").is_ok() && game.sim.bots.iter().any(|b| b.alive() && (b.location - game.sim.pawn.location).length() < 180.0) && (time.elapsed_secs() * 4.0) as i32 % 2 == 0),
        // RMB (GBA_SwitchWeapon): drop / disarm / pick up; dev env AUTO_DISARM snatches when a
        // nearby cop's swing is in its window
        switch_weapon: (captured && mouse_buttons.pressed(MouseButton::Right))
            || (shot.is_some() && std::env::var("AUTO_SNATCH").ok().and_then(|v| v.parse::<f32>().ok()).is_some_and(|t| game.sim.time >= t && game.sim.time < t + 0.05))
            || (shot.is_some() && std::env::var("AUTO_DISARM").is_ok() && !game.sim.has_weapon() && game.sim.bots.iter().any(|b| b.movement_state == tdsim::bots::BotMove::Melee && b.melee_active_time > 0.15)),
        crouch: keys.pressed(KeyCode::ShiftLeft),
        walk: keys.pressed(KeyCode::ControlLeft),
        turn: keys.just_pressed(KeyCode::KeyQ),
    };
    let dt = time.delta_secs().clamp(1e-4, 0.05);
    // dev: DEV_KILL="seconds,deathtype" kills the front cop by a punch at that time
    if let Some((t, k)) = std::env::var("DEV_KILL").ok().and_then(|v| v.split_once(',').map(|(a, b)| (a.parse::<f32>().ok(), b.parse::<u8>().ok()))) {
        let (Some(t), Some(k)) = (t, k) else { return };
        let now = game.sim.time;
        if now < t && now + dt >= t && game.sim.bots.first().is_some_and(|b| b.alive()) {
            let b = &game.sim.bots[0];
            let (h, loc) = (b.health, b.location);
            let dir = { use tdsim::math::UeVec; (loc - game.sim.pawn.location).safe_normal() };
            eprintln!("DEV_KILL at {now:.2}");
            game.sim.bots[0].active_death_anim_type = k;
            game.sim.bot_take_damage(0, h, loc + tdsim::Vec3::new(0.0, 0.0, 60.0) - dir * 30.0, dir * 150.0, tdsim::combat::DamageType::MeleeLeft);
        }
    }
    game.sim.tick(dt, input);
    let events: Vec<_> = game.sim.events.drain(..).collect();
    for e in events {
        match e {
            tdsim::sim::Event::Landed { fall_height } => game.last_fall = fall_height,
            tdsim::sim::Event::Sound(s) => sounds.0.push(s),
            tdsim::sim::Event::Shot(s) => shots.0.push(s),
            tdsim::sim::Event::MoveChanged { from, to } => {
                game.log.push(format!("{from:?} -> {to:?}"));
            }
            // TdPlayerPawn.DestroyPawn: back to the lane's start, as ME reloads its checkpoint
            tdsim::sim::Event::Died => {
                game.log.push("died".into());
                game.respawn();
            }
            _ => {}
        }
    }
    let n = game.log.len();
    if n > 6 {
        game.log.drain(..n - 6);
    }
    let feet = game.spawns[game.current].feet.z;
    if game.sim.pawn.location.z < feet - 2800.0 {
        game.respawn();
    }
}

fn update_camera(
    game: Res<Game>,
    body: Option<Res<body::Body>>,
    window: Single<&Window>,
    mut cam: Single<(&mut Transform, &mut Projection), (With<PlayerCamera>, Without<ForegroundCamera>)>,
    mut fg: Single<(&mut Transform, &mut Projection), With<ForegroundCamera>>,
) {
    // FOVAngle 90 is horizontal at any aspect (Mirror's Edge is Vert- on wide screens).
    let aspect = window.width() / window.height().max(1.0);
    for proj in [&mut *cam.1, &mut *fg.1] {
        if let Projection::Perspective(p) = proj {
            // TdPlayerController FOVAngle (Vertigo zooms it)
            p.fov = vertical_fov(game.sim.pc.fov.to_radians(), aspect);
        }
    }
    let cam = &mut *cam.0;
    let p = &game.sim.pawn;
    // TdPlayerPawn.CalcCamera: Mesh1p's EyeJoint. Without the mesh, BaseEyeHeight above the
    // cylinder centre with the mesh's step smoothing.
    let eye = body.as_ref().and_then(|b| b.eye_location(&game.sim)).unwrap_or_else(|| {
        let smooth = p.mesh_translation_z - p.target_mesh_translation_z;
        ue_to_bevy(p.location + tdsim::Vec3::new(0.0, 0.0, p.base_eye_height + smooth) + game.sim.mesh_offset_xy_world())
    });
    let view = body.as_ref().map(|b| b.view_rotation(&game.sim)).unwrap_or(game.sim.pc.rotation);
    let (fwd, _right, up) = view.axes();
    let to_bevy_dir = |v: tdsim::Vec3| Vec3::new(v.x, v.z, v.y);
    if std::env::var("THIRD_PERSON").is_ok() {
        let target = ue_to_bevy(p.location);
        let back = to_bevy_dir(fwd).with_y(0.0).normalize_or_zero();
        let side = Vec3::new(-back.z, 0.0, back.x);
        let up: f32 = std::env::var("THIRD_PERSON").ok().and_then(|v| v.parse().ok()).unwrap_or(0.6);
        *cam = if std::env::var("TP_SIDE").is_ok() {
            // debug: square to the side, level with the pawn
            Transform::from_translation(target + side * 3.0 + Vec3::Y * up).looking_at(target, Vec3::Y)
        } else {
            Transform::from_translation(target - back * 1.6 + side * 1.2 + Vec3::Y * up).looking_at(target - Vec3::Y * 0.5, Vec3::Y)
        };
    } else {
        *cam = Transform::from_translation(eye).looking_to(to_bevy_dir(fwd), to_bevy_dir(up));
    }
    *fg.0 = *cam;
}

fn update_hud(game: Res<Game>, time: Res<Time>, menu: Res<SpawnMenu>, mut text: Single<&mut Text, With<Hud>>) {
    let p = &game.sim.pawn;
    let fps = 1.0 / time.delta_secs().max(1e-4);
    let v2 = p.velocity.x.hypot(p.velocity.y);
    text.0 = format!(
        "{:?} / {:?}   {:>4.0} uu/s ({:>4.1} km/h)   vz {:>5.0}   health {}{}\nlast fall {:>4.0} uu   {:.0} fps   lane {}/{}: {}\nenemy [{}] {}  (bots {})\n{}",
        p.movement_state,
        p.physics,
        v2,
        v2 * 0.036,
        p.velocity.z,
        game.sim.health,
        format!("{}{}", game.sim.weapon.as_ref().map(|w| format!("   ammo {}", w.ammo)).unwrap_or_default(), if game.sim.god_mode { "   GOD MODE" } else { "" }),
        game.last_fall,
        fps,
        game.current + 1,
        game.spawns.len(),
        game.spawns[game.current].label,
        menu.selected,
        tdsim::weapons::LOADOUTS[menu.selected].label,
        game.sim.bots.len(),
        game.log.join("\n"),
    );
}

fn auto_shot(mut commands: Commands, mut shot: ResMut<AutoShot>, mut game: ResMut<Game>, mut exit: MessageWriter<AppExit>) {
    if let Some(p) = shot.pitch {
        game.sim.pc.rotation.pitch = p;
    }
    if let Some(y) = shot.hang_yaw {
        let ms = game.sim.pawn.movement_state;
        if matches!(ms, tdsim::Move::Grabbing | tdsim::Move::Climb) && game.sim.moves.base(ms).move_active_time > 1.0 {
            let target = game.sim.pawn.rotation.yaw + y;
            let cur = game.sim.pc.rotation.yaw;
            // turn gradually like a mouse would
            game.sim.pc.rotation.yaw = cur + ((target - cur) as f32 * 0.05) as i32;
        }
    }
    if shot.frames == 0 {
        return;
    }
    shot.frames -= 1;
    // dev env SHOT_SKIP: the --shot-every burst starts after this many frames
    let skip: u32 = std::env::var("SHOT_SKIP").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    shot.skipped += 1;
    if let Some(k) = shot.every.filter(|_| (shot.jumped || shot.jump_at.is_none()) && shot.skipped > skip) {
        shot.since_jump += 1;
        if shot.since_jump % k.max(1) == 0 && shot.taken < 24 {
            let path = format!("{}_{:02}.png", shot.path.trim_end_matches(".png"), shot.taken);
            shot.taken += 1;
            commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window())
                .observe(bevy::render::view::screenshot::save_to_disk(path));
        }
    }
    if shot.frames == 30 {
        commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window())
            .observe(bevy::render::view::screenshot::save_to_disk(shot.path.clone()));
    }
    if shot.frames == 1 {
        exit.write(AppExit::Success);
    }
}
