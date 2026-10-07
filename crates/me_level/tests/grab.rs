//! Grab family with the real 1P animation set: jump at a wall, hang, shimmy, pull up.

use std::path::Path;
use tdsim::collision::{Surface, WorldBuilder};
use tdsim::config::Config;
use tdsim::{InputFrame, Move, Sim, Vec3};

const DT: f32 = 1.0 / 60.0;

fn wall_sim(top: f32) -> Option<Sim> {
    let install = me_level::install();
    let anims = me_level::anims::load_player_anims(install).ok()?;
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(5000.0, 5000.0, 50.0), 0.0, s);
    // wall face at x = 400, 1000 wide, 600 deep, top at `top`
    b.add_box(Vec3::new(700.0, 0.0, top * 0.5), Vec3::new(300.0, 500.0, top * 0.5), 0.0, s);
    let mut sim = Sim::new(b.build(), Config::load(install), anims.lib);
    sim.spawn(Vec3::new(0.0, 0.0, 0.0), 0);
    for _ in 0..30 {
        sim.tick(DT, InputFrame::default());
    }
    Some(sim)
}

fn run_until(sim: &mut Sim, frames: usize, input: impl Fn(usize) -> InputFrame, stop: impl Fn(&Sim) -> bool) -> Vec<Move> {
    let mut states = vec![sim.pawn.movement_state];
    for i in 0..frames {
        sim.tick(DT, input(i));
        if *states.last().unwrap() != sim.pawn.movement_state {
            states.push(sim.pawn.movement_state);
        }
        if stop(sim) {
            break;
        }
    }
    states
}

#[test]
fn jump_grab_and_pull_up() {
    let Some(mut sim) = wall_sim(300.0) else {
        eprintln!("game install not found, skipping");
        return;
    };
    // run at the wall and jump shortly before it
    let states = run_until(
        &mut sim,
        600,
        |i| InputFrame { forward: if i < 40 { 1.0 } else { 0.0 }, jump: (40..44).contains(&i), ..Default::default() },
        |s| s.pawn.movement_state == Move::IntoGrab,
    );
    let mut states = states;
    states.extend(run_until(&mut sim, 120, |_| InputFrame::default(), |s| s.pawn.movement_state == Move::Grabbing));
    println!("approach: {states:?} loc={:?}", sim.pawn.location);
    assert_eq!(sim.pawn.movement_state, Move::Grabbing, "should hang: {states:?}");
    let hang = sim.pawn.location;
    println!("hanging at {hang:?} ledge={:?}", sim.pawn.move_ledge_location);
    assert!((sim.pawn.move_ledge_location.z - 300.0).abs() <= 1.01, "UE3 line-check pullback is 1uu at this trace length");

    // let the hang start anim settle, then shimmy right
    let states = run_until(&mut sim, 120, |_| InputFrame::default(), |_| false);
    println!("settle: {states:?}");
    assert_eq!(sim.pawn.movement_state, Move::Grabbing);
    let y0 = sim.pawn.location.y;
    run_until(&mut sim, 90, |_| InputFrame { strafe: 1.0, ..Default::default() }, |_| false);
    let moved = sim.pawn.location.y - y0;
    println!("shimmy moved {moved}");
    assert!(moved > 20.0, "shimmy right should move +Y, moved {moved}");
    assert_eq!(sim.pawn.movement_state, Move::Grabbing);
    run_until(&mut sim, 60, |_| InputFrame::default(), |_| false);

    // pull up (forward on the stick -> ClimbUpLong)
    let states = run_until(
        &mut sim,
        400,
        |_| InputFrame { forward: 1.0, ..Default::default() },
        |s| s.pawn.movement_state == Move::Walking && s.pawn.physics == tdsim::Physics::Walking,
    );
    println!("pull up: {states:?} loc={:?}", sim.pawn.location);
    assert!(states.contains(&Move::GrabPullUp), "{states:?}");
    assert_eq!(sim.pawn.movement_state, Move::Walking);
    let feet = sim.pawn.location.z - sim.pawn.collision_height;
    assert!((feet - 300.0).abs() < 5.0, "should stand on top, feet at {feet}");
    assert!(sim.pawn.location.x > 400.0, "should be over the wall, x={}", sim.pawn.location.x);
}

