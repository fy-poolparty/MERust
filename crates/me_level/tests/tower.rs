//! Lane H: ladder up the tower, exit at the top, drops into the landing bags / onto concrete.

use std::path::Path;
use tdsim::config::Config;
use tdsim::{InputFrame, Move, Sim, Vec3};
use tdsim::math::UeVec;

const DT: f32 = 1.0 / 60.0;

fn tower_sim() -> Option<Sim> {
    let install = me_level::install();
    let anims = me_level::anims::load_player_anims(install).ok()?;
    let props = match me_level::load_props(install, &tdsim::testmap::props(), 64) { Ok(p) => p, Err(e) => panic!("props: {e}") };
    let mut sim = Sim::new(me_level::testmap_world(&props), Config::load(install), anims.lib);
    sim.ladders = tdsim::testmap::ladders();
    sim.swings = tdsim::testmap::swings();
    sim.ziplines = tdsim::testmap::ziplines();
    sim.balances = tdsim::testmap::balances();
    Some(sim)
}

fn log_states(sim: &mut Sim, frames: usize, input: impl Fn(usize, &Sim) -> InputFrame) -> Vec<(Move, usize)> {
    let mut v = vec![(sim.pawn.movement_state, 0)];
    for i in 0..frames {
        let inp = input(i, sim);
        sim.tick(DT, inp);
        if v.last().unwrap().0 != sim.pawn.movement_state {
            v.push((sim.pawn.movement_state, i));
        }
    }
    v
}

#[test]
fn climb_tower_ladder() {
    let Some(mut sim) = tower_sim() else { return };
    let l = &sim.ladders[0];
    println!("ladder: {} locations first {:?} last {:?} pawn at {:?}", l.pawn_ladder_locations.len(), l.ladder_location(0), l.ladder_location(l.last_step()), l.pawn_ladder_locations.last());
    sim.spawn(Vec3::new(600.0, 8400.0, 0.0), 0);
    let st = log_states(&mut sim, 60 * 40, |_, s| InputFrame { forward: if s.pawn.movement_state == Move::Walking && s.pawn.location.z > 1000.0 { 0.0 } else { 1.0 }, ..Default::default() });
    println!("{st:?}\nend {:?}", sim.pawn.location);
    assert!(st.iter().any(|s| s.0 == Move::Climb), "should climb: {st:?}");
    assert_eq!(sim.pawn.movement_state, Move::Walking, "{st:?}");
    let feet = sim.pawn.location.z - sim.pawn.collision_height;
    assert!((feet - tdsim::testmap::TOWER_TOP).abs() < 5.0, "should stand on the roof, feet at {feet}");
}

/// Off the roof's far edge: into the bags (soft landing), from the tower top.
#[test]
fn drop_into_landing_bags() {
    let Some(mut sim) = tower_sim() else { return };
    let top = tdsim::testmap::TOWER_TOP;
    for (turn, x0) in [(false, 1300.0f32), (true, 1300.0)] {
        sim.spawn(Vec3::new(x0, 8400.0, top), 0);
        let st = log_states(&mut sim, 60 * 8, |i, s| InputFrame {
            forward: if s.pawn.location.x < 1505.0 && s.pawn.physics == tdsim::Physics::Walking { 1.0 } else { 0.0 },
            turn: turn && i % 20 == 0 && s.pawn.movement_state == Move::Falling,
            ..Default::default()
        });
        println!("turn {turn}: {st:?} end {:?} died {}", sim.pawn.location, sim.events.iter().any(|e| matches!(e, tdsim::sim::Event::Died)));
        assert!(st.iter().any(|s| s.0 == Move::SoftLanding), "should soft land: {st:?}");
        assert!(!sim.pawn.dying);
        sim.events.clear();
    }
}

