use tdsim::collision::{Surface, WorldBuilder};
use tdsim::config::Config;
use tdsim::{InputFrame, Sim, Vec3};

#[test]
fn eye_joint_height() {
    let install = me_level::install();
    let Ok(a) = me_level::anims::load_player_anims(install) else { return };
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(5000.0, 5000.0, 50.0), 0.0, s);
    let mut sim = Sim::new(b.build(), Config::load(install), a.lib.clone());
    sim.spawn(Vec3::ZERO, 0);
    for _ in 0..30 {
        sim.tick(1.0 / 60.0, InputFrame::default());
    }
    let mut pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    pe.update(&mut sim, 0.0);
    let m = me_level::pose::mesh_to_world(&sim, a.lib.mesh_rot, a.upper.origin);
    let g = pe.globals();
    for name in ["EyeJoint", "CameraJoint", "Head", "Hips", "RightHand", "LeftFoot"] {
        let i = pe.bone_index(name).unwrap();
        let w = m * g[i];
        let t = w.w_axis;
        // Bevy (x, y up, z) metres -> UE (x, z, y) uu relative to the pawn centre
        let p = sim.pawn.location;
        println!("{name:12} ue rel ({:7.1} {:7.1} {:7.1})", t.x * 100.0 - p.x, t.z * 100.0 - p.y, t.y * 100.0 - p.z);
    }
}

#[test]
#[ignore]
fn hands_vs_eye_running() {
    let install = me_level::install();
    let Ok(a) = me_level::anims::load_player_anims(install) else { return };
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(50000.0, 5000.0, 50.0), 0.0, s);
    let mut sim = Sim::new(b.build(), Config::load(install), a.lib.clone());
    sim.spawn(Vec3::ZERO, 0);
    let mut pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    for i in 0..240 {
        sim.tick(1.0 / 60.0, InputFrame { forward: 1.0, ..Default::default() });
        pe.update(&mut sim, 1.0 / 60.0);
        if i % 8 == 0 && i > 150 {
            let m = me_level::pose::mesh_to_world(&sim, a.lib.mesh_rot, a.upper.origin);
            let g = pe.globals();
            let at = |n: &str| { let t = (m * g[pe.bone_index(n).unwrap()]).w_axis; (t.x * 100.0, t.z * 100.0, t.y * 100.0) };
            let e = at("EyeJoint");
            let r = at("RightHand");
            let l = at("LeftHand");
            let ca = pe.camera_animation();
            println!("cam anim p {} y {} r {}", ca.pitch, ca.yaw, ca.roll);
            println!("{i} ws {:?} eye z {:.1}  R rel ({:.0},{:.0},{:.0})  L rel ({:.0},{:.0},{:.0})", sim.pawn.current_walking_state, e.2 - sim.pawn.location.z,
                r.0 - e.0, r.1 - e.1, r.2 - e.2, l.0 - e.0, l.1 - e.1, l.2 - e.2);
        }
    }
}

#[test]
#[ignore]
fn feet_on_floor_crouch_slide() {
    let install = me_level::install();
    let Ok(a) = me_level::anims::load_player_anims(install) else { return };
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(50000.0, 5000.0, 50.0), 0.0, s);
    let mut sim = Sim::new(b.build(), Config::load(install), a.lib.clone());
    sim.spawn(Vec3::ZERO, 0);
    let mut pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    for i in 0..260 {
        let crouch = (90..200).contains(&i);
        sim.tick(1.0 / 60.0, InputFrame { forward: if i < 140 { 1.0 } else { 0.0 }, crouch, ..Default::default() });
        pe.update(&mut sim, 1.0 / 60.0);
        if i % 15 == 0 {
            let m = me_level::pose::mesh_to_world(&sim, a.lib.mesh_rot, a.upper.origin);
            let g = pe.globals();
            let z = |n: &str| (m * g[pe.bone_index(n).unwrap()]).w_axis.y * 100.0;
            println!("{i:3} {:?} toeL {:.1} toeR {:.1} footL {:.1} hips {:.1} eye {:.1}", sim.pawn.movement_state,
                z("LeftToeBase"), z("RightToeBase"), z("LeftFoot"), z("Hips"), z("EyeJoint"));
        }
    }
}

