//! The startup menu: choose the map, adjust the settings and point the game at your Mirror's Edge
//! install. Nothing from the game ships with this project: every model, animation, sound and ini
//! value is read from your own install, so the first launch asks for that folder and remembers it
//! (me_level::remember_install). Winit runs one event loop per process, so Play relaunches this
//! executable with `--map test` (which skips the menu) and closes the menu.

use crate::settings::Settings;
use bevy::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Sensitivity,
    Fov,
    Volume,
}

#[derive(Component, Clone, Copy)]
enum MenuButton {
    Play,
    Adjust(Field, f32),
    PickFolder,
    Quit,
}

#[derive(Component)]
struct ValueText(Field);

#[derive(Component)]
struct InstallText;

#[derive(Resource)]
struct Menu {
    settings: Settings,
    /// The command line to pass on (minus the program name).
    args: Vec<String>,
    install: Option<PathBuf>,
    /// Why the last folder choice was rejected.
    folder_error: Option<String>,
}

const IDLE: Color = Color::srgb(0.16, 0.17, 0.2);
const HOVER: Color = Color::srgb(0.27, 0.29, 0.34);
const ACCENT: Color = Color::srgb(0.75, 0.12, 0.1);
const ACCENT_HOVER: Color = Color::srgb(0.9, 0.2, 0.15);
const DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.6);
const DISABLED: Color = Color::srgb(0.12, 0.125, 0.14);

pub fn run(args: Vec<String>, install: Option<PathBuf>) {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "Mirror's Edge - Rust".into(), resolution: (960, 640).into(), ..default() }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.07, 0.075, 0.09)))
        .insert_resource(Menu { settings: Settings::load(), args, install, folder_error: None })
        .add_systems(Startup, setup)
        .add_systems(Update, (buttons, refresh_values, ask_install_on_launch, crate::style::badge, dev_shot))
        .run();
}

fn text(s: impl Into<String>, size: f32) -> (Text, TextFont) {
    (Text::new(s), TextFont { font_size: FontSize::Px(size), ..default() })
}

fn button(p: &mut ChildSpawnerCommands, label: &str, action: MenuButton, width: f32, color: Color) {
    p.spawn((
        Button,
        action,
        Node { width: px(width), height: px(44), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
        BackgroundColor(color),
    ))
    .with_child(text(label, 18.0));
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(14),
            ..default()
        })
        .with_children(|p| {
            p.spawn((text("MIRROR'S EDGE", 44.0), TextColor(ACCENT)));
            p.spawn((text("Rust port", 18.0), TextColor(DIM)));
            p.spawn(Node { height: px(12), ..default() });
            button(p, "Test map", MenuButton::Play, 320.0, ACCENT);
            // Tutorial: work in progress, shown crossed out and not clickable
            p.spawn((
                Node { width: px(320), height: px(44), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                BackgroundColor(DISABLED),
            ))
            .with_children(|b| {
                b.spawn((text("Tutorial", 18.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.35))));
                // the strike-through line over the label
                b.spawn((
                    Node { position_type: PositionType::Absolute, width: px(90), height: px(2), left: px(115), top: px(22), ..default() },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.55)),
                ));
                b.spawn((
                    Node { position_type: PositionType::Absolute, right: px(10), ..default() },
                    text("WIP", 14.0),
                    TextColor(Color::srgb(0.95, 0.75, 0.2)),
                ));
            });
            p.spawn(Node { height: px(12), ..default() });
            for (field, name, step) in [(Field::Sensitivity, "Mouse sensitivity", 1.15), (Field::Fov, "Field of view", 5.0), (Field::Volume, "Volume", 0.1)] {
                p.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(10), ..default() }).with_children(|r| {
                    r.spawn(Node { width: px(200), height: px(44), align_items: AlignItems::Center, ..default() }).with_child(text(name, 18.0));
                    button(r, "-", MenuButton::Adjust(field, -step), 44.0, IDLE);
                    r.spawn(Node { width: px(80), height: px(44), align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default() })
                        .with_child((ValueText(field), text("", 18.0)));
                    button(r, "+", MenuButton::Adjust(field, step), 44.0, IDLE);
                });
            }
            p.spawn(Node { height: px(12), ..default() });
            // the install folder: everything is loaded from it at runtime
            p.spawn(Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(12), ..default() }).with_children(|r| {
                r.spawn(Node { max_width: px(560), ..default() }).with_child((InstallText, text("", 14.0), TextColor(DIM)));
                button(r, "Change folder", MenuButton::PickFolder, 170.0, IDLE);
            });
            if !crate::body::umodel_exe().is_file() {
                p.spawn((
                    text("umodel not found: models will be missing. Put umodel_64.exe in third_party/umodel or set UMODEL.", 14.0),
                    TextColor(Color::srgb(1.0, 0.45, 0.35)),
                ));
            }
            p.spawn(Node { height: px(12), ..default() });
            button(p, "Quit", MenuButton::Quit, 160.0, IDLE);
        });

}