/// Off the roof's side, onto concrete: deadly.
#[test]
fn fall_off_tower_dies() {
    let Some(mut sim) = tower_sim() else { return };
    let top = tdsim::testmap::TOWER_TOP;
    sim.spawn(Vec3::new(1250.0, 8550.0, top), 16384);
    let st = log_states(&mut sim, 60 * 6, |_, s| InputFrame { forward: if s.pawn.physics == tdsim::Physics::Walking && s.pawn.location.y < 8655.0 { 1.0 } else { 0.0 }, ..Default::default() });
    println!("{st:?} died {}", sim.events.iter().any(|e| matches!(e, tdsim::sim::Event::Died)));
    assert!(sim.events.iter().any(|e| matches!(e, tdsim::sim::Event::Died)), "{st:?}");
}

#[test]
#[ignore]
fn debug_soft_target() {
    let Some(mut sim) = tower_sim() else { return };
    let top = tdsim::testmap::TOWER_TOP;
    sim.spawn(Vec3::new(1300.0, 8400.0, top), 0);
    for i in 0..400 {
        let s = &sim;
        let fwd = if s.pawn.location.x < 1505.0 && s.pawn.physics == tdsim::Physics::Walking { 1.0 } else { 0.0 };
        sim.tick(DT, InputFrame { forward: fwd, ..Default::default() });
        let p = &sim.pawn;
        if p.physics == tdsim::Physics::Falling && i % 4 == 0 {
            let accel = Vec3::new(p.acceleration.x, p.acceleration.y, p.gravity_z());
            let mut feet = p.location;
            feet.z -= p.collision_height;
            let end = feet + p.velocity * 2.0 + accel * 4.0 * 0.5;
            let h = sim.world.line_check(end, feet, Vec3::ZERO);
            println!("{i} {:?} loc ({:.0},{:.0}) v ({:.0},{:.0}) hit {} at ({:.0},{:.0},{:.0}) n.z {:.2} soft {}", p.movement_state, p.location.x, p.location.z, p.velocity.x, p.velocity.z,
                h.hit, h.location.x, h.location.y, h.location.z, h.normal.z, h.surface.soft_landing);
        }
    }
}

#[test]
#[ignore]
fn debug_bag_ray() {
    let Some(sim) = tower_sim() else { return };
    for (x, y) in [(1700.0f32, 8318.0f32), (1700.0, 8400.0), (1934.0, 8318.0), (1650.0, 8300.0)] {
        let h = sim.world.line_check(Vec3::new(x, y, -50.0), Vec3::new(x, y, 1000.0), Vec3::ZERO);
        let u = sim.world.line_check(Vec3::new(x, y, 1000.0), Vec3::new(x, y, 60.0), Vec3::ZERO);
        println!("down at ({x},{y}): hit {} z {:.1} n {:?} soft {} | up from inside: hit {} z {:.1} n {:?}", h.hit, h.location.z, h.normal, h.surface.soft_landing, u.hit, u.location.z, u.normal);
    }
}

#[test]
#[ignore]
fn debug_ladder_top() {
    let Some(mut sim) = tower_sim() else { return };
    sim.spawn(Vec3::new(600.0, 8400.0, 0.0), 0);
    for i in 0..1200 {
        let s = &sim;
        let fwd = if s.pawn.movement_state == Move::Walking && s.pawn.location.z > 1000.0 { 0.0 } else { 1.0 };
        sim.tick(DT, InputFrame { forward: fwd, ..Default::default() });
        let p = &sim.pawn;
        if i > 1040 && i < 1100 && i % 3 == 0 {
            println!("{i} {:?} {:?} loc ({:.0},{:.0}) v ({:.0},{:.0}) slot {:?} rm {} cw {} climb {:?}", p.movement_state, p.physics, p.location.x, p.location.z, p.velocity.x, p.velocity.z,
                sim.anim.current(tdsim::pawn::Slot::FullBody).map(|s| (s.name.clone(), s.position)), p.is_using_root_motion, p.collide_world, sim.moves.climb.climb_state);
        }
    }
}

