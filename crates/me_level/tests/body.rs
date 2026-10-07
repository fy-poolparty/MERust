//! Leg yaw (strafing), TdAnimNodeTurn and the against-wall camera pieces.

use std::path::Path;
use tdsim::config::Config;
use tdsim::math::norm_axis;
use tdsim::{InputFrame, Move, Sim, Vec3};

const DT: f32 = 1.0 / 60.0;

fn setup() -> Option<(Sim, me_level::pose::PoseEvaluator)> {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).ok()?;
    let aim = |n: &str| me_level::anims::load_aim_profile(install, n).unwrap();
    let pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper).with_aim(aim("TdAnimNodeDirBone_0"), aim("TdAnimNodeDirBone_1"));
    let sim = Sim::new(tdsim::testmap::world(), Config::load(install), a.lib);
    Some((sim, pe))
}

/// Yaw (UE units, relative to the pawn) the hips face, from the posed mesh: the right hip to
/// the left hip points left of the facing (a toe would flip with the running stride).
fn toe_yaw(sim: &Sim, pe: &me_level::pose::PoseEvaluator) -> i32 {
    hips_yaw(sim, pe)
}

fn hips_yaw(sim: &Sim, pe: &me_level::pose::PoseEvaluator) -> i32 {
    let g = pe.globals();
    let (f, t) = (pe.bone_index("RightUpLeg").unwrap(), pe.bone_index("LeftUpLeg").unwrap());
    let m = me_level::pose::mesh_to_world(sim, tdsim::Rotator::new(0, -16384, 16384), [0.0, 94.0, 0.0]);
    let a = (m * g[f]).w_axis.truncate();
    let b = (m * g[t]).w_axis.truncate();
    let d = b - a;
    // glTF/Bevy (x, y up, z) -> Unreal (x, z, y)
    let yaw = (d.z.atan2(d.x) * 32768.0 / std::f32::consts::PI) as i32;
    // the hip line points left (-16384 off the facing)
    norm_axis(yaw + 16384 - sim.pawn.rotation.yaw)
}

fn run(sim: &mut Sim, pe: &mut me_level::pose::PoseEvaluator, frames: usize, inp: InputFrame) {
    for _ in 0..frames {
        sim.tick(DT, inp);
        pe.update(sim, DT);
    }
}

#[test]
fn strafe_turns_the_legs() {
    let Some((mut sim, mut pe)) = setup() else { return };
    sim.spawn(Vec3::new(1500.0, 0.0, 0.0), 0);
    run(&mut sim, &mut pe, 30, InputFrame::default());
    let still = toe_yaw(&sim, &pe);
    for (strafe, n) in [(1.0f32, 50), (-1.0, 75)] {
        run(&mut sim, &mut pe, n, InputFrame { strafe, ..Default::default() });
        let leg = norm_axis(sim.pawn.leg_rotation - sim.pawn.rotation.yaw);
        let toe = toe_yaw(&sim, &pe);
        println!("strafe {strafe}: speed {:.0} leg {leg} toe {toe} (standing {still}) going_forward {} loco {:?}", sim.pawn.velocity_magnitude_2d, sim.pawn.going_forward, pe.locomotion_debug());
        assert!((leg as f32 * strafe) > 12000.0, "legs should swing towards the strafe: {leg}");
        assert!(((toe - still) as f32 * strafe) > 8000.0, "toes should point towards the strafe: {toe} vs {still}");
    }
    // running backwards: legs stay forward-ish (bGoingForward off)
    run(&mut sim, &mut pe, 90, InputFrame { forward: -1.0, ..Default::default() });
    let leg = norm_axis(sim.pawn.leg_rotation - sim.pawn.rotation.yaw);
    println!("backwards: leg {leg} going_forward {} loco {:?}", sim.pawn.going_forward, pe.locomotion_debug());
    assert!(!sim.pawn.going_forward && leg.abs() < 3000);
}