fn adjust(s: &mut Settings, field: Field, step: f32) {
    match field {
        // multiplicative, like the game's [ ] keys
        Field::Sensitivity => s.sensitivity = (if step > 0.0 { s.sensitivity * step } else { s.sensitivity / -step }).clamp(0.1, 10.0),
        Field::Fov => s.fov = (s.fov + step).clamp(60.0, 120.0),
        Field::Volume => s.volume = ((s.volume + step) * 10.0).round().clamp(0.0, 10.0) / 10.0,
    }
}

/// The chosen folder, or a Mirror's Edge install directly inside it (people often pick the
/// parent, e.g. steamapps\common).
fn resolve_install(p: &Path) -> Option<PathBuf> {
    if me_level::is_install(p) {
        return Some(p.to_path_buf());
    }
    std::fs::read_dir(p).ok()?.flatten().map(|e| e.path()).find(|c| me_level::is_install(c))
}

/// Ask for the install folder (a native folder dialog) and remember it.
fn pick_folder(menu: &mut Menu) {
    let Some(dir) = rfd::FileDialog::new().set_title("Select your Mirror's Edge folder (the one with TdGame and Binaries)").pick_folder() else {
        return;
    };
    match resolve_install(&dir) {
        Some(p) => {
            if let Err(e) = me_level::remember_install(&p) {
                eprintln!("could not remember the install folder ({e})");
            }
            menu.install = Some(p);
            menu.folder_error = None;
        }
        None => menu.folder_error = Some(format!("{} is not a Mirror's Edge folder (no TdGame\\CookedPC)", dir.display())),
    }
}

fn buttons(
    mut menu: ResMut<Menu>,
    mut q: Query<(&Interaction, &MenuButton, &mut BackgroundColor), Changed<Interaction>>,
    mut exit: MessageWriter<AppExit>,
) {
    for (interaction, action, mut bg) in &mut q {
        let (base, hover) = match action {
            MenuButton::Play => (ACCENT, ACCENT_HOVER),
            _ => (IDLE, HOVER),
        };
        match interaction {
            Interaction::Hovered => bg.0 = hover,
            Interaction::None => bg.0 = base,
            Interaction::Pressed => match *action {
                MenuButton::Adjust(field, step) => {
                    let mut s = menu.settings;
                    adjust(&mut s, field, step);
                    menu.settings = s;
                    s.save();
                }
                MenuButton::PickFolder => pick_folder(&mut menu),
                MenuButton::Play => {
                    menu.settings.save();
                    let mut args = menu.args.clone();
                    args.push("--map".into());
                    args.push("test".into());
                    match std::env::current_exe().map(|exe| std::process::Command::new(exe).args(&args).spawn()) {
                        Ok(Ok(_)) => {
                            exit.write(AppExit::Success);
                        }
                        Ok(Err(e)) | Err(e) => eprintln!("could not start the game ({e})"),
                    }
                }
                MenuButton::Quit => {
                    exit.write(AppExit::Success);
                }
            },
        }
    }
}

/// First launch (no install found or remembered): ask for the folder right away.
fn ask_install_on_launch(mut menu: ResMut<Menu>, mut frames: Local<u32>) {
    *frames += 1;
    // a few frames in, so the menu window is up behind the dialog
    if *frames == 5 && menu.install.is_none() && std::env::var("MENU_SHOT").is_err() {
        pick_folder(&mut menu);
    }
}

fn refresh_values(menu: Res<Menu>, mut values: Query<(&ValueText, &mut Text), Without<InstallText>>, mut install: Query<(&mut Text, &mut TextColor), With<InstallText>>) {
    if !menu.is_changed() {
        return;
    }
    for (v, mut t) in &mut values {
        t.0 = match v.0 {
            Field::Sensitivity => format!("{:.2}", menu.settings.sensitivity),
            Field::Fov => format!("{:.0}", menu.settings.fov),
            Field::Volume => format!("{:.0}%", menu.settings.volume * 100.0),
        };
    }
    for (mut t, mut c) in &mut install {
        (t.0, c.0) = match (&menu.folder_error, &menu.install) {
            (Some(e), _) => (e.clone(), Color::srgb(1.0, 0.45, 0.35)),
            (None, Some(p)) => (format!("Mirror's Edge folder: {}", p.display()), DIM),
            (None, None) => ("Mirror's Edge folder not set: choose it to load the game's models, animations and sounds".into(), Color::srgb(0.95, 0.75, 0.2)),
        };
    }
}

/// Dev: MENU_SHOT=<png> saves the menu after a few frames and quits.
fn dev_shot(mut commands: Commands, mut frames: Local<u32>, mut exit: MessageWriter<AppExit>) {
    let Ok(path) = std::env::var("MENU_SHOT") else { return };
    *frames += 1;
    if *frames == 30 {
        commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk(path));
    }
    if *frames == 120 {
        exit.write(AppExit::Success);
    }
}