#[test]
#[ignore]
fn camera_anim_skill_roll() {
    let install = me_level::install();
    let Ok(a) = me_level::anims::load_player_anims(install) else { return };
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(5000.0, 5000.0, 50.0), 0.0, s);
    let mut sim = Sim::new(b.build(), Config::load(install), a.lib.clone());
    sim.spawn(Vec3::ZERO, 0);
    let mut pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    sim.tick(1.0 / 60.0, InputFrame::default());
    sim.anim.play(tdsim::pawn::Slot::FullBody, "fallinglandroll", 1.0, 0.0, 0.2, false, false, false);
    let mut line = String::new();
    for i in 0..76 {
        pe.update(&mut sim, 1.0 / 60.0);
        sim.anim.tick(1.0 / 60.0, &sim.pawn.clone());
        if i % 6 == 0 {
            let r = me_level::camera::camera_rotation(tdsim::Rotator::ZERO, pe.camera_animation());
            line += &format!(" p{}", r.pitch);
        }
    }
    println!("view pitch over roll:{line}");
}

#[test]
#[ignore]
fn hang_clip_hands() {
    let install = me_level::install();
    let Ok(a) = me_level::anims::load_player_anims(install) else { return };
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(5000.0, 5000.0, 50.0), 0.0, s);
    let mut sim = Sim::new(b.build(), Config::load(install), a.lib.clone());
    sim.spawn(Vec3::ZERO, 0);
    let pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    let m = me_level::pose::mesh_to_world(&sim, a.lib.mesh_rot, a.upper.origin);
    for clip in ["HangFree", "HangFreeTurnRight", "HangFreeHardStart", "HangFreeStrafe"] {
        for ph in [0.0, 0.25, 0.5, 0.75, 0.99] {
            let Some(g) = pe.clip_globals(clip, ph) else { continue };
            let at = |n: &str| { let t = (m * g[pe.bone_index(n).unwrap()]).w_axis; (t.x * 100.0 - sim.pawn.location.x, t.z * 100.0, t.y * 100.0 - sim.pawn.location.z) };
            let (r, l, e) = (at("RightHand"), at("LeftHand"), at("EyeJoint"));
            println!("{clip:20} {ph:.2} R ({:.0},{:.0},{:.0}) L ({:.0},{:.0},{:.0}) eye ({:.0},{:.0},{:.0})", r.0, r.1, r.2, l.0, l.1, l.2, e.0, e.1, e.2);
        }
    }
}

/// Hands / eye of the hang clips in pawn space (X toward the wall, Y right, Z up), and the
/// eye's facing yaw, to see which way each clip turns the body.
#[test]
#[ignore]
fn hang_clip_frames() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    let pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    let mut wb = tdsim::collision::WorldBuilder::default();
    let sf = wb.surface(tdsim::collision::Surface::default());
    wb.add_box(tdsim::Vec3::new(0.0, 0.0, -500.0), tdsim::Vec3::new(10.0, 10.0, 10.0), 0.0, sf);
    let mut sim = tdsim::Sim::new(wb.build(), tdsim::config::Config::load(install), a.lib.clone());
    sim.pawn.location = tdsim::Vec3::ZERO;
    sim.pawn.rotation = tdsim::Rotator::ZERO;
    let m = me_level::pose::mesh_to_world(&sim, a.lib.mesh_rot, a.upper.origin);
    let names: Vec<String> = std::env::var("CLIPS").map(|v| v.split(",").map(|s| s.to_string()).collect()).unwrap_or(vec!["Hang".into(), "HangTurnRightIdle".into()]);
    let names: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    for n in names {
        for ph in [0.0f32, 0.5] {
            let Some(g) = pe.clip_globals(n, ph) else { println!("{n}: missing"); continue };
            let at = |b: &str| { let t = (m * g[pe.bone_index(b).unwrap()]).w_axis; tdsim::Vec3::new(t.x * 100.0, t.z * 100.0, t.y * 100.0) };
            let eye_m = m * g[pe.bone_index("EyeJoint").unwrap()];
            // pick the bone axis that is most horizontal-forward in the Hang clip: print all three
            let ax = |c: glam::Vec4| (c.x, c.z, c.y);
            let fmt = |v: tdsim::Vec3| format!("({:.0},{:.0},{:.0})", v.x, v.y, v.z);
            let e = at("EyeJoint");
            println!("{n:22} ph {ph}: eye {} R {} L {} eyeaxes x{:.2?} y{:.2?} z{:.2?}", fmt(e), fmt(at("RightHand") - e), fmt(at("LeftHand") - e),
                ax(eye_m.x_axis), ax(eye_m.y_axis), ax(eye_m.z_axis));
        }
    }
}