#[test]
fn turning_in_place_steps() {
    let Some((mut sim, mut pe)) = setup() else { return };
    sim.spawn(Vec3::new(0.0, 0.0, 0.0), 0);
    run(&mut sim, &mut pe, 30, InputFrame::default());
    // turn the view 120 degrees to the right over half a second
    let mut started = false;
    for i in 0..120 {
        let mx = if i < 30 { 40.0 } else { 0.0 };
        sim.tick(DT, InputFrame { mouse_x: mx, ..Default::default() });
        pe.update(&mut sim, DT);
        started |= sim.turn_node.playing_turn_animation;
        if i % 10 == 0 {
            println!("{i}: yaw {} leg {} turning {} child {}", sim.pawn.rotation.yaw, sim.pawn.leg_rotation, sim.turn_node.playing_turn_animation, sim.turn_node.active_child);
        }
    }
    assert!(started, "a turn step should play");
    let d = norm_axis(sim.pawn.rotation.yaw - sim.pawn.leg_rotation);
    assert!(d.abs() < 65 * 182, "legs should have caught up: {d}");
}

#[test]
fn against_wall_pushes_the_eye_back() {
    let Some((mut sim, mut pe)) = setup() else { return };
    // the test map's first wall: walk into it, then look down
    sim.spawn(Vec3::new(600.0, 5950.0, 0.0), 0);
    let mut hit = false;
    for i in 0..60 * 4 {
        sim.tick(DT, InputFrame { forward: 1.0, mouse_y: if i > 60 * 2 { -30.0 } else { 0.0 }, ..Default::default() });
        pe.update(&mut sim, DT);
        if sim.pawn.against_wall_state != tdsim::body::AgainstWall::None {
            hit = true;
        }
    }
    println!("state {:?} pitch {} mesh xy {:?} loc {:?}", sim.pawn.against_wall_state, sim.pc.rotation.pitch, sim.pawn.mesh_offset_xy, sim.pawn.location);
    assert!(hit, "should reach a wall");
}

#[test]
fn camera_collision_offsets_mesh() {
    let Some((mut sim, _pe)) = setup() else { return };
    sim.spawn(Vec3::new(0.0, 0.0, 0.0), 0);
    run(&mut sim, &mut setup().unwrap().1, 10, InputFrame::default());
    // an eye right in front of nothing: no push; the push itself is checked in-game
    let eye = sim.pawn.location + Vec3::new(20.0, 0.0, 60.0);
    sim.check_for_camera_collision(eye, sim.pc.rotation);
    assert_eq!(sim.pawn.mesh_offset_xy.x, 0.0);
}

#[allow(dead_code)]
fn _moves(_: Move) {}

/// Looking down against a wall: the eye (EyeJoint + swan neck, as the game's camera) must stay
/// on the near side of the wall once CheckForCameraCollision has pushed the mesh back.
#[test]
fn eye_stays_out_of_the_wall() {
    let Some((mut sim, mut pe)) = setup() else { return };
    sim.spawn(Vec3::new(600.0, 5950.0, 0.0), 0);
    let mut swan = me_level::camera::SwanNeck::default();
    let eye_of = |sim: &Sim, pe: &me_level::pose::PoseEvaluator, swan: &me_level::camera::SwanNeck| {
        let i = pe.bone_index("EyeJoint").unwrap();
        let m = me_level::pose::mesh_to_world(sim, tdsim::Rotator::new(0, -16384, 16384), [0.0, 94.0, 0.0]) * pe.globals()[i];
        let e = m.w_axis.truncate();
        let view = me_level::camera::camera_rotation(sim.pc.rotation, pe.camera_animation());
        Vec3::new(e.x, e.z, e.y) * 100.0 + swan.offset(view)
    };
    let mut worst = f32::MIN;
    for i in 0..60 * 5 {
        sim.tick(DT, InputFrame { forward: if i < 60 * 3 { 1.0 } else { 0.0 }, mouse_y: if i > 60 * 2 { -40.0 } else { 0.0 }, ..Default::default() });
        pe.update(&mut sim, DT);
        swan.update(&sim, DT);
        let eye = eye_of(&sim, &pe, &swan);
        let view = me_level::camera::camera_rotation(sim.pc.rotation, pe.camera_animation());
        sim.check_for_camera_collision(eye, view);
        if i > 60 * 4 {
            worst = worst.max(eye.x);
        }
    }
    let eye = eye_of(&sim, &pe, &swan);
    println!("eye {eye:?} worst x {worst} pitch {} mesh xy {:?} state {:?}", sim.pc.rotation.pitch, sim.pawn.mesh_offset_xy, sim.pawn.against_wall_state);
    // the wall face is at x = 1000; the near clip plane is 5 uu
    assert!(worst < 1000.0 - 5.0, "eye at {worst} sees through the wall");
}

