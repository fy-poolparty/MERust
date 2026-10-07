//! Turns, dodge jump, coil, skill roll and the wall-climb 180 jump with the real animations.

use std::path::Path;
use tdsim::collision::{Surface, WorldBuilder};
use tdsim::config::Config;
use tdsim::{InputFrame, Move, Sim, Vec3};
use tdsim::math::UeVec;

const DT: f32 = 1.0 / 60.0;

/// Floor, plus optional boxes (center, half extents).
fn sim_with(boxes: &[(Vec3, Vec3)], spawn: Vec3) -> Option<Sim> {
    let install = me_level::install();
    let anims = me_level::anims::load_player_anims(install).ok()?;
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(8000.0, 8000.0, 50.0), 0.0, s);
    for (c, h) in boxes {
        b.add_box(*c, *h, 0.0, s);
    }
    let mut sim = Sim::new(b.build(), Config::load(install), anims.lib);
    sim.spawn(spawn, 0);
    for _ in 0..30 {
        sim.tick(DT, InputFrame::default());
    }
    Some(sim)
}

fn states(sim: &mut Sim, frames: usize, input: impl Fn(usize, &Sim) -> InputFrame) -> Vec<Move> {
    let mut v = vec![sim.pawn.movement_state];
    for i in 0..frames {
        let inp = input(i, sim);
        sim.tick(DT, inp);
        if *v.last().unwrap() != sim.pawn.movement_state {
            v.push(sim.pawn.movement_state);
        }
    }
    v
}

#[test]
fn turn_180_standing() {
    let Some(mut sim) = sim_with(&[], Vec3::ZERO) else { return };
    let yaw0 = sim.pawn.rotation.yaw;
    let st = states(&mut sim, 90, |i, _| InputFrame { turn: i == 0, ..Default::default() });
    let d = sim.pawn.rotation.yaw - yaw0;
    let dc = sim.pc.rotation.yaw - yaw0;
    println!("{st:?} pawn turned {d} controller turned {dc}");
    assert!(st.contains(&Move::Turn180), "{st:?}");
    assert_eq!(sim.pawn.movement_state, Move::Walking);
    assert!((d.rem_euclid(65536) - 32768).abs() < 1200, "pawn yaw delta {d}");
    assert!((dc.rem_euclid(65536) - 32768).abs() < 1200, "view yaw delta {dc}");
}

#[test]
fn dodge_jump_sideways() {
    let Some(mut sim) = sim_with(&[], Vec3::ZERO) else { return };
    let st = states(&mut sim, 60, |i, _| InputFrame { strafe: 1.0, jump: i == 5, ..Default::default() });
    println!("{st:?} loc {:?}", sim.pawn.location);
    assert!(st.contains(&Move::DodgeJump), "{st:?}");
    assert!(sim.pawn.location.y > 150.0, "should dodge right: {:?}", sim.pawn.location);
}

#[test]
fn coil_in_jump() {
    let Some(mut sim) = sim_with(&[], Vec3::ZERO) else { return };
    // run up, jump, crouch in the air
    let st = states(&mut sim, 140, |i, _| InputFrame { forward: 1.0, jump: i == 60, crouch: (66..80).contains(&i), ..Default::default() });
    println!("{st:?}");
    assert!(st.contains(&Move::Coil), "{st:?}");
}

#[test]
fn skill_roll_from_drop() {
    // start on a 400 high block, run off it, tap crouch just before landing
    let Some(mut sim) = sim_with(&[(Vec3::new(0.0, 0.0, 200.0), Vec3::new(300.0, 300.0, 200.0))], Vec3::new(0.0, 0.0, 400.0)) else { return };
    let mut landed_at = None;
    let st = states(&mut sim, 300, |_, s| {
        let near = s.pawn.physics == tdsim::Physics::Falling && s.pawn.location.z - s.pawn.collision_height < 60.0;
        InputFrame { forward: 1.0, crouch: near, ..Default::default() }
    });
    for (i, m) in st.iter().enumerate() {
        if *m == Move::Landing || *m == Move::SkillRoll {
            landed_at.get_or_insert(i);
        }
    }
    println!("{st:?} end {:?}", sim.pawn.location);
    assert!(st.contains(&Move::SkillRoll), "{st:?}");
    assert_eq!(sim.pawn.movement_state, Move::Walking);
}

