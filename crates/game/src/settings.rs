//! The player's settings (mouse sensitivity, field of view, volume), kept in `settings.txt`
//! beside the executable as `key = value` lines.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Mouse counts per pixel of motion before MouseSensitivity (1 = the game's).
    pub sensitivity: f32,
    /// PlayerController DefaultFOV (horizontal degrees; the game's is 90).
    pub fov: f32,
    /// Master volume, 0..1.
    pub volume: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { sensitivity: 1.0, fov: 90.0, volume: 0.9 }
    }
}

fn path() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("settings.txt"))).unwrap_or_else(|| PathBuf::from("settings.txt"))
}

impl Settings {
    pub fn load() -> Self {
        let mut s = Settings::default();
        let Ok(text) = std::fs::read_to_string(path()) else { return s };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let Ok(v) = v.trim().parse::<f32>() else { continue };
            match k.trim() {
                "sensitivity" => s.sensitivity = v.clamp(0.1, 10.0),
                "fov" => s.fov = v.clamp(60.0, 120.0),
                "volume" => s.volume = v.clamp(0.0, 1.0),
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let text = format!("sensitivity = {:.3}\nfov = {:.0}\nvolume = {:.2}\n", self.sensitivity, self.fov, self.volume);
        if let Err(e) = std::fs::write(path(), text) {
            eprintln!("could not save settings ({e})");
        }
    }
}

/// The master volume the audio reads every frame.
static VOLUME: AtomicU32 = AtomicU32::new(0x3F66_6666); // 0.9

pub fn set_volume(v: f32) {
    VOLUME.store(v.to_bits(), Ordering::Relaxed);
}

pub fn volume() -> f32 {
    f32::from_bits(VOLUME.load(Ordering::Relaxed))
}