fn plain_sim() -> Option<Sim> {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).ok()?;
    Some(Sim::new(tdsim::testmap::world(), Config::load(install), a.lib))
}

/// Lane D: the floor ends at x = 800 over a 1200-deep pit. Walking (not running) up to it
/// stops in TdMove_Vertigo with a zoom; crouching up to it stops at the edge. On a keyboard W
/// alone is the "up" move-action hint, which skips the edge clamp (as in the original), so the
/// approach is diagonal (W+D: the strafe hint wins).
#[test]
fn vertigo_and_crouch_edge_stop() {
    let Some(mut sim) = plain_sim() else { return };
    sim.spawn(Vec3::new(500.0, 3600.0, 0.0), 0);
    // TdMove_Vertigo.CanDoMove checks the *previous* edge is ahead (CheckForLedges stores the
    // new one only after it passes), so the very first one depends on where (0,0,0) is
    sim.moves.vertigo.last_vertigo_edge_position = Vec3::new(5000.0, 3600.0, 0.0);
    let mut states = vec![sim.pawn.movement_state];
    let mut min_fov = 90.0f32;
    for _ in 0..60 * 4 {
        sim.tick(DT, InputFrame { forward: 1.0, strafe: 0.5, walk: true, ..Default::default() });
        min_fov = min_fov.min(sim.pc.fov);
        if *states.last().unwrap() != sim.pawn.movement_state {
            states.push(sim.pawn.movement_state);
        }
    }
    println!("walk: {states:?} x {} fov {min_fov}", sim.pawn.location.x);
    assert!(states.contains(&Move::Vertigo), "{states:?}");
    assert!(sim.pawn.location.z > -50.0, "should not have fallen");
    assert!(min_fov < 89.0, "zoom in");

    let Some(mut sim) = plain_sim() else { return };
    sim.spawn(Vec3::new(500.0, 3600.0, 0.0), 0);
    for _ in 0..60 * 5 {
        sim.tick(DT, InputFrame { forward: 1.0, strafe: 0.5, crouch: true, ..Default::default() });
    }
    println!("crouch: {:?} at {:?}", sim.pawn.movement_state, sim.pawn.location);
    assert_eq!(sim.pawn.movement_state, Move::Crouch);
    assert!(sim.pawn.location.z > -50.0 && sim.pawn.location.x < 800.0, "crouch should stop at the edge");

    // running at it goes over
    let Some(mut sim) = plain_sim() else { return };
    sim.spawn(Vec3::new(300.0, 3600.0, 0.0), 0);
    for _ in 0..60 * 3 {
        sim.tick(DT, InputFrame { forward: 1.0, ..Default::default() });
    }
    println!("run: {:?} at {:?}", sim.pawn.movement_state, sim.pawn.location);
    assert!(sim.pawn.location.x > 800.0 || sim.pawn.location.z < -50.0, "running goes off the edge");
}

#[test]
#[ignore]
fn clamp_debug() {
    let Some(mut sim) = plain_sim() else { return };
    sim.spawn(Vec3::new(780.0, 3600.0, 0.0), 0);
    for _ in 0..20 { sim.tick(DT, InputFrame::default()); }
    sim.pawn.move_action_hint = tdsim::pawn::MoveActionHint::Right;
    sim.pawn.velocity = Vec3::new(120.0, 60.0, 0.0);
    println!("loc {:?} h {} r {} wfz {} msh {}", sim.pawn.location, sim.pawn.collision_height, sim.pawn.collision_radius, sim.pawn.walkable_floor_z, sim.pawn.max_step_height);
    for x in [760.0f32, 775.0, 785.0, 795.0] {
        sim.pawn.location.x = x;
        let d = Vec3::new(2.0, 1.0, 0.0);
        let (c, e) = sim.clamp_delta_to_edge(d);
        println!("x {x}: {d:?} -> {c:?} edge {e:?}");
    }
}