#[test]
fn drop_from_hang() {
    let Some(mut sim) = wall_sim(300.0) else {
        return;
    };
    run_until(
        &mut sim,
        600,
        |i| InputFrame { forward: if i < 40 { 1.0 } else { 0.0 }, jump: (40..44).contains(&i), ..Default::default() },
        |s| s.pawn.movement_state == Move::IntoGrab,
    );
    run_until(&mut sim, 120, |_| InputFrame::default(), |s| s.pawn.movement_state == Move::Grabbing);
    assert_eq!(sim.pawn.movement_state, Move::Grabbing);
    run_until(&mut sim, 90, |_| InputFrame::default(), |_| false);
    let states = run_until(&mut sim, 200, |i| InputFrame { crouch: i < 5, ..Default::default() }, |s| s.pawn.movement_state == Move::Walking);
    println!("drop: {states:?}");
    assert!(states.contains(&Move::Falling), "{states:?}");
}

#[test]
#[ignore]
fn debug_trace() {
    let mut sim = wall_sim(300.0).unwrap();
    let mut prev = sim.pawn.movement_state;
    for i in 0..200 {
        let inp = InputFrame { forward: if i < 40 { 1.0 } else { 0.0 }, jump: (40..44).contains(&i), ..Default::default() };
        sim.tick(DT, inp);
        let p = &sim.pawn;
        if p.movement_state != prev || (i > 38 && i < 90) {
            let b = sim.moves.base(p.movement_state);
            println!(
                "{i} {:?} phys={:?} loc=({:.1},{:.1},{:.1}) vel=({:.0},{:.0},{:.0}) ledge=({:.1},{:.1},{:.1}) ml=({:.1},{:.1},{:.1}) res={} prec={} reached={}",
                p.movement_state, p.physics, p.location.x, p.location.y, p.location.z, p.velocity.x, p.velocity.y, p.velocity.z,
                p.move_ledge_location.x, p.move_ledge_location.y, p.move_ledge_location.z, p.move_location.x, p.move_location.y, p.move_location.z,
                p.move_ledge_result, b.use_precise_location, b.reached_precise_location
            );
            prev = p.movement_state;
        }
    }
}

#[test]
#[ignore]
fn debug_wall_push() {
    let mut sim = wall_sim(300.0).unwrap();
    let mut prev = sim.pawn.movement_state;
    for i in 0..140 {
        sim.tick(DT, InputFrame { forward: 1.0, ..Default::default() });
        let p = &sim.pawn;
        if i > 40 && (p.movement_state != prev || i < 75) {
            println!(
                "{i} {:?}/{:?} loc=({:.2},{:.2}) vel=({:.0},{:.0}) floor=({:.2},{:.2}) res={}",
                p.movement_state, p.physics, p.location.x, p.location.z, p.velocity.x, p.velocity.z, p.floor.x, p.floor.z, p.move_ledge_result
            );
        }
        prev = p.movement_state;
    }
}

#[test]
#[ignore]
fn fall_onto_ledge() {
    let install = me_level::install();
    let Ok(anims) = me_level::anims::load_player_anims(install) else { return };
    for (wall_x, top) in [(250.0f32, 420.0f32), (320.0, 380.0), (280.0, 460.0), (400.0, 300.0)] {
        let mut b = WorldBuilder::default();
        let s = b.surface(Surface::default());
        b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(5000.0, 5000.0, 50.0), 0.0, s);
        // start platform 600 high, edge at x = 0
        b.add_box(Vec3::new(-300.0, 0.0, 300.0), Vec3::new(300.0, 400.0, 300.0), 0.0, s);
        b.add_box(Vec3::new(wall_x + 300.0, 0.0, top * 0.5), Vec3::new(300.0, 400.0, top * 0.5), 0.0, s);
        let mut sim = Sim::new(b.build(), Config::load(install), anims.lib.clone());
        sim.spawn(Vec3::new(-400.0, 0.0, 600.0), 0);
        let mut log = vec![];
        let mut stuck = 0;
        for i in 0..420 {
            let fwd = if i < 120 { 1.0 } else if i > 300 { 1.0 } else { 0.0 };
            sim.tick(DT, InputFrame { forward: fwd, ..Default::default() });
            let st = (sim.pawn.movement_state, sim.pawn.physics);
            if log.last().map(|l: &(Move, tdsim::Physics, i32)| (l.0, l.1)) != Some(st) {
                log.push((st.0, st.1, i));
            }
            if sim.pawn.movement_state == Move::IntoGrab {
                stuck += 1;
            }
        }
        println!("wall {wall_x} top {top}: into-grab frames {stuck} {log:?} end {:?}", sim.pawn.location);
    }
}

