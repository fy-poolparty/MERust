//! Lane I: swing bars and the zipline.

use std::path::Path;
use tdsim::config::Config;
use tdsim::{InputFrame, Move, Sim, Vec3};

const DT: f32 = 1.0 / 60.0;

fn lane_sim() -> Option<Sim> {
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

fn spawn_at(sim: &mut Sim, name_prefix: &str) {
    let s = tdsim::testmap::spawns().into_iter().find(|s| s.name.starts_with(name_prefix)).unwrap();
    sim.spawn(s.feet, s.yaw);
}

/// Runs `frames`, printing state changes, custom anims and sounds; returns the states visited.
fn run(sim: &mut Sim, frames: usize, verbose: bool, input: impl Fn(usize, &Sim) -> InputFrame) -> Vec<Move> {
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
            let line = format!("{:?}/{:?} am {:?} [{anim}]", sim.pawn.movement_state, sim.pawn.physics, sim.pawn.animation_movement_state);
            if line != last {
                println!("  {i}: {line} at {:?} v {:?}", sim.pawn.location, sim.pawn.velocity);
                last = line;
            }
            for e in &sim.events {
                if let tdsim::sim::Event::Sound(s) = e {
                    println!("  {i}: sound {s:?}");
                }
            }
        }
    }
    states
}

#[test]
fn swing_bar_catch_and_jump_off() {
    let Some(mut sim) = lane_sim() else { return };
    spawn_at(&mut sim, "I swing");
    let verbose = std::env::var("VERBOSE").is_ok();
    // run at the bar, jump near the edge, swing, then jump at the top of the forward swing
    let st = run(&mut sim, 60 * 8, verbose, |i, s| {
        let jump_edge = s.pawn.movement_state == Move::Walking && s.pawn.location.x > 650.0;
        let swinging = s.pawn.movement_state == Move::Swing;
        let at_top = swinging && s.moves.swing.swing_angle > 1.0 && s.moves.base(Move::Swing).move_active_time > 1.0;
        InputFrame { forward: if swinging && !at_top { 1.0 } else if swinging { 0.0 } else { 1.0 }, jump: jump_edge || (at_top && i % 2 == 0), ..Default::default() }
    });
    println!("states {st:?} at {:?}", sim.pawn.location);
    assert!(st.contains(&Move::Swing), "should catch the bar: {st:?}");
    assert!(st.contains(&Move::SwingJump), "should jump off: {st:?}");
}

#[test]
fn zipline_ride() {
    let Some(mut sim) = lane_sim() else { return };
    spawn_at(&mut sim, "I zipline");
    let verbose = std::env::var("VERBOSE").is_ok();
    let st = run(&mut sim, 60 * 14, verbose, |_, s| {
        let near_edge = s.pawn.movement_state == Move::Walking && s.pawn.location.x > 330.0;
        InputFrame { forward: if s.pawn.movement_state == Move::Walking { 1.0 } else { 0.0 }, jump: near_edge, ..Default::default() }
    });
    println!("states {st:?} at {:?}", sim.pawn.location);
    assert!(st.contains(&Move::ZipLine), "should ride the cable: {st:?}");
    assert!(sim.pawn.location.x > 2500.0, "should come off near the end: {:?}", sim.pawn.location);
}

/// Camera animation and eye axes while swinging (env STRENGTH0=1 drops the SwingControl).
#[test]
#[ignore]
fn swing_camera() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    let Some(mut sim) = lane_sim() else { return };
    let mut pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    spawn_at(&mut sim, "I swing");
    for i in 0..60 * 5 {
        let jump = sim.pawn.movement_state == Move::Walking && sim.pawn.location.x > 650.0;
        sim.tick(DT, InputFrame { forward: 1.0, jump, ..Default::default() });
        pe.update(&mut sim, DT);
        if sim.pawn.movement_state == Move::Swing && i % 10 == 0 {
            let ca = pe.camera_animation();
            let m = me_level::pose::mesh_to_world(&sim, a.lib.mesh_rot, a.upper.origin);
            let g = pe.globals();
            let e = m * g[pe.bone_index("EyeJoint").unwrap()];
            let ue = |v: glam::Vec4| (v.x, v.z, v.y);
            println!("{i} angle {:.2} ctl roll {} str {:.2} cam p {} y {} r {} eye x {:.2?} y {:.2?} z {:.2?}", sim.moves.swing.swing_angle, sim.pawn.swing_control.roll, sim.pawn.swing_control.strength,
                ca.pitch, ca.yaw, ca.roll, ue(e.x_axis), ue(e.y_axis), ue(e.z_axis));
        }
    }
}

#[test]
fn swing_and_zipline_cues_load() {
    let install = me_level::install();
    if !install.exists() {
        return;
    }
    let bank = me_level::sounds::load_sound_bank(install, &[]);
    for c in [tdsim::moves::swing::SWING_SOUND, tdsim::moves::zipline::ZIPPING_SOUND] {
        let cue = bank.cues.get(c).unwrap_or_else(|| panic!("{c} missing"));
        let mut ws = Vec::new();
        upk::sound::waves(&cue.root, &mut ws);
        println!("{c}: {:?} waves {}", cue.root, ws.len());
        assert!(ws.iter().all(|w| bank.waves.contains_key(w)), "{c}: waves missing");
    }
}