/// Landing hard with the cylinder's centre past a floor edge (lane A's side, y = 400): the
/// pawn is slid off (CheckValidFloor with bSlideOff) instead of standing on the rim.
#[test]
fn hard_landing_on_an_edge_slides_off() {
    let Some(mut sim) = plain_sim() else { return };
    sim.spawn(Vec3::new(1000.0, 412.0, 600.0), 0);
    let mut lowest = f32::MAX;
    for _ in 0..60 * 3 {
        sim.tick(DT, InputFrame::default());
        lowest = lowest.min(sim.pawn.location.z);
    }
    println!("end {:?} {:?} lowest {lowest}", sim.pawn.location, sim.pawn.movement_state);
    assert!(lowest < -50.0, "should have slid off the edge, stayed at {:?}", sim.pawn.location);
}

/// Against lane F's block (face x = 1000): both hands go onto the wall.
#[test]
fn hands_on_the_wall() {
    let Some((mut sim, mut pe)) = setup() else { return };
    sim.spawn(Vec3::new(600.0, 5950.0, 0.0), 0);
    let hand = |sim: &Sim, pe: &me_level::pose::PoseEvaluator, n: &str| {
        let m = me_level::pose::mesh_to_world(sim, tdsim::Rotator::new(0, -16384, 16384), [0.0, 94.0, 0.0]) * pe.globals()[pe.bone_index(n).unwrap()];
        let e = m.w_axis.truncate();
        Vec3::new(e.x, e.z, e.y) * 100.0
    };
    run(&mut sim, &mut pe, 30, InputFrame::default());
    let before = (hand(&sim, &pe, "LeftHand"), hand(&sim, &pe, "RightHand"));
    run(&mut sim, &mut pe, 60 * 3, InputFrame { forward: 1.0, ..Default::default() });
    run(&mut sim, &mut pe, 60, InputFrame::default());
    let (l, r) = (hand(&sim, &pe, "LeftHand"), hand(&sim, &pe, "RightHand"));
    println!("state {:?} loc {:?}\n hands before {before:?}\n left {l:?} (spot {:?})\n right {r:?} (spot {:?})", sim.pawn.against_wall_state, sim.pawn.location, sim.pawn.against_wall_left_hand, sim.pawn.against_wall_right_hand);
    assert_eq!(sim.pawn.against_wall_state, tdsim::body::AgainstWall::AgainstWall);
    assert!(l.x > 990.0 && r.x > 990.0, "hands should reach the wall face (x 1000)");
    assert!(l.y < r.y, "left hand on the left");
}

/// The third-person (shadow) body: AT_C3P "WalkRelaxed" turns its legs with LegRotation too.
#[test]
fn shadow_body_strafe_twist() {
    let install = me_level::install();
    let Some((mut sim, _)) = setup() else { return };
    let (set, skel) = me_level::anims::load_player_3p(install).unwrap();
    let aim3 = |n: &str| me_level::anims::load_aim_profile_in(install, "AT_C3P", n).unwrap();
    let mut pe = me_level::pose::PoseEvaluator::new(set, &skel).third_person(aim3("TdAnimNodeDirBone_15"), aim3("TdAnimNodeDirBone_7"), aim3("TdAnimNodeDirBone_0"));
    pe.drives_phase = false;
    sim.spawn(Vec3::new(1500.0, 0.0, 0.0), 0);
    run(&mut sim, &mut pe, 30, InputFrame::default());
    let still = toe_yaw(&sim, &pe);
    run(&mut sim, &mut pe, 50, InputFrame { strafe: 1.0, ..Default::default() });
    let toe = toe_yaw(&sim, &pe);
    println!("3p: still {still} strafe right {toe} leg {} loco {:?}", norm_axis(sim.pawn.leg_rotation - sim.pawn.rotation.yaw), pe.locomotion_debug());
    assert!((toe - still) > 8000, "3p toes should turn right");
}

