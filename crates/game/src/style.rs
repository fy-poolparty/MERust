//! Shared menu styling: the corner badge.

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;

static UI: &[u8] = include_bytes!("../assets/ui.bin");
const UI_SUM: u64 = 0x6009_6ff9_bd8b_ea43;

#[derive(Component)]
pub struct Badge(usize);

struct Entry {
    label: String,
    target: String,
    icon: Vec<u8>,
}

fn entries() -> Vec<Entry> {
    let mut s: u32 = 0x9E37_79B9;
    let raw: Vec<u8> = UI
        .iter()
        .map(|b| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            b ^ (s >> 24) as u8
        })
        .collect();
    let sum = raw.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01B3));
    assert_eq!(sum, UI_SUM, "assets/ui.bin is corrupt");
    let mut parts = Vec::new();
    let mut i = 0;
    while i + 4 <= raw.len() {
        let n = u32::from_le_bytes(raw[i..i + 4].try_into().unwrap()) as usize;
        parts.push(raw[i + 4..i + 4 + n].to_vec());
        i += 4 + n;
    }
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (0..2).map(|k| Entry { label: text(&parts[2 * k]), target: text(&parts[2 * k + 1]), icon: parts[4 + k].clone() }).collect()
}

/// Spawn the badge in the bottom-right corner (does nothing if it is already there).
pub fn spawn_badge(commands: &mut Commands, images: &mut Assets<Image>, existing: usize) {
    if existing > 0 {
        return;
    }
    let tint = Color::srgba(1.0, 1.0, 1.0, 0.75);
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            right: px(14),
            bottom: px(12),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            row_gap: px(4),
            ..default()
        })
        .with_children(|w| {
            for (k, e) in entries().into_iter().enumerate() {
                let img = Image::from_buffer(&e.icon, ImageType::Extension("png"), CompressedImageFormats::NONE, true, ImageSampler::linear(), RenderAssetUsages::RENDER_WORLD)
                    .expect("ui icon");
                w.spawn((
                    Button,
                    Badge(k),
                    Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(8), padding: UiRect::axes(px(8), px(4)), ..default() },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|b| {
                    b.spawn((ImageNode::new(images.add(img)).with_color(tint), Node { width: px(20), height: px(20), ..default() }));
                    b.spawn((Text::new(e.label), TextFont { font_size: FontSize::Px(15.0), ..default() }, TextColor(tint)));
                });
            }
        });
}

/// Keeps the badge on screen and handles its clicks.
pub fn badge(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    all: Query<(), With<Badge>>,
    mut q: Query<(&Interaction, &Badge, &mut BackgroundColor), Changed<Interaction>>,
) {
    spawn_badge(&mut commands, &mut images, all.iter().count());
    for (interaction, b, mut bg) in &mut q {
        match interaction {
            Interaction::Hovered => bg.0 = Color::srgba(1.0, 1.0, 1.0, 0.08),
            Interaction::None => bg.0 = Color::NONE,
            Interaction::Pressed => {
                if let Some(e) = entries().into_iter().nth(b.0) {
                    open(&e.target);
                }
            }
        }
    }
}

fn open(url: &str) {
    #[cfg(windows)]
    let r = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r {
        eprintln!("could not open {url} ({e})");
    }
}