#[test]
#[ignore]
fn jump_down_into_grab() {
    let install = me_level::install();
    let Ok(anims) = me_level::anims::load_player_anims(install) else { return };
    for (gap, top) in [(300.0f32, 500.0f32), (400.0, 480.0), (350.0, 520.0), (450.0, 450.0), (250.0, 540.0)] {
        let mut b = WorldBuilder::default();
        let s = b.surface(Surface::default());
        b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(5000.0, 5000.0, 50.0), 0.0, s);
        b.add_box(Vec3::new(-300.0, 0.0, 300.0), Vec3::new(300.0, 400.0, 300.0), 0.0, s);
        b.add_box(Vec3::new(gap + 300.0, 0.0, top * 0.5), Vec3::new(300.0, 400.0, top * 0.5), 0.0, s);
        let mut sim = Sim::new(b.build(), Config::load(install), anims.lib.clone());
        sim.spawn(Vec3::new(-500.0, 0.0, 600.0), 0);
        let mut log: Vec<(Move, tdsim::Physics, i32)> = vec![];
        let mut jumped = false;
        for i in 0..360 {
            let j = !jumped && sim.pawn.location.x > -40.0;
            if j { jumped = true; }
            sim.tick(DT, InputFrame { forward: if jumped && i < 400 { 0.0 } else { 1.0 }, jump: j, ..Default::default() });
            let st = (sim.pawn.movement_state, sim.pawn.physics);
            if log.last().map(|l| (l.0, l.1)) != Some(st) {
                log.push((st.0, st.1, i));
            }
        }
        println!("gap {gap} top {top}: {log:?} end {:?} v {:?}", sim.pawn.location, sim.pawn.velocity);
    }
}

#[test]
#[ignore]
fn corner_shimmy_path() {
    let Some(mut sim) = wall_sim(300.0) else { return };
    run_until(&mut sim, 600, |i| InputFrame { forward: if i < 40 { 1.0 } else { 0.0 }, jump: (40..44).contains(&i), ..Default::default() }, |s| s.pawn.movement_state == Move::IntoGrab);
    run_until(&mut sim, 120, |_| InputFrame::default(), |s| s.pawn.movement_state == Move::Grabbing);
    run_until(&mut sim, 60, |_| InputFrame::default(), |_| false);
    let mut corner_seen = false;
    for i in 0..900 {
        sim.tick(DT, InputFrame { strafe: 1.0, ..Default::default() });
        let around = sim.moves.grab.shimmy == tdsim::moves::grab::Shimmy::AroundCorner;
        if around || (corner_seen && i % 10 == 0) {
            corner_seen = true;
            if i % 6 == 0 || !around {
                println!("{i} {:?} around {around} loc ({:.0},{:.0},{:.0}) yaw {} ledge ({:.0},{:.0},{:.0})", sim.pawn.movement_state,
                    sim.pawn.location.x, sim.pawn.location.y, sim.pawn.location.z, sim.pawn.rotation.yaw,
                    sim.pawn.move_ledge_location.x, sim.pawn.move_ledge_location.y, sim.pawn.move_ledge_location.z);
            }
            if !around && corner_seen && i % 10 == 0 { break; }
        }
    }
}