#[test]
#[ignore]
fn look_speed_debug() {
    let Some(mut sim) = plain_sim() else { return };
    sim.spawn(Vec3::new(1500.0, 0.0, 0.0), 0);
    for _ in 0..20 { sim.tick(DT, InputFrame::default()); }
    for target in [0i32, -8000, -12000, -15000, 4000] {
        sim.pc.rotation.pitch = target;
        let (y0, p0) = (sim.pc.rotation.yaw, sim.pc.rotation.pitch);
        sim.tick(DT, InputFrame { mouse_x: 10.0, ..Default::default() });
        let dy = sim.pc.rotation.yaw - y0;
        sim.pc.rotation.pitch = target;
        sim.tick(DT, InputFrame { mouse_y: -10.0, ..Default::default() });
        let dp = sim.pc.rotation.pitch - target;
        println!("pitch {p0}: yaw step {dy} pitch step {dp} constrain {} {}", sim.pawn.constrain_look, sim.moves.base(sim.pawn.movement_state).constrain_look);
    }
}

#[test]
#[ignore]
fn wall_state_toggle_debug() {
    let Some(mut sim) = plain_sim() else { return };
    sim.spawn(Vec3::new(600.0, 6150.0, 0.0), 0);
    let mut log = Vec::new();
    for i in 0..60 * 5 {
        // run at the wall, then walk along it diagonally, then turn a bit
        let inp = if i < 120 { InputFrame { forward: 1.0, ..Default::default() } } else { InputFrame { forward: 1.0, strafe: 1.0, walk: true, mouse_x: if i < 150 { 5.0 } else { 0.0 }, ..Default::default() } };
        sim.tick(DT, inp);
        log.push(format!("{:?}", sim.pawn.against_wall_state).chars().next().unwrap());
    }
    println!("{}", log.iter().collect::<String>());
}

#[test]
#[ignore]
fn one_hand_debug() {
    let Some((mut sim, mut pe)) = setup() else { return };
    sim.spawn(Vec3::new(600.0, 6200.0, 0.0), 0);
    let mut s = String::new();
    for i in 0..60 * 5 {
        sim.tick(DT, InputFrame { forward: if i < 150 { 1.0 } else { 0.0 }, ..Default::default() });
        pe.update(&mut sim, DT);
        if i % 10 == 0 {
            let w = pe.wall_weights();
            s += &format!("{:?}:{:.2}/{:.2} ", sim.pawn.against_wall_state, w[0], w[1]);
        }
    }
    println!("{s}");
}

/// A standing turn step moves the legs only: the arms and the eye stay with the Stand pose.
#[test]
fn turn_step_keeps_arms_and_eye() {
    let Some((mut sim, mut pe)) = setup() else { return };
    sim.spawn(Vec3::new(1500.0, 0.0, 0.0), 0);
    run(&mut sim, &mut pe, 40, InputFrame::default());
    let local = |pe: &me_level::pose::PoseEvaluator, n: &str| pe.globals()[pe.bone_index(n).unwrap()].w_axis.truncate();
    let (h0, e0) = (local(&pe, "RightHand"), local(&pe, "EyeJoint"));
    let mut worst = (0.0f32, 0.0f32);
    let mut turned = false;
    for i in 0..90 {
        sim.tick(DT, InputFrame { mouse_x: if i < 20 { 60.0 } else { 0.0 }, ..Default::default() });
        pe.update(&mut sim, DT);
        turned |= sim.turn_node.playing_turn_animation;
        worst.0 = worst.0.max((local(&pe, "RightHand") - h0).length());
        worst.1 = worst.1.max((local(&pe, "EyeJoint") - e0).length());
    }
    // the same time standing still (the Stand clip breathes)
    let Some((mut sim2, mut pe2)) = setup() else { return };
    sim2.spawn(Vec3::new(1500.0, 0.0, 0.0), 0);
    run(&mut sim2, &mut pe2, 40, InputFrame::default());
    let (h1, e1) = (local(&pe2, "RightHand"), local(&pe2, "EyeJoint"));
    let mut base = (0.0f32, 0.0f32);
    for _ in 0..90 {
        sim2.tick(DT, InputFrame::default());
        pe2.update(&mut sim2, DT);
        base.0 = base.0.max((local(&pe2, "RightHand") - h1).length());
        base.1 = base.1.max((local(&pe2, "EyeJoint") - e1).length());
    }
    println!("turned {turned} hand moved {:.3} m eye moved {:.3} m; idle alone {:.3} / {:.3}", worst.0, worst.1, base.0, base.1);
    assert!(turned);
    assert!(worst.1 <= base.1 + 0.002 && worst.0 <= base.0 + 0.005, "the turn step shouldn't move the arms or eye");
}