#[test]
fn wallclimb_180_jump() {
    // tall wall at x = 400
    let Some(mut sim) = sim_with(&[(Vec3::new(700.0, 0.0, 500.0), Vec3::new(300.0, 500.0, 500.0))], Vec3::ZERO) else { return };
    let st = states(&mut sim, 240, |i, s| {
        let ms = s.pawn.movement_state;
        let t = s.moves.base(ms).move_active_time;
        InputFrame {
            forward: if ms == Move::WallClimb180TurnJump || i > 150 { 0.0 } else { 1.0 },
            jump: (40..44).contains(&i) || (ms == Move::WallClimb180TurnJump && t > 0.1),
            turn: ms == Move::WallClimbing && t > 0.25,
            ..Default::default()
        }
    });
    println!("{st:?} loc {:?} vel {:?} yaw {}", sim.pawn.location, sim.pawn.velocity, sim.pawn.rotation.yaw);
    assert!(st.contains(&Move::WallClimbing), "{st:?}");
    assert!(st.contains(&Move::WallClimb180TurnJump), "{st:?}");
    assert!(sim.pawn.location.x < 250.0, "jumped away from the wall: {:?}", sim.pawn.location);
    assert!((sim.pawn.rotation.yaw.rem_euclid(65536) - 32768).abs() < 1500, "facing away, yaw {}", sim.pawn.rotation.yaw);
}

#[test]
fn springboard_off_step() {
    // 64 high step (x 600..712) in front of a 140 high block (x 712..1100)
    let Some(mut sim) = sim_with(
        &[
            (Vec3::new(656.0, 0.0, 32.0), Vec3::new(56.0, 300.0, 32.0)),
            (Vec3::new(906.0, 0.0, 70.0), Vec3::new(194.0, 300.0, 70.0)),
        ],
        Vec3::ZERO,
    ) else {
        return;
    };
    let mut jumped = false;
    let st = states(&mut sim, 200, |_, s| {
        let j = !jumped && s.pawn.location.x > 470.0;
        InputFrame { forward: 1.0, jump: j, ..Default::default() }
    });
    let _ = &mut jumped;
    println!("{st:?} end {:?}", sim.pawn.location);
    assert!(st.contains(&Move::SpringBoarding), "{st:?}");
}

/// Jump off a high platform, 180 in the air, fall uncontrolled and land: log the landing inputs.
#[test]
#[ignore]
fn turn_180_in_air_long_fall() {
    let h: f32 = std::env::var("H").ok().and_then(|v| v.parse().ok()).unwrap_or(1200.0);
    let Some(mut sim) = sim_with(&[(Vec3::new(-500.0, 0.0, h - 50.0), Vec3::new(500.0, 500.0, 50.0))], Vec3::new(-600.0, 0.0, h + 100.0)) else { return };
    let mut jumped = false;
    let mut prev = sim.pawn.movement_state;
    for i in 0..900 {
        let j = !jumped && sim.pawn.location.x > -60.0;
        jumped |= j;
        let t = jumped && sim.pawn.movement_state == Move::Jump && sim.moves.base(Move::Jump).move_active_time > 0.15;
        sim.tick(1.0 / 60.0, InputFrame { forward: if jumped { 0.0 } else { 1.0 }, jump: j, turn: t, ..Default::default() });
        let p = &sim.pawn;
        if p.movement_state != prev || (jumped && i % 20 == 0) || (85..182).contains(&i) {
            let dot = p.rotation.vector().safe_normal().dot(p.velocity.safe_normal());
            println!("{i} {:?} old {:?} yaw {} pc {:?} vel ({:.0},{:.0},{:.0}) dot {dot:.2} ws {:?} slot {:?}", p.movement_state, p.old_movement_state, p.rotation.yaw, (sim.pc.rotation.pitch as i16, sim.pc.rotation.yaw as i16, sim.moves.base(Move::Turn180InAir).look_at_target_location),
                p.velocity.x, p.velocity.y, p.velocity.z, p.current_walking_state, sim.anim.current(tdsim::pawn::Slot::FullBody).map(|s| s.name.clone()));
            prev = p.movement_state;
        }
    }
}