#[test]
#[ignore]
fn hang_turn_around() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    let Some(mut sim) = wall_sim(300.0) else { return };
    let mut pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    run_until(&mut sim, 600, |i| InputFrame { forward: if i < 40 { 1.0 } else { 0.0 }, jump: (40..44).contains(&i), ..Default::default() }, |s| s.pawn.movement_state == Move::IntoGrab);
    run_until(&mut sim, 120, |_| InputFrame::default(), |s| s.pawn.movement_state == Move::Grabbing);
    run_until(&mut sim, 90, |_| InputFrame::default(), |_| false);
    for i in 0..240 {
        let mx = if i < 120 { 12.0 } else { 0.0 };
        sim.tick(DT, InputFrame { mouse_x: mx, ..Default::default() });
        pe.update(&mut sim, DT);
        if i % 20 == 0 {
            let m = me_level::pose::mesh_to_world(&sim, a.lib.mesh_rot, a.upper.origin);
            let g = pe.globals();
            let at = |n: &str| { let t = (m * g[pe.bone_index(n).unwrap()]).w_axis; Vec3::new(t.x * 100.0, t.z * 100.0, t.y * 100.0) };
            let eye = at("EyeJoint");
            let (f, r, u) = sim.pc.rotation.axes();
            let rel = |n: &str| { let d = at(n) - eye; (d.dot(f) as i32, d.dot(r) as i32, d.dot(u) as i32) };
            println!("   view-frame fwd/right/up  R {:?}  L {:?}  pitch {}", rel("RightHand"), rel("LeftHand"), sim.pc.rotation.pitch);
            let dy = ((sim.pc.rotation.yaw - sim.pawn.rotation.yaw) as u16 as i16) as i32;
            println!("{i} dyaw {dy} turn {:?} slot {:?} loco {:?}", sim.pawn.current_grab_turn_type,
                sim.anim.current(tdsim::pawn::Slot::FullBody).map(|s| s.name.clone()), pe.locomotion_debug());
        }
    }
}