#[test]
#[ignore]
fn debug_180_cushion() {
    let Some(mut sim) = tower_sim() else { return };
    let top = tdsim::testmap::TOWER_TOP;
    sim.spawn(Vec3::new(1050.0, 8400.0, top), 0);
    let mut jumped = false;
    let mut prev = sim.pawn.movement_state;
    for i in 0..600 {
        let j = !jumped && sim.pawn.location.x > 1440.0;
        jumped |= j;
        let t = jumped && sim.pawn.movement_state == Move::Jump && sim.moves.base(Move::Jump).move_active_time > 0.1;
        sim.tick(DT, InputFrame { forward: if jumped { 0.0 } else { 1.0 }, jump: j, turn: t, ..Default::default() });
        let p = &sim.pawn;
        if p.movement_state != prev {
            let dot = p.rotation.vector().safe_normal().dot(p.velocity.safe_normal());
            println!("{i} {:?} old {:?} yaw {} pc {} v ({:.0},{:.0}) dot {dot:.2} ws {:?} slot {:?}", p.movement_state, p.old_movement_state, p.rotation.yaw, sim.pc.rotation.yaw, p.velocity.size_2d(), p.velocity.z, p.current_walking_state, sim.anim.current(tdsim::pawn::Slot::FullBody).map(|s| s.name.clone()));
            prev = p.movement_state;
        }
    }
}

/// Up the ladder a bit, then hold back: slide down and step off at the bottom.
#[test]
fn ladder_down_and_off() {
    let Some(mut sim) = tower_sim() else { return };
    sim.spawn(Vec3::new(600.0, 8400.0, 0.0), 0);
    let st = log_states(&mut sim, 60 * 12, |i, s| {
        let up = s.pawn.movement_state != Move::Climb || i < 300;
        InputFrame { forward: if up { 1.0 } else { -1.0 }, ..Default::default() }
    });
    println!("{st:?} end {:?}", sim.pawn.location);
    let after: Vec<_> = st.iter().skip_while(|s| s.0 != Move::Climb).collect();
    assert!(after.iter().any(|s| s.0 == Move::Walking), "should step off the ladder: {st:?}");
}

#[test]
#[ignore]
fn debug_ladder_down() {
    let Some(mut sim) = tower_sim() else { return };
    sim.spawn(Vec3::new(600.0, 8400.0, 0.0), 0);
    for i in 0..720 {
        let up = sim.pawn.movement_state != Move::Climb || i < 300;
        sim.tick(DT, InputFrame { forward: if up { 1.0 } else { -1.0 }, ..Default::default() });
        let p = &sim.pawn;
        if i >= 298 && i % 6 == 0 {
            let b = sim.moves.base(Move::Climb);
            println!("{i} {:?} {:?} z {:.1} vz {:.0} fast {} precise {} target {:.1} playing {} hint {:?} max {} cw {}", p.movement_state, p.physics, p.location.z, p.velocity.z, p.climb_down_fast,
                b.use_precise_location, b.precise_location.z, sim.moves.climb.playing_animation, p.move_action_hint, p.move_action_max, p.collide_world);
        }
    }
}

