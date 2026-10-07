//! UE3 config (.ini) values, read from the user's install (`TdGame/Config/Default*.ini`).
//!
//! A `config` property on class C takes its value from `[TdGame.C]`, falling back to the nearest
//! parent class section that sets it, then to the class's `defaultproperties`. Values are parsed
//! the way UE3's ImportText does (C `atof`, so `"- 200"` reads as 0 and `"0.15f"` as 0.15).

use crate::math::Vec3;
use std::collections::HashMap;
use std::path::Path;

#[derive(Default, Clone)]
pub struct Config {
    /// section (lowercase) -> key (lowercase) -> values in file order (arrays repeat keys).
    sections: HashMap<String, HashMap<String, Vec<String>>>,
}

impl Config {
    /// Loads `DefaultGame.ini` and `DefaultPawnMovement.ini` from `<install>/TdGame/Config`.
    pub fn load(install: &Path) -> Self {
        let mut c = Config::default();
        let dir = install.join("TdGame").join("Config");
        for f in ["DefaultGame.ini", "DefaultPawnMovement.ini", "DefaultEngine.ini", "DefaultInput.ini"] {
            if let Ok(text) = std::fs::read(dir.join(f)) {
                c.add_text(&String::from_utf8_lossy(&text));
            }
        }
        c
    }

    pub fn add_text(&mut self, text: &str) {
        let mut cur: Option<String> = None;
        for line in text.lines() {
            let l = line.trim().trim_start_matches('\u{feff}');
            if l.is_empty() || l.starts_with(';') || l.starts_with('#') {
                continue;
            }
            if l.starts_with('[') && l.ends_with(']') {
                cur = Some(l[1..l.len() - 1].trim().to_ascii_lowercase());
                continue;
            }
            let Some(sec) = &cur else { continue };
            let Some(eq) = l.find('=') else { continue };
            let mut key = l[..eq].trim().to_ascii_lowercase();
            // `+Key=` / `.Key=` array syntax
            if key.starts_with('+') || key.starts_with('.') || key.starts_with('-') {
                key.remove(0);
            }
            let val = l[eq + 1..].trim().to_string();
            self.sections.entry(sec.clone()).or_default().entry(key).or_default().push(val);
        }
    }

    /// Raw value for `key` looked up through `classes` (most derived first, names without the
    /// `TdGame.` package). The last assignment in a section wins.
    pub fn raw(&self, classes: &[&str], key: &str) -> Option<&str> {
        let key = key.to_ascii_lowercase();
        for c in classes {
            let sec = format!("tdgame.{}", c.to_ascii_lowercase());
            if let Some(v) = self.sections.get(&sec).and_then(|s| s.get(&key)).and_then(|v| v.last()) {
                return Some(v.as_str());
            }
        }
        None
    }

    pub fn raw_in(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .get(&section.to_ascii_lowercase())
            .and_then(|s| s.get(&key.to_ascii_lowercase()))
            .and_then(|v| v.last())
            .map(|s| s.as_str())
    }

    pub fn f32(&self, classes: &[&str], key: &str, default: f32) -> f32 {
        self.raw(classes, key).map(atof).unwrap_or(default)
    }

    pub fn i32(&self, classes: &[&str], key: &str, default: i32) -> i32 {
        self.raw(classes, key).map(atoi).unwrap_or(default)
    }

    pub fn bool(&self, classes: &[&str], key: &str, default: bool) -> bool {
        self.raw(classes, key).map(parse_bool).unwrap_or(default)
    }

    pub fn vec3(&self, classes: &[&str], key: &str, default: Vec3) -> Vec3 {
        self.raw(classes, key).map(|s| parse_vec3(s, default)).unwrap_or(default)
    }
}

/// C `atof`: optional whitespace, sign, digits, fraction, exponent; stops at the first bad char.
pub fn atof(s: &str) -> f32 {
    let b = s.trim_start().as_bytes();
    let mut i = 0;
    let mut neg = false;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        neg = b[i] == b'-';
        i += 1;
    }
    let start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == start || (i == start + 1 && b[start] == b'.') {
        return 0.0;
    }
    let mut end = i;
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let ds = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > ds {
            end = j;
        }
    }
    let v: f64 = std::str::from_utf8(&b[start..end]).ok().and_then(|t| t.parse().ok()).unwrap_or(0.0);
    (if neg { -v } else { v }) as f32
}

pub fn atoi(s: &str) -> i32 {
    let b = s.trim_start().as_bytes();
    let mut i = 0;
    let mut neg = false;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        neg = b[i] == b'-';
        i += 1;
    }
    let mut v: i64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        v = v * 10 + (b[i] - b'0') as i64;
        i += 1;
    }
    (if neg { -v } else { v }) as i32
}

pub fn parse_bool(s: &str) -> bool {
    let t = s.trim();
    ["true", "yes", "on", "1"].iter().any(|k| t.eq_ignore_ascii_case(k))
}

/// `(X=1,Y=2,Z=3)`; missing components keep `default`.
pub fn parse_vec3(s: &str, default: Vec3) -> Vec3 {
    let mut v = default;
    for part in s.trim().trim_start_matches('(').trim_end_matches(')').split(',') {
        let mut kv = part.splitn(2, '=');
        let k = kv.next().unwrap_or("").trim();
        let val = atof(kv.next().unwrap_or(""));
        match k.to_ascii_uppercase().as_str() {
            "X" => v.x = val,
            "Y" => v.y = val,
            "Z" => v.z = val,
            _ => {}
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atof_matches_c() {
        assert_eq!(atof("- 200"), 0.0);
        assert_eq!(atof("0.150f"), 0.15);
        assert_eq!(atof("1.f"), 1.0);
        assert_eq!(atof(" -5000"), -5000.0);
        assert_eq!(atof("630.0"), 630.0);
    }
}