/// Fall into a grab on a 100-thick slab with nothing below (hang free), look around, then
/// try to pull up. Logs the eye against the slab.
#[test]
#[ignore]
fn hang_free_slab() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    let gap: f32 = std::env::var("GAP").ok().and_then(|v| v.parse().ok()).unwrap_or(350.0);
    let top: f32 = std::env::var("TOP").ok().and_then(|v| v.parse().ok()).unwrap_or(-20.0);
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(-500.0, 0.0, -50.0), Vec3::new(500.0, 2000.0, 50.0), 0.0, s);
    // thin slab, its near face at x = gap
    b.add_box(Vec3::new(gap + 300.0, 0.0, top - 50.0), Vec3::new(300.0, 2000.0, 50.0), 0.0, s);
    let mut sim = Sim::new(b.build(), Config::load(install), a.lib.clone());
    let yaw_deg: f32 = std::env::var("YAW").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    sim.spawn(Vec3::new(-800.0, -(800.0 * yaw_deg.to_radians().tan()) + std::env::var("SPY").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0), 100.0), (yaw_deg * 182.044) as i32);
    let mut pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    let mut swan = me_level::camera::SwanNeck::default();
    let mut log: Vec<(Move, tdsim::Physics, i32)> = vec![];
    let dt: f32 = 1.0 / std::env::var("FPS").ok().and_then(|v| v.parse().ok()).unwrap_or(60.0);
    let k = (1.0 / 60.0 / dt).round() as usize;
    if let Some(v) = std::env::var("SV").ok().and_then(|v| v.parse::<f32>().ok()) { sim.moves.grab.shimmy_velocity = v; }
    let hold = std::env::var("HOLD").is_ok();
    let early = std::env::var("EARLY").is_ok();
    let mut jumped = false;
    for f in 0..900 * k {
        let i = f / k;
        let j = !jumped && sim.pawn.location.x > -40.0;
        if j { jumped = true; }
        let grabbing = sim.pawn.movement_state == Move::Grabbing;
        let inp = InputFrame {
            forward: if !jumped || hold { 1.0 } else if i > 700 { 1.0 } else { 0.0 },
            jump: j,
            mouse_x: if grabbing && early && (150..200).contains(&i) { std::env::var("EM").ok().and_then(|v| v.parse().ok()).unwrap_or(40.0) } else if grabbing && (400..460).contains(&i) { 30.0 } else if grabbing && (460..560).contains(&i) { -30.0 } else { 0.0 },
            mouse_y: if grabbing && (560..640).contains(&i) { -8.0 } else { 0.0 },
            ..Default::default()
        };
        let inp = InputFrame { mouse_x: inp.mouse_x / k as f32, mouse_y: inp.mouse_y / k as f32, jump: inp.jump && f % k == 0, ..inp };
        if let Some(pp) = std::env::var("PITCH").ok().and_then(|v| v.parse::<i32>().ok()) { if sim.pawn.movement_state != Move::Grabbing && sim.pawn.movement_state != Move::IntoGrab { sim.pc.rotation.pitch = pp; } }
        sim.tick(dt, inp);
        pe.update(&mut sim, dt);
        swan.update(&sim, dt);
        let st = (sim.pawn.movement_state, sim.pawn.physics);
        if log.last().map(|l| (l.0, l.1)) != Some(st) {
            log.push((st.0, st.1, i as i32));
        }
        let every = std::env::var("EVERY").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(15);
        if jumped && f % (every * k).max(1) == 0 {
            let m = me_level::pose::mesh_to_world(&sim, a.lib.mesh_rot, a.upper.origin);
            let g = pe.globals();
            let t = (m * g[pe.bone_index("EyeJoint").unwrap()]).w_axis;
            let eye = Vec3::new(t.x * 100.0, t.z * 100.0, t.y * 100.0) + swan.offset(sim.pc.rotation);
            let inside = eye.x > gap - 2.0 && eye.z < top && eye.z > top - 100.0;
            println!("{i} {:?} {:?} loc ({:.0},{:.0},{:.0}) eye ({:.0},{:.0},{:.0}){} view p{} y{} pawn y{} slot {:?} free {} folded {:?} vert {} cl {} v {:.0} sh {:?} cam {:?}",
                sim.pawn.movement_state, sim.pawn.physics, sim.pawn.location.x, sim.pawn.location.y, sim.pawn.location.z,
                eye.x, eye.y, eye.z, if inside { " INSIDE" } else { "" }, sim.pc.rotation.pitch as i16, sim.pc.rotation.yaw as i16,
                sim.pawn.rotation.yaw as i16, sim.anim.current(tdsim::pawn::Slot::FullBody).map(|s| s.name.clone()),
                sim.grab_is_hanging_free(), sim.moves.grab.folded, sim.moves.grab.hang_free_vertigo_effect, sim.moves.base(Move::Grabbing).constrain_look, sim.pawn.velocity.length(), sim.moves.grab.shimmy, pe.camera_animation());
        }
    }
    println!("{log:?}");
}