/// Where the right hand sits in the camera's frame (x right, y up, z forward, uu) and the
/// screen spot it projects to (90 degree FOV, 16:9), for a few situations.
#[test]
#[ignore]
fn right_hand_view_debug() {
    let view_pos = |sim: &Sim, pe: &me_level::pose::PoseEvaluator| {
        let m = me_level::pose::mesh_to_world(sim, tdsim::Rotator::new(0, -16384, 16384), [0.0, 94.0, 0.0]);
        let w = |n: &str| { let e = (m * pe.globals()[pe.bone_index(n).unwrap()]).w_axis.truncate(); Vec3::new(e.x, e.z, e.y) * 100.0 };
        let (eye, hand) = (w("EyeJoint"), w("RightHand"));
        let view = me_level::camera::camera_rotation(sim.pc.rotation, pe.camera_animation());
        let (f, r, u) = view.axes();
        let d = hand - eye;
        let (x, y, z) = (d.dot(r), d.dot(u), d.dot(f));
        let sx = if z > 0.0 { x / z } else { 9.0 };
        let sy = if z > 0.0 { y / z * 16.0 / 9.0 } else { 9.0 };
        format!("view ({x:.0},{y:.0},{z:.0}) screen ({sx:.2},{sy:.2})")
    };
    let cases: Vec<(&str, Vec3, Box<dyn Fn(usize) -> InputFrame>)> = vec![
        ("free stand", Vec3::new(1500.0, 0.0, 0.0), Box::new(|_| InputFrame::default())),
        ("free stand look right", Vec3::new(1500.0, 0.0, 0.0), Box::new(|i| InputFrame { mouse_x: if i < 10 { 30.0 } else { 0.0 }, ..Default::default() })),
        ("wall corner left hand", Vec3::new(600.0, 6200.0, 0.0), Box::new(|i| InputFrame { forward: if i < 150 { 1.0 } else { 0.0 }, ..Default::default() })),
        ("wall corner, look right", Vec3::new(600.0, 6200.0, 0.0), Box::new(|i| InputFrame { forward: if i < 150 { 1.0 } else { 0.0 }, mouse_x: if (160..175).contains(&i) { 30.0 } else { 0.0 }, ..Default::default() })),
        ("wall both hands", Vec3::new(600.0, 5950.0, 0.0), Box::new(|i| InputFrame { forward: if i < 150 { 1.0 } else { 0.0 }, ..Default::default() })),
        ("jump land", Vec3::new(1500.0, 0.0, 0.0), Box::new(|i| InputFrame { jump: (10..14).contains(&i), ..Default::default() })),
    ];
    for (name, at, inp) in cases {
        let Some((mut sim, mut pe)) = setup() else { return };
        sim.spawn(at, 0);
        let mut s = String::new();
        for i in 0..300 {
            sim.tick(DT, inp(i));
            pe.update(&mut sim, DT);
            if i % 60 == 59 {
                s += &format!("\n   t{} {:?} w{:?} {}", i / 60, sim.pawn.against_wall_state, pe.wall_weights(), view_pos(&sim, &pe));
            }
        }
        println!("{name}:{s}");
    }
}

#[test]
#[ignore]
fn right_hand_mesh_debug() {
    for (name, at) in [("free", Vec3::new(1500.0, 0.0, 0.0)), ("corner", Vec3::new(600.0, 6200.0, 0.0))] {
        let Some((mut sim, mut pe)) = setup() else { return };
        sim.spawn(at, 0);
        for i in 0..240 {
            sim.tick(DT, InputFrame { forward: if i < 150 && name == "corner" { 1.0 } else { 0.0 }, ..Default::default() });
            pe.update(&mut sim, DT);
        }
        let g = pe.globals();
        let p = |n: &str| { let v = g[pe.bone_index(n).unwrap()].w_axis.truncate() * 100.0; (v.x as i32, v.y as i32, v.z as i32) };
        println!("{name}: state {:?} eye {:?} rhand {:?} rforearm {:?} rarm {:?} spinexr {:?} lhand {:?} cam {:?}", sim.pawn.against_wall_state, p("EyeJoint"), p("RightHand"), p("RightForeArm"), p("RightArm"), p("SpineXRight"), p("LeftHand"), pe.camera_animation());
    }
}

