//! SpeedVault / VaultOver with the real 1P animation set.

use std::path::Path;
use tdsim::collision::{Surface, WorldBuilder};
use tdsim::config::Config;
use tdsim::{InputFrame, Move, Sim, Vec3};

const DT: f32 = 1.0 / 60.0;

/// Floor plus one obstacle starting at x = 600: `depth` long, `height` tall.
fn obstacle_sim(height: f32, depth: f32) -> Option<Sim> {
    let install = me_level::install();
    let anims = me_level::anims::load_player_anims(install).ok()?;
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(5000.0, 5000.0, 50.0), 0.0, s);
    b.add_box(Vec3::new(600.0 + depth * 0.5, 0.0, height * 0.5), Vec3::new(depth * 0.5, 400.0, height * 0.5), 0.0, s);
    let mut sim = Sim::new(b.build(), Config::load(install), anims.lib);
    sim.spawn(Vec3::new(0.0, 0.0, 0.0), 0);
    for _ in 0..30 {
        sim.tick(DT, InputFrame::default());
    }
    Some(sim)
}

/// Run at the obstacle holding forward and tap jump once within `jump_dist` of it.
fn run_forward(sim: &mut Sim, frames: usize) -> Vec<(Move, f32, f32)> {
    let mut log = vec![(sim.pawn.movement_state, sim.pawn.location.x, sim.pawn.location.z)];
    let mut jumped = 0;
    for _ in 0..frames {
        let jump = if sim.pawn.location.x > 480.0 && jumped < 3 {
            jumped += 1;
            true
        } else {
            false
        };
        sim.tick(DT, InputFrame { forward: 1.0, jump, ..Default::default() });
        if log.last().unwrap().0 != sim.pawn.movement_state {
            log.push((sim.pawn.movement_state, sim.pawn.location.x, sim.pawn.location.z));
        }
    }
    log
}

#[test]
fn vault_over_railing() {
    let Some(mut sim) = obstacle_sim(100.0, 20.0) else {
        eprintln!("game install not found, skipping");
        return;
    };
    let log = run_forward(&mut sim, 180);
    println!("{log:?}");
    println!("end loc {:?} type {}", sim.pawn.location, sim.moves.vault.active_vault_type);
    assert!(log.iter().any(|e| e.0 == Move::VaultOver), "{log:?}");
    assert_eq!(sim.moves.vault.active_vault_type, 3, "VaultOver type");
    assert!(sim.pawn.location.x > 700.0, "should be past the railing: {:?}", sim.pawn.location);
    // walking hovers ~3.15uu: FloorDist targets 2.15 on a trace time pulled back by 1uu
    let feet = sim.pawn.location.z - sim.pawn.collision_height;
    assert!(feet > 0.0 && feet < 3.5, "back on the floor, feet {feet}");
    assert_eq!(sim.pawn.movement_state, Move::Walking);
}

#[test]
fn vault_onto_box() {
    let Some(mut sim) = obstacle_sim(100.0, 600.0) else {
        return;
    };
    let log = run_forward(&mut sim, 180);
    println!("{log:?}");
    println!("end loc {:?} type {}", sim.pawn.location, sim.moves.vault.active_vault_type);
    assert!(log.iter().any(|e| e.0 == Move::VaultOver), "{log:?}");
    assert_eq!(sim.moves.vault.active_vault_type, 2, "VaultOnto type");
    let i = log.iter().position(|e| e.0 == Move::VaultOver).unwrap();
    let after = log[i + 1];
    assert_eq!(after.0, Move::Walking, "{log:?}");
    let feet = after.2 - sim.pawn.collision_height;
    assert!(feet > 100.0 && feet < 103.5, "standing on the box after the vault, feet {feet}");
}