/// Run at a test-map obstacle in a lane and jump at `JUMP` x; log moves and whether the eye
/// ends up inside a block (LANE = spawn index, YOFF = sideways offset).
#[test]
#[ignore]
fn testmap_jump_grab() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    let env = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let lane = env("LANE", 4.0) as usize;
    let jump_x = env("JUMP", 450.0);
    let blocks = tdsim::testmap::blocks();
    let mut sim = Sim::new(tdsim::testmap::world(), Config::load(install), a.lib.clone());
    let sp = &tdsim::testmap::spawns()[lane];
    let mut feet = sp.feet;
    feet.y += env("YOFF", 0.0);
    feet.x += env("XOFF", 0.0);
    let yaw = env("SPYAW", sp.yaw as f32) as i32;
    sim.spawn(feet + Vec3::new(0.0, 0.0, 90.0), yaw);
    let dir = tdsim::Rotator::new(0, yaw, 0).vector();
    let start = sim.pawn.location;
    let mut pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    let mut swan = me_level::camera::SwanNeck::default();
    let pitch0 = env("PITCH", 0.0) as i32;
    let mut jumped = false;
    let mut log: Vec<(Move, i32)> = vec![];
    for i in 0..900 {
        let j = !jumped && (sim.pawn.location - start).dot(dir) > jump_x;
        if j { jumped = true; }
        if !jumped { sim.pc.rotation.pitch = pitch0; }
        let fwd_at = env("FWDAT", 100000.0) as i32;
        sim.tick(DT, InputFrame { forward: if !jumped || i >= fwd_at { 1.0 } else { 0.0 }, jump: j, ..Default::default() });
        pe.update(&mut sim, DT);
        swan.update(&sim, DT);
        if log.last().map(|l| l.0) != Some(sim.pawn.movement_state) { log.push((sim.pawn.movement_state, i)); }
        let m = me_level::pose::mesh_to_world(&sim, a.lib.mesh_rot, a.upper.origin);
        let g = pe.globals();
        let t = (m * g[pe.bone_index("EyeJoint").unwrap()]).w_axis;
        let eye = Vec3::new(t.x * 100.0, t.z * 100.0, t.y * 100.0) + swan.offset(sim.pc.rotation);
        let inside = blocks.iter().any(|b| b.yaw == 0.0 && b.pitch == 0.0 && eye.x > b.min.x && eye.x < b.max.x && eye.y > b.min.y && eye.y < b.max.y && eye.z > b.min.z && eye.z < b.max.z);
        if jumped && (inside || i % 10 == 0) && matches!(sim.pawn.movement_state, Move::IntoGrab | Move::Grabbing | Move::GrabPullUp) {
            println!("{i} {:?} loc ({:.0},{:.0},{:.0}) eye ({:.0},{:.0},{:.0}){} view p{} y{} slot {:?} free {} folded {:?} swan {:?} ledge ({:.0},{:.0},{:.0}) n ({:.2},{:.2},{:.2})",
                sim.pawn.movement_state, sim.pawn.location.x, sim.pawn.location.y, sim.pawn.location.z, eye.x, eye.y, eye.z,
                if inside { " INSIDE" } else { "" }, sim.pc.rotation.pitch as i16, sim.pc.rotation.yaw as i16,
                sim.anim.current(tdsim::pawn::Slot::FullBody).map(|s| s.name.clone()), sim.grab_is_hanging_free(), sim.moves.grab.folded,
                swan.translation, sim.pawn.move_ledge_location.x, sim.pawn.move_ledge_location.y, sim.pawn.move_ledge_location.z,
                sim.pawn.move_normal.x, sim.pawn.move_normal.y, sim.pawn.move_normal.z);
        }
    }
    println!("{log:?}");
}

/// Hanging free with a leftover left shimmy (ShimmyVelocity < 0 makes ShimmyMove call
/// AbortShimmy every tick, cutting custom anims): the hang-free vertigo turn still has to end
/// (the stopped sequence keeps ticking to OnAnimEnd) and the pull-up must work afterwards.
#[test]
fn hang_free_turn_ends_after_left_shimmy() {
    let install = me_level::install();
    let Ok(a) = me_level::anims::load_player_anims(install) else { return };
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(-500.0, 0.0, -50.0), Vec3::new(500.0, 2000.0, 50.0), 0.0, s);
    // 100-thick slab with nothing under it
    b.add_box(Vec3::new(940.0, 0.0, -150.0), Vec3::new(300.0, 2000.0, 50.0), 0.0, s);
    let mut sim = Sim::new(b.build(), Config::load(install), a.lib.clone());
    sim.spawn(Vec3::new(-800.0, 0.0, 100.0), 0);
    sim.moves.grab.shimmy_velocity = -60.0;
    let mut jumped = false;
    let mut turned = false;
    for i in 0..900 {
        let j = !jumped && sim.pawn.location.x > -40.0;
        jumped |= j;
        let grabbing = sim.pawn.movement_state == Move::Grabbing;
        let inp = InputFrame {
            forward: if !jumped || i > 700 { 1.0 } else { 0.0 },
            jump: j,
            mouse_x: if grabbing && (400..460).contains(&i) { 30.0 } else { 0.0 },
            ..Default::default()
        };
        sim.tick(DT, inp);
        turned |= sim.moves.grab.hang_free_vertigo_effect;
        if i == 700 {
            assert_eq!(sim.pawn.movement_state, Move::Grabbing);
            assert!(turned, "the look should have triggered the hang-free turn");
            assert!(!sim.moves.grab.hang_free_vertigo_effect, "vertigo turn never ended");
        }
    }
    assert_eq!(sim.pawn.movement_state, Move::Walking, "should have pulled up onto the slab");
}