/// The low platform: up its ladder, run off its -X edge with a 180 in the air into the bags:
/// steep enough falls go forward, this one should land on the back (LayOnGround).
#[test]
fn low_platform_180_into_bags() {
    let Some(mut sim) = tower_sim() else { return };
    sim.spawn(Vec3::new(3550.0, 8400.0, 0.0), 32768);
    let mut jumped = false;
    let mut st = vec![(sim.pawn.movement_state, 0usize)];
    for i in 0..60 * 20 {
        let on_top = sim.pawn.location.z > 600.0 && sim.pawn.physics == tdsim::Physics::Walking;
        let j = !jumped && on_top && sim.pawn.location.x < 3010.0;
        jumped |= j;
        let t = jumped && sim.pawn.movement_state == Move::Jump && sim.moves.base(Move::Jump).move_active_time > 0.1;
        sim.tick(DT, InputFrame { forward: if jumped { 0.0 } else { 1.0 }, jump: j, turn: t, ..Default::default() });
        if st.last().unwrap().0 != sim.pawn.movement_state {
            st.push((sim.pawn.movement_state, i));
        }
    }
    println!("{st:?} end {:?}", sim.pawn.location);
    assert!(st.iter().any(|s| s.0 == Move::Climb), "{st:?}");
    assert!(st.iter().any(|s| s.0 == Move::SoftLanding), "{st:?}");
    assert!(st.iter().any(|s| s.0 == Move::LayOnGround), "should land on the back: {st:?}");
}

/// Lane H pipe, beside the tower's ladder: climb it to the top and back down; and from the
/// ladder, jump across onto it (GrabTransfer's ladder path, A + jump). The other way round
/// doesn't reach: a pipe climber hangs 159 off the wall (the tutorial's pipes stand 120 off
/// it on elbows) and the ladder's volume only reaches 76 out.
#[test]
fn pipe_climb_and_transfer_from_ladder() {
    let Some(mut sim) = tower_sim() else { return };
    let pipe = sim.ladders.iter().position(|l| l.ladder_type == tdsim::ladder::LadderType::Pipe).expect("course pipe");
    let last = sim.ladders[pipe].ladder_location(sim.ladders[pipe].last_step());
    sim.spawn(Vec3::new(600.0, 8220.0, 0.0), 0);
    let st = log_states(&mut sim, 60 * 25, |_, _| InputFrame { forward: 1.0, ..Default::default() });
    println!("up the pipe: {st:?} at {:?}", sim.pawn.location);
    assert_eq!(sim.pawn.movement_state, Move::Climb, "{st:?}");
    assert!((sim.pawn.location.z - last.z).abs() < 2.0, "should reach the pipe's top step {:?}: {:?}", last, sim.pawn.location);

    // up the ladder to the pipe's height, then A + jump across
    sim.spawn(Vec3::new(600.0, 8400.0, 0.0), 0);
    let mut jumped = false;
    let st = log_states(&mut sim, 60 * 30, |i, s| {
        let on_ladder = s.pawn.movement_state == Move::Climb && s.moves.climb.ladder == Some(0);
        if on_ladder && s.pawn.location.z > 800.0 && !jumped {
            InputFrame { strafe: -1.0, jump: i % 20 == 0, ..Default::default() }
        } else if s.moves.climb.ladder == Some(pipe) {
            InputFrame::default()
        } else {
            InputFrame { forward: 1.0, ..Default::default() }
        }
    });
    jumped = true;
    let _ = jumped;
    println!("ladder -> pipe: {st:?} at {:?}", sim.pawn.location);
    assert!(st.iter().any(|s| s.0 == Move::GrabTransfer), "{st:?}");
    assert_eq!(sim.moves.climb.ladder, Some(pipe), "should hang on the pipe: {st:?}");
    assert_eq!(sim.pawn.movement_state, Move::Climb, "{st:?}");
}
#[test]
#[ignore]
fn debug_ladder_to_pipe() {
    let Some(mut sim) = tower_sim() else { return };
    sim.spawn(Vec3::new(600.0, 8400.0, 0.0), 0);
    for _ in 0..60 * 12 {
        let inp = if sim.pawn.movement_state == Move::Climb && sim.pawn.location.z > 800.0 { InputFrame::default() } else { InputFrame { forward: 1.0, ..Default::default() } };
        sim.tick(DT, inp);
    }
    println!("at {:?} state {:?} ladder {:?}", sim.pawn.location, sim.pawn.movement_state, sim.moves.climb.ladder);
    for i in 0..40 {
        sim.tick(DT, InputFrame { strafe: -1.0, jump: i == 20, ..Default::default() });
        if i == 19 || i == 20 {
            let ok = sim.grab_transfer_can_do_move(Move::GrabTransfer);
            println!("i {i}: hint {:?} max {} can_do {ok} fit {} target {:?} ladder {:?} state {:?}", sim.pawn.move_action_hint, sim.pawn.move_action_max, sim.moves.grab_transfer.fit_for_grab, sim.moves.grab_transfer.transfer_location, sim.moves.grab_transfer.transfer_ladder, sim.pawn.movement_state);
        }
    }
    println!("end state {:?}", sim.pawn.movement_state);
}