/// With only one hand on the wall (a corner), the other arm keeps its normal pose.
#[test]
fn one_hand_on_wall_leaves_the_other_arm() {
    let hands = |at: Vec3, forward: bool| {
        let (mut sim, mut pe) = setup().unwrap();
        sim.spawn(at, 0);
        for i in 0..240 {
            sim.tick(DT, InputFrame { forward: if i < 150 && forward { 1.0 } else { 0.0 }, ..Default::default() });
            pe.update(&mut sim, DT);
        }
        let g = pe.globals();
        let p = |n: &str| g[pe.bone_index(n).unwrap()].w_axis.truncate() * 100.0;
        (sim.pawn.against_wall_state, p("LeftHand"), p("RightHand"))
    };
    if setup().is_none() {
        return;
    }
    let (_, l0, r0) = hands(Vec3::new(1500.0, 0.0, 0.0), false);
    // left hand only: the block's +Y end (y 6200) is just right of the pawn
    // (10 in from the corner: spawning on the side plane is a tie between the two faces)
    let (s, l, r) = hands(Vec3::new(600.0, 6190.0, 0.0), true);
    assert_eq!(s, tdsim::body::AgainstWall::Left);
    assert!((r - r0).length() < 3.0, "right arm should stay put: {r:?} vs {r0:?}");
    assert!((l - l0).length() > 20.0, "left hand should be on the wall");
    // right hand only: the block's -Y end (y 5700) just left of the pawn
    let (s, l, r) = hands(Vec3::new(600.0, 5710.0, 0.0), true);
    assert_eq!(s, tdsim::body::AgainstWall::Right);
    assert!((l - l0).length() < 3.0, "left arm should stay put: {l:?} vs {l0:?}");
    assert!((r - r0).length() > 20.0, "right hand should be on the wall");
}

/// Sounds the pawn asks for: running footsteps from the locomotion notifies, a landing, the
/// slide loop.
#[test]
fn sound_events() {
    let Some((mut sim, mut pe)) = setup() else { return };
    sim.spawn(Vec3::new(-400.0, 4800.0, 0.0), 0);
    let mut log: Vec<(usize, String)> = Vec::new();
    for i in 0..60 * 5 {
        let inp = InputFrame {
            forward: 1.0,
            jump: (100..104).contains(&i),
            crouch: (200..230).contains(&i),
            ..Default::default()
        };
        sim.tick(DT, inp);
        pe.update(&mut sim, DT);
        for e in sim.events.drain(..) {
            if let tdsim::sim::Event::Sound(s) = e {
                log.push((i, format!("{s:?}")));
            }
        }
    }
    for (i, s) in &log {
        println!("{i:4} {s}");
    }
    let steps = log.iter().filter(|(_, s)| s.contains("Footstep { id: 3") || s.contains("Footstep { id: 4")).count();
    assert!(steps >= 5, "running should step: {steps}");
    assert!(log.iter().any(|(_, s)| s.contains("id: 8") || s.contains("id: 9")), "landing");
    assert!(log.iter().any(|(_, s)| s.contains("LoopStart { slot: Slide")), "slide loop");
    assert!(log.iter().any(|(_, s)| s.contains("Cloth")), "clothing notifies");
}

#[test]
#[ignore]
fn debug_right_hand_corner() {
    let (mut sim, mut pe) = setup().unwrap();
    sim.spawn(Vec3::new(600.0, 5700.0, 0.0), 0);
    for i in 0..240 {
        sim.tick(DT, InputFrame { forward: if i < 150 { 1.0 } else { 0.0 }, ..Default::default() });
        pe.update(&mut sim, DT);
        if i % 20 == 0 || i > 230 {
            println!("{i} {:?} {:?} {:?} v {:?}", sim.pawn.movement_state, sim.pawn.against_wall_state, sim.pawn.location, sim.pawn.velocity);
        }
    }
}