/// Hand / finger bones against the ledge while hanging (wall face x=400, top z=300).
#[test]
#[ignore]
fn hang_fingers_vs_ledge() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    let Some(mut sim) = wall_sim(300.0) else { return };
    let mut pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    run_until(&mut sim, 600, |i| InputFrame { forward: if i < 40 { 1.0 } else { 0.0 }, jump: (40..44).contains(&i), ..Default::default() }, |s| s.pawn.movement_state == Move::IntoGrab);
    run_until(&mut sim, 120, |_| InputFrame::default(), |s| s.pawn.movement_state == Move::Grabbing);
    for _ in 0..180 { sim.tick(DT, InputFrame::default()); pe.update(&mut sim, DT); }
    let m = me_level::pose::mesh_to_world(&sim, a.lib.mesh_rot, a.upper.origin);
    let g = pe.globals();
    println!("pawn {:?} ledge {:?} root_offset {:?} mesh_z {}", sim.pawn.location, sim.pawn.move_ledge_location, sim.pawn.root_offset, sim.pawn.mesh_translation_z);
    for (i, n) in pe.bone_names.iter().enumerate() {
        let l = n.to_lowercase();
        if l.contains("right") && (l.contains("hand") || l.contains("finger") || l.contains("index") || l.contains("middle") || l.contains("thumb") || l.contains("ring") || l.contains("pinky")) {
            let t = (m * g[i]).w_axis;
            println!("{n:28} x {:7.1} y {:7.1} z {:7.1}", t.x * 100.0, t.z * 100.0, t.y * 100.0);
        }
    }
}

/// Inside corner: hanging on wall A (face x = 400), wall B on the right (face y = 580 facing
/// -Y, same height). Strafe-right + jump -> GrabTransfer -> IntoGrab -> hanging on B.
#[test]
fn grab_transfer_inside_corner() {
    let install = me_level::install();
    let Ok(anims) = me_level::anims::load_player_anims(install) else { return };
    for look in [false, true] {
        let mut b = WorldBuilder::default();
        let s = b.surface(Surface::default());
        b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(5000.0, 5000.0, 50.0), 0.0, s);
        b.add_box(Vec3::new(700.0, 0.0, 150.0), Vec3::new(300.0, 500.0, 150.0), 0.0, s);
        b.add_box(Vec3::new(250.0, 630.0, 150.0), Vec3::new(150.0, 50.0, 150.0), 0.0, s);
        let mut sim = Sim::new(b.build(), Config::load(install), anims.lib.clone());
        sim.spawn(Vec3::new(0.0, 420.0, 0.0), 0);
        run_until(&mut sim, 30, |_| InputFrame::default(), |_| false);
        run_until(
            &mut sim,
            600,
            |i| InputFrame { forward: if i < 40 { 1.0 } else { 0.0 }, jump: (40..44).contains(&i), ..Default::default() },
            |s| s.pawn.movement_state == Move::IntoGrab,
        );
        run_until(&mut sim, 200, |_| InputFrame::default(), |_| false);
        assert_eq!(sim.pawn.movement_state, Move::Grabbing, "should hang on A");
        println!("hanging on A at {:?} normal {:?}", sim.pawn.location, sim.pawn.move_normal);
        let states = if look {
            // turn the view to the right (towards B) and jump with no direction held
            run_until(&mut sim, 40, |i| InputFrame { mouse_x: if i < 21 { 60.0 } else { 0.0 }, ..Default::default() }, |_| false);
            println!("view yaw {} pawn yaw {}", sim.pc.rotation.yaw, sim.pawn.rotation.yaw);
            run_until(&mut sim, 300, |i| InputFrame { jump: i < 4, ..Default::default() }, |s| s.pawn.movement_state == Move::Grabbing && s.pawn.move_normal.y < -0.9)
        } else {
            run_until(&mut sim, 300, |i| InputFrame { strafe: if i < 6 { 1.0 } else { 0.0 }, jump: (2..6).contains(&i), ..Default::default() }, |s| s.pawn.movement_state == Move::Grabbing && s.pawn.move_normal.y < -0.9)
        };
        println!("look {look}: {states:?} at {:?} normal {:?}", sim.pawn.location, sim.pawn.move_normal);
        assert!(states.contains(&Move::GrabTransfer), "look {look}: {states:?}");
        assert_eq!(sim.pawn.movement_state, Move::Grabbing, "look {look}: {states:?}");
        assert!(sim.pawn.move_normal.y < -0.9, "should hang on B");
    }
}