/// GetCameraAnimation candidates per clip: FMatrix::Rotator and the exe's own decomposition
/// (0x12B0670) for EyeJoint and CameraJoint, in Unreal mesh space.
#[test]
#[ignore]
fn camera_anim_decomp() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    let pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    let s = |v: glam::Vec3| glam::Vec3::new(v.x, v.z, v.y);
    let k = 32768.0 / std::f32::consts::PI;
    let clips = std::env::var("CLIPS").unwrap_or("Stand,runfwd,HangFree,Hang,HangFreeTurnRight,HangFreeHardStart,HangFreeFoldedEndHangFree,HangTurnRightIdle".into());
    for n in clips.split(',') {
        for ph in [0.0f32, 0.25, 0.5, 0.75] {
            let Some(g) = pe.clip_globals(n, ph) else { println!("{n} missing"); break };
            let mut line = format!("{n:26} {ph:.2}:");
            for b in ["EyeJoint", "CameraJoint"] {
                let m = g[pe.bone_index(b).unwrap()];
                let (ux, uy, uz) = (s(m.x_axis.truncate()).normalize(), s(m.z_axis.truncate()).normalize(), s(m.y_axis.truncate()).normalize());
                let f = me_level::pose::rotator_from_axes(ux, uy, uz);
                let exe = ((-uz.x).atan2(uz.z) * k) as i32;
                let exe_y = (ux.y.atan2(uy.y) * k) as i32;
                let exe_r = (uz.y.clamp(-1.0, 1.0).asin() * k) as i32;
                line += &format!("  {b}: fm ({},{},{}) exe ({},{},{})", f.pitch, f.yaw, f.roll, exe, exe_y, exe_r);
            }
            println!("{line}");
        }
    }
}

/// Running with the view pitched (TdSkelControlAim1p, both hands while jogging or faster).
#[test]
#[ignore]
fn hands_in_view_running_pitched() {
    let install = me_level::install();
    let Ok(a) = me_level::anims::load_player_anims(install) else { return };
    for pitch in [0i32, -7000, 5000] {
        let mut b = WorldBuilder::default();
        let s = b.surface(Surface::default());
        b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(50000.0, 5000.0, 50.0), 0.0, s);
        let mut sim = Sim::new(b.build(), Config::load(install), a.lib.clone());
        sim.spawn(Vec3::ZERO, 0);
        let mut pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
        for i in 0..200 {
            sim.pc.rotation.pitch = pitch;
            sim.tick(1.0 / 60.0, InputFrame { forward: 1.0, ..Default::default() });
            pe.update(&mut sim, 1.0 / 60.0);
            if i % 20 == 19 && i > 150 {
                let m = me_level::pose::mesh_to_world(&sim, a.lib.mesh_rot, a.upper.origin);
                let g = pe.globals();
                let at = |n: &str| { let t = (m * g[pe.bone_index(n).unwrap()]).w_axis; Vec3::new(t.x * 100.0, t.z * 100.0, t.y * 100.0) };
                let e = at("EyeJoint");
                let (vx, vy, vz) = sim.pc.rotation.axes();
                let r = at("RightHand") - e;
                let l = at("LeftHand") - e;
                println!("pitch {pitch} ws {:?} aim {:?} R view ({:.0} {:.0} {:.0}) L view ({:.0} {:.0} {:.0})", sim.pawn.current_walking_state, sim.aim_mode(true),
                    r.dot(vx), r.dot(vy), r.dot(vz), l.dot(vx), l.dot(vy), l.dot(vz));
            }
        }
    }
}
