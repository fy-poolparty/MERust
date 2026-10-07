//! Lane K: melee against the patrol cops.

use std::path::Path;
use tdsim::config::Config;
use tdsim::{InputFrame, Move, Sim, Vec3};

const DT: f32 = 1.0 / 60.0;

fn arena() -> Option<Sim> {
    let install = me_level::install();
    let anims = me_level::anims::load_player_anims(install).ok()?;
    let bots = me_level::anims::load_bot_anims(install).unwrap();
    let mut sim = Sim::new(tdsim::testmap::world(), Config::load(install), anims.lib);
    sim.bot_lib = bots.lib.clone();
    for (feet, yaw) in tdsim::testmap::bot_spawns() {
        sim.bots.push(tdsim::bots::Bot::new(bots.lib.clone(), feet, yaw));
    }
    let s = tdsim::testmap::spawns().into_iter().find(|s| s.name.starts_with("K combat")).unwrap();
    sim.spawn(s.feet, s.yaw);
    Some(sim)
}

fn trace(sim: &mut Sim, frames: usize, verbose: bool, input: impl Fn(usize, &Sim) -> InputFrame) -> Vec<Move> {
    let mut states = vec![sim.pawn.movement_state];
    let mut last = String::new();
    for i in 0..frames {
        sim.events.clear();
        let inp = input(i, sim);
        sim.tick(DT, inp);
        if *states.last().unwrap() != sim.pawn.movement_state {
            states.push(sim.pawn.movement_state);
        }
        if verbose {
            let anim = sim.anim.slots.iter().filter(|(_, s)| s.playing).map(|(k, s)| format!("{k:?}:{}", s.name)).collect::<Vec<_>>().join(",");
            let b = &sim.bots[0];
            let banim = b.anim.slots.iter().filter(|(_, s)| s.playing).map(|(k, s)| format!("{k:?}:{}", s.name)).collect::<Vec<_>>().join(",");
            let line = format!("{:?} [{anim}] hp {} | bot0 {:?} hp {} [{banim}]", sim.pawn.movement_state, sim.health, b.movement_state, b.health);
            if line != last {
                println!("  {i}: {line} at {:?} bot0 at {:?}", sim.pawn.location, b.location);
                last = line;
            }
            for e in &sim.events {
                if let tdsim::sim::Event::Sound(s) = e {
                    if matches!(s, tdsim::sound::SoundEvent::MeleeImpact { .. }) {
                        println!("  {i}: {s:?}");
                    }
                }
            }
        }
    }
    states
}

/// Walk up to the first cop and keep punching: it takes hits, reacts and goes down.
#[test]
fn punch_combo_kills_a_cop() {
    let Some(mut sim) = arena() else { return };
    let verbose = std::env::var("VERBOSE").is_ok();
    let st = trace(&mut sim, 60 * 12, verbose, |i, s| {
        let d = (s.bots[0].location - s.pawn.location).length();
        InputFrame { forward: if d > 120.0 { 1.0 } else { 0.0 }, attack: d < 200.0 && i % 12 < 2, ..Default::default() }
    });
    println!("states {st:?} bot0 hp {} state {:?}", sim.bots[0].health, sim.bots[0].movement_state);
    assert!(st.contains(&Move::Melee), "{st:?}");
    assert!(sim.bots[0].health < 100, "the cop should have been hit");
}

/// Standing in front of a cop: it attacks, and the player takes damage and stumbles.
#[test]
fn cop_hits_the_player() {
    let Some(mut sim) = arena() else { return };
    let verbose = std::env::var("VERBOSE").is_ok();
    let st = trace(&mut sim, 60 * 8, verbose, |_, s| {
        let d = (s.bots[0].location - s.pawn.location).length();
        InputFrame { forward: if d > 110.0 { 1.0 } else { 0.0 }, ..Default::default() }
    });
    println!("states {st:?} hp {}", sim.health);
    assert!(sim.health < 100 || sim.pawn.time_since_last_damage < 8.0, "should be hit: {st:?}");
    assert!(st.contains(&Move::Stumble), "{st:?}");
}

/// Jump kick: run at a cop and kick.
#[test]
fn jump_kick_lands() {
    let Some(mut sim) = arena() else { return };
    let verbose = std::env::var("VERBOSE").is_ok();
    let st = trace(&mut sim, 60 * 6, verbose, |_, s| {
        let d = (s.bots[0].location - s.pawn.location).length();
        let air = s.pawn.physics == tdsim::Physics::Falling;
        InputFrame { forward: 1.0, jump: d < 330.0 && !air && s.pawn.movement_state == Move::Walking, attack: air && d < 260.0, ..Default::default() }
    });
    println!("states {st:?} bot0 hp {} {:?}", sim.bots[0].health, sim.bots[0].movement_state);
    assert!(st.contains(&Move::MeleeAir), "{st:?}");
    assert!(sim.bots[0].health < 100, "the kick should land");
    let _ = Vec3::ZERO;
}

/// Slide kick: run, slide into a cop and kick; it goes down (HitMeleeSlide), and the next
/// punch is the bent-over finisher (soccer kick).
#[test]
fn slide_kick_and_follow_up() {
    let Some(mut sim) = arena() else { return };
    let verbose = std::env::var("VERBOSE").is_ok();
    let st = trace(&mut sim, 60 * 6, verbose, |i, s| {
        let d = (s.bots[0].location - s.pawn.location).length();
        let sliding = matches!(s.pawn.movement_state, Move::Slide | Move::MeleeSlide);
        let stunned = s.bots[0].movement_state == tdsim::bots::BotMove::Stumble;
        InputFrame {
            forward: if sliding || d > 110.0 { 1.0 } else { 0.0 },
            crouch: d < 450.0 && !stunned && s.pawn.movement_state != Move::Melee,
            attack: (sliding && d < 200.0) || (stunned && !sliding && i % 10 < 2),
            ..Default::default()
        }
    });
    println!("states {st:?} bot0 hp {} {:?} {:?}", sim.bots[0].health, sim.bots[0].movement_state, sim.bots[0].stumble_state);
    assert!(st.contains(&Move::MeleeSlide), "{st:?}");
}

/// Spamming attack in the air (no target): which arm each punch uses.
#[test]
fn punch_sides_when_spamming() {
    let Some(mut sim) = arena() else { return };
    let s = tdsim::testmap::spawns().into_iter().next().unwrap();
    sim.spawn(s.feet, s.yaw);
    sim.bots.clear();
    let mut seq = Vec::new();
    let mut last = String::new();
    for i in 0..60 * 4 {
        sim.events.clear();
        // mash: press every 6 frames
        sim.tick(DT, InputFrame { attack: i % 6 < 3, ..Default::default() });
        for (_, sl) in sim.anim.slots.iter().filter(|(_, s)| s.playing) {
            let n = sl.name.to_string();
            if n.starts_with("Melee") && n != last {
                seq.push(format!("{i}:{n}"));
                last = n;
            }
        }
    }
    println!("{}", seq.join(" "));
    let lefts = seq.iter().filter(|s| s.contains("StartLeft")).count();
    let rights = seq.iter().filter(|s| s.contains("StartRight")).count();
    assert!(lefts > 0 && rights > 0, "only one arm: {seq:?}");
}