/// Jumps off the pipe / ladder with each strafe direction: states, custom anims, sounds.
#[test]
#[ignore]
fn debug_jump_from_climb() {
    for (start_y, strafe) in [(8220.0, 1.0), (8220.0, -1.0), (8400.0, -1.0), (8400.0, 1.0)] {
        let Some(mut sim) = tower_sim() else { return };
        sim.spawn(Vec3::new(600.0, start_y, 0.0), 0);
        for _ in 0..60 * 12 {
            let inp = if sim.pawn.movement_state == Move::Climb && sim.pawn.location.z > 800.0 { InputFrame::default() } else { InputFrame { forward: 1.0, ..Default::default() } };
            sim.tick(DT, inp);
        }
        println!("== y {start_y} strafe {strafe}: at {:?} {:?} ladder {:?}", sim.pawn.location, sim.pawn.movement_state, sim.moves.climb.ladder);
        let mut last = String::new();
        for i in 0..120 {
            sim.events.clear();
            sim.tick(DT, InputFrame { strafe, jump: i == 10, ..Default::default() });
            let anim = sim.anim.slots.iter().filter(|(_, s)| s.playing).map(|(k, s)| format!("{k:?}:{}", s.name)).collect::<Vec<_>>().join(",");
            let line = format!("{:?} amstate {:?} anim [{anim}]", sim.pawn.movement_state, sim.pawn.animation_movement_state);
            if line != last { println!("  {i}: {line} at {:?}", sim.pawn.location); last = line; }
            for e in &sim.events { if let tdsim::sim::Event::Sound(s) = e { println!("  {i}: sound {s:?}"); } }
        }
    }
}

/// Hand positions on the pipe with the view turned away (TdSkelControlAim1p).
#[test]
#[ignore]
fn debug_arm_aim_on_pipe() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    let Some(mut sim) = tower_sim() else { return };
    let mut pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    sim.spawn(Vec3::new(600.0, 8220.0, 0.0), 0);
    for _ in 0..60 * 8 {
        let inp = if sim.pawn.movement_state == Move::Climb && sim.pawn.location.z > 500.0 { InputFrame::default() } else { InputFrame { forward: 1.0, ..Default::default() } };
        sim.tick(DT, inp);
        pe.update(&mut sim, DT);
    }
    let yaw: i32 = std::env::var("YAW").ok().and_then(|v| v.parse().ok()).unwrap_or(26000);
    let pitch: i32 = std::env::var("PITCH").ok().and_then(|v| v.parse().ok()).unwrap_or(-6000);
    for f in 0..90 {
        sim.pc.rotation.yaw = sim.pawn.rotation.yaw + yaw * f.min(30) / 30;
        sim.pc.rotation.pitch = pitch;
        sim.tick(DT, InputFrame::default());
        pe.update(&mut sim, DT);
        if f % 15 == 14 {
            let m = me_level::pose::mesh_to_world(&sim, a.lib.mesh_rot, a.upper.origin);
            let g = pe.globals();
            let at = |b: &str| { let t = (m * g[pe.bone_index(b).unwrap()]).w_axis; Vec3::new(t.x * 100.0, t.z * 100.0, t.y * 100.0) };
            let e = at("EyeJoint");
            let r = at("RightHand") - e;
            let l = at("LeftHand") - e;
            // in the view frame: forward, right, up
            let (vx, vy, vz) = sim.pc.rotation.axes();
            println!("f {f} state {:?} aim {:?} R view ({:.0} {:.0} {:.0}) L view ({:.0} {:.0} {:.0}) pawn yaw {} ctrl yaw {}", sim.pawn.movement_state, sim.aim_mode(true),
                r.dot(vx), r.dot(vy), r.dot(vz), l.dot(vx), l.dot(vy), l.dot(vz), sim.pawn.rotation.yaw, sim.pc.rotation.yaw);
        }
    }
}