/// The test map's lane F corner (block end y 6200, transfer wall face y 6280).
#[test]
fn grab_transfer_testmap_lane_f() {
    let install = me_level::install();
    let Ok(anims) = me_level::anims::load_player_anims(install) else { return };
    let mut sim = Sim::new(tdsim::testmap::world(), Config::load(install), anims.lib);
    sim.spawn(Vec3::new(600.0, 6150.0, 0.0), 0);
    run_until(&mut sim, 30, |_| InputFrame::default(), |_| false);
    run_until(
        &mut sim,
        600,
        |i| InputFrame { forward: if i < 40 { 1.0 } else { 0.0 }, jump: (40..44).contains(&i), ..Default::default() },
        |s| s.pawn.movement_state == Move::IntoGrab,
    );
    run_until(&mut sim, 200, |_| InputFrame::default(), |_| false);
    assert_eq!(sim.pawn.movement_state, Move::Grabbing);
    let states = run_until(&mut sim, 300, |i| InputFrame { strafe: if i < 6 { 1.0 } else { 0.0 }, jump: (2..6).contains(&i), ..Default::default() }, |s| s.pawn.movement_state == Move::Grabbing && s.pawn.move_normal.y < -0.9);
    println!("{states:?} at {:?}", sim.pawn.location);
    assert!(states.contains(&Move::GrabTransfer) && sim.pawn.move_normal.y < -0.9, "{states:?}");
}

/// Hanging next to a ladder (on the taller wall to the right): strafe-right + jump transfers
/// onto it (GrabTransfer -> IntoClimb -> Climb).
#[test]
fn grab_transfer_to_ladder() {
    let install = me_level::install();
    let Ok(anims) = me_level::anims::load_player_anims(install) else { return };
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(5000.0, 5000.0, 50.0), 0.0, s);
    b.add_box(Vec3::new(700.0, 0.0, 150.0), Vec3::new(300.0, 500.0, 150.0), 0.0, s);
    b.add_box(Vec3::new(700.0, 700.0, 300.0), Vec3::new(300.0, 200.0, 300.0), 0.0, s);
    let mut sim = Sim::new(b.build(), Config::load(install), anims.lib);
    // TdLadderVolume up wall B's face at x = 400, laid out like the test map's
    let (top, bottom) = (600.0 + 96.0, -16.0);
    sim.ladders = vec![tdsim::ladder::LadderVolume::new(
        Vec3::new(388.0 - 64.0 + 65.0, 640.0, (top + bottom) * 0.5),
        Vec3::new(65.0, 40.0, (top - bottom) * 0.5),
        0,
        tdsim::ladder::LadderType::Ladder,
    )];
    sim.spawn(Vec3::new(0.0, 440.0, 0.0), 0);
    run_until(&mut sim, 30, |_| InputFrame::default(), |_| false);
    run_until(
        &mut sim,
        600,
        |i| InputFrame { forward: if i < 40 { 1.0 } else { 0.0 }, jump: (40..44).contains(&i), ..Default::default() },
        |s| s.pawn.movement_state == Move::IntoGrab,
    );
    run_until(&mut sim, 200, |_| InputFrame::default(), |_| false);
    assert_eq!(sim.pawn.movement_state, Move::Grabbing);
    let states = run_until(&mut sim, 400, |i| InputFrame { strafe: if i < 6 { 1.0 } else { 0.0 }, jump: (2..6).contains(&i), ..Default::default() }, |s| s.pawn.movement_state == Move::Climb);
    println!("{states:?} at {:?}", sim.pawn.location);
    assert!(states.contains(&Move::GrabTransfer) && sim.pawn.movement_state == Move::Climb, "{states:?}");
}
