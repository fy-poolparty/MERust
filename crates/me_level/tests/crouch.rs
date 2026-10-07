//! Crouch / Slide with the real 1P animation set.

use std::path::Path;
use tdsim::collision::{Surface, WorldBuilder};
use tdsim::config::Config;
use tdsim::math::UeVec;
use tdsim::{InputFrame, Move, Sim, Vec3};

const DT: f32 = 1.0 / 60.0;

/// Floor plus a low roof from x = `x0` to `x1` with its underside at `under`.
fn roof_sim(x0: f32, x1: f32, under: f32) -> Option<Sim> {
    let install = me_level::install();
    let anims = me_level::anims::load_player_anims(install).ok()?;
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(5000.0, 5000.0, 50.0), 0.0, s);
    b.add_box(Vec3::new((x0 + x1) * 0.5, 0.0, under + 50.0), Vec3::new((x1 - x0) * 0.5, 400.0, 50.0), 0.0, s);
    let mut sim = Sim::new(b.build(), Config::load(install), anims.lib);
    sim.spawn(Vec3::new(0.0, 0.0, 0.0), 0);
    for _ in 0..30 {
        sim.tick(DT, InputFrame::default());
    }
    Some(sim)
}

#[test]
fn slide_under_bar_then_stand() {
    let Some(mut sim) = roof_sim(700.0, 760.0, 130.0) else {
        eprintln!("game install not found, skipping");
        return;
    };
    let mut log = vec![(sim.pawn.movement_state, 0.0f32)];
    let mut speeds = Vec::new();
    for i in 0..240 {
        let crouch = sim.pawn.location.x > 450.0 && sim.pawn.location.x < 780.0;
        sim.tick(DT, InputFrame { forward: 1.0, crouch, ..Default::default() });
        if log.last().unwrap().0 != sim.pawn.movement_state {
            log.push((sim.pawn.movement_state, sim.pawn.location.x));
        }
        if sim.pawn.movement_state == Move::Slide && i % 6 == 0 {
            speeds.push(sim.pawn.velocity.size_2d() as i32);
        }
    }
    println!("{log:?}\nslide speeds {speeds:?}\nend {:?}", sim.pawn.location);
    assert!(log.iter().any(|e| e.0 == Move::Slide), "{log:?}");
    assert!(sim.pawn.location.x > 800.0, "should get past the bar: {:?}", sim.pawn.location);
    assert_eq!(sim.pawn.movement_state, Move::Walking);
    assert!((sim.pawn.collision_height - 90.0).abs() < 0.01);
}

#[test]
fn stays_crouched_under_low_roof() {
    let Some(mut sim) = roof_sim(300.0, 900.0, 130.0) else {
        return;
    };
    // crouch-walk in (too slow to slide from standing)
    for _ in 0..10 {
        sim.tick(DT, InputFrame { crouch: true, ..Default::default() });
    }
    assert_eq!(sim.pawn.movement_state, Move::Crouch);
    for _ in 0..240 {
        sim.tick(DT, InputFrame { forward: 1.0, crouch: true, ..Default::default() });
        if sim.pawn.location.x > 500.0 {
            break;
        }
    }
    println!("under roof at {:?}", sim.pawn.location);
    assert!(sim.pawn.location.x > 500.0);
    // let go of crouch under the roof: must stay crouched
    for _ in 0..30 {
        sim.tick(DT, InputFrame::default());
    }
    assert_eq!(sim.pawn.movement_state, Move::Crouch, "no headroom to stand");
    assert!(!sim.pawn.can_uncrouch);
    // walk out without crouch held: stands once clear
    for _ in 0..300 {
        sim.tick(DT, InputFrame { forward: 1.0, ..Default::default() });
        if sim.pawn.movement_state == Move::Walking {
            break;
        }
    }
    println!("stood up at {:?}", sim.pawn.location);
    assert_eq!(sim.pawn.movement_state, Move::Walking);
    assert!(sim.pawn.location.x > 900.0 - 40.0);
}