/// Turned towards the other climbable, jump (GrabJump): states, anims, sounds.
#[test]
#[ignore]
fn debug_turned_jump_from_climb() {
    let yaw: i32 = std::env::var("YAW").ok().and_then(|v| v.parse().ok()).unwrap_or(-24000);
    let start_y: f32 = std::env::var("SY").ok().and_then(|v| v.parse().ok()).unwrap_or(8220.0);
    let Some(mut sim) = tower_sim() else { return };
    sim.spawn(Vec3::new(600.0, start_y, 0.0), 0);
    for _ in 0..60 * 12 {
        let inp = if sim.pawn.movement_state == Move::Climb && sim.pawn.location.z > 800.0 { InputFrame::default() } else { InputFrame { forward: 1.0, ..Default::default() } };
        sim.tick(DT, inp);
    }
    println!("at {:?} {:?} ladder {:?}", sim.pawn.location, sim.pawn.movement_state, sim.moves.climb.ladder);
    let mut last = String::new();
    for i in 0..200 {
        if i < 30 { sim.pc.rotation.yaw = sim.pawn.rotation.yaw + yaw * (i + 1) / 30; }
        sim.events.clear();
        sim.tick(DT, InputFrame { jump: i == 40, ..Default::default() });
        let anim = sim.anim.slots.iter().filter(|(_, s)| s.playing).map(|(k, s)| format!("{k:?}:{}", s.name)).collect::<Vec<_>>().join(",");
        let line = format!("{:?} amstate {:?} anim [{anim}] ladder {:?}", sim.pawn.movement_state, sim.pawn.animation_movement_state, sim.moves.climb.ladder);
        if line != last { println!("  {i}: {line} at {:?}", sim.pawn.location); last = line; }
        for e in &sim.events { if let tdsim::sim::Event::Sound(s) = e { println!("  {i}: sound {s:?}"); } }
    }
}

/// Turned towards the ladder on the pipe, jump: GrabTransfer onto the ladder, and IntoClimb plays
/// the side hang-start (OldMovementState GrabTransfer) instead of nothing.
#[test]
fn pipe_to_ladder_transfer_plays_hang_start() {
    let Some(mut sim) = tower_sim() else { return };
    sim.spawn(Vec3::new(600.0, 8220.0, 0.0), 0);
    for _ in 0..60 * 12 {
        let inp = if sim.pawn.movement_state == Move::Climb && sim.pawn.location.z > 800.0 { InputFrame::default() } else { InputFrame { forward: 1.0, ..Default::default() } };
        sim.tick(DT, inp);
    }
    let mut anims = Vec::new();
    for i in 0..200 {
        if i < 30 {
            sim.pc.rotation.yaw = sim.pawn.rotation.yaw + 12000 * (i + 1) / 30;
        }
        sim.tick(DT, InputFrame { jump: i == 40, ..Default::default() });
        if sim.pawn.movement_state == Move::IntoClimb {
            if let Some(a) = sim.anim.current(tdsim::pawn::Slot::FullBody) {
                anims.push(a.name.clone());
            }
        }
    }
    assert_eq!(sim.moves.climb.ladder, Some(0), "should be on the ladder");
    assert!(anims.iter().any(|a| a.starts_with("LadderClimbHangStart")), "{anims:?}");
}