/// Where the body sits in the view right after jumping off a bar.
#[test]
#[ignore]
fn swing_jump_off_view() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    let Some(mut sim) = lane_sim() else { return };
    let mut pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    spawn_at(&mut sim, "I swing");
    let mut after = 0;
    for _ in 0..60 * 8 {
        let s = &sim;
        let swinging = s.pawn.movement_state == Move::Swing;
        let at_top = swinging && s.moves.swing.swing_angle > 1.0 && s.moves.base(Move::Swing).move_active_time > 1.5;
        let jump_edge = s.pawn.movement_state == Move::Walking && s.pawn.location.x > 650.0 && after == 0;
        sim.tick(DT, InputFrame { forward: if after == 0 { 1.0 } else { 0.0 }, jump: jump_edge || at_top, ..Default::default() });
        pe.update(&mut sim, DT);
        if sim.pawn.movement_state == Move::SwingJump || after > 0 {
            after += 1;
            if after % 3 == 1 && after < 50 {
                let m = me_level::pose::mesh_to_world(&sim, a.lib.mesh_rot, a.upper.origin);
                let g = pe.globals();
                let at = |b: &str| { let t = (m * g[pe.bone_index(b).unwrap()]).w_axis; Vec3::new(t.x * 100.0, t.z * 100.0, t.y * 100.0) };
                let view = me_level::camera::camera_rotation(sim.pc.rotation, pe.camera_animation());
                let (vx, vy, vz) = view.axes();
                let e = at("EyeJoint");
                let rel = |b: &str| { let d = at(b) - e; format!("{b} ({:.0} {:.0} {:.0})", d.dot(vx), d.dot(vy), d.dot(vz)) };
                let slot = sim.anim.current(tdsim::pawn::Slot::FullBody).map(|c| format!("{} w {:.2}", c.name, c.weight)).unwrap_or_default();
                println!("{after} slot [{slot}] {:?} am {:?} view p {} cam {:?} swing str {:.2} root {:.2} | {} {} {} {}", sim.pawn.movement_state, sim.pawn.animation_movement_state, view.pitch, pe.camera_animation(),
                    sim.pawn.swing_control.strength, sim.pawn.root_offset_strength, rel("Hips"), rel("Spine1"), rel("Neck"), rel("RightHand"));
            }
        }
    }
}

/// Balance across the lane J pipe (or beam, BEAM=1), countering the lean with strafe; without
/// countering (NOCOUNTER=1) the lean runs away and you fall off.
fn balance_run(spawn: &str, counter: bool) -> (Vec<Move>, Sim) {
    let mut sim = lane_sim().unwrap();
    spawn_at(&mut sim, spawn);
    // a slightly turned view, as a player's mouse never sits still: it starts the lean
    let look = 600;
    let verbose = std::env::var("VERBOSE").is_ok();
    let st = run(&mut sim, 60 * 14, verbose, |i, s| {
        let lean = s.moves.balance.balance_factor;
        let strafe = if counter && s.pawn.movement_state == Move::Balance { (-lean * 4.0).clamp(-1.0, 1.0) } else { 0.0 };
        if verbose && i % 30 == 0 && s.pawn.movement_state == Move::Balance {
            println!("    lean {lean:.2} danger {} at {:?} v {:.0}", s.moves.balance.danger, s.pawn.location, s.pawn.velocity.length());
        }
        let on = s.pawn.movement_state == Move::Balance && s.moves.base(Move::Balance).move_active_time < 0.02;
        let mouse_x = if on { look as f32 / s.pc.mouse_sensitivity.max(1e-3) } else { 0.0 };
        InputFrame { forward: if s.pawn.location.x < 1900.0 { 1.0 } else { 0.0 }, strafe, mouse_x, ..Default::default() }
    });
    println!("states {st:?} at {:?}", sim.pawn.location);
    (st, sim)
}

#[test]
fn balance_pipe_with_counter() {
    if lane_sim().is_none() {
        return;
    }
    let (st, sim) = balance_run("J balance: pipe", true);
    assert!(st.contains(&Move::Balance), "{st:?}");
    assert!(sim.pawn.location.x > 1700.0 && sim.pawn.location.z > 50.0, "should reach the far side: {:?}", sim.pawn.location);
}

#[test]
fn balance_beam_without_counter_falls() {
    if lane_sim().is_none() {
        return;
    }
    let (st, sim) = balance_run("J balance: narrow beam", false);
    assert!(st.contains(&Move::Balance), "{st:?}");
    assert!(sim.pawn.location.z < -200.0, "should fall off: {:?} {st:?}", sim.pawn.location);
}

#[test]
fn zipline_drop_midway() {
    let Some(mut sim) = lane_sim() else { return };
    spawn_at(&mut sim, "I zipline");
    let st = run(&mut sim, 60 * 8, false, |_, s| {
        let near_edge = s.pawn.movement_state == Move::Walking && s.pawn.location.x > 330.0;
        let midway = s.pawn.movement_state == Move::ZipLine && s.pawn.location.x > 1500.0;
        InputFrame { forward: if s.pawn.movement_state == Move::Walking { 1.0 } else { 0.0 }, jump: near_edge, crouch: midway, ..Default::default() }
    });
    println!("states {st:?} at {:?}", sim.pawn.location);
    let after = st.iter().position(|m| *m == Move::ZipLine).expect("rode the cable");
    assert_eq!(st.get(after + 1), Some(&Move::Falling), "{st:?}");
    assert!(sim.pawn.location.x < 2600.0, "dropped before the end: {:?}", sim.pawn.location);
}
