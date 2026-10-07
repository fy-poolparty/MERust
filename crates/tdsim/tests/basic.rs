use tdsim::anim::AnimLib;
use tdsim::collision::{Surface, WorldBuilder};
use tdsim::config::Config;
use tdsim::{InputFrame, Move, Physics, Sim, Vec3};

fn flat_sim() -> Sim {
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(20000.0, 20000.0, 50.0), 0.0, s);
    let cfg = Config::load(&install());
    let mut sim = Sim::new(b.build(), cfg, AnimLib::default());
    sim.spawn(Vec3::new(0.0, 0.0, 0.0), 0);
    sim
}

const DT: f32 = 1.0 / 60.0;

/// The install's ini files (MIRRORS_EDGE_DIR, else the folder the game remembered; tdsim can't
/// use me_level::find_install). Missing: Config::load falls back to the defaults.
fn install() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("MIRRORS_EDGE_DIR") {
        return p.into();
    }
    let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("XDG_CONFIG_HOME")).map(std::path::PathBuf::from).unwrap_or_default();
    std::fs::read_to_string(base.join("mirrors-edge-rust").join("install.txt")).map(|s| s.trim().into()).unwrap_or_default()
}

#[test]
fn settles_and_runs() {
    let mut sim = flat_sim();
    for _ in 0..30 {
        sim.tick(DT, InputFrame::default());
    }
    assert_eq!(sim.pawn.physics, Physics::Walking, "should land");
    assert_eq!(sim.pawn.movement_state, Move::Walking);
    let feet = sim.pawn.location.z - sim.pawn.collision_height;
    assert!(feet > 0.0 && feet < 2.5, "floor dist {feet}");
    let mut samples = Vec::new();
    for i in 0..(60 * 8) {
        sim.tick(DT, InputFrame { forward: 1.0, ..Default::default() });
        if i % 30 == 29 {
            samples.push((i as f32 * DT, sim.pawn.velocity.x));
        }
    }
    for (t, v) in &samples {
        println!("t={t:.2} v={v:.1}");
    }
    let v1 = samples.iter().find(|s| s.0 > 0.9).unwrap().1;
    let v8 = samples.last().unwrap().1;
    assert!(v1 > 400.0 && v1 < 600.0, "speed at 1s {v1}");
    assert!(v8 > 680.0 && v8 <= 721.0, "speed at 8s {v8}");
}

#[test]
fn jump_height() {
    let mut sim = flat_sim();
    for _ in 0..30 {
        sim.tick(DT, InputFrame::default());
    }
    let z0 = sim.pawn.location.z;
    sim.tick(DT, InputFrame { jump: true, ..Default::default() });
    assert_eq!(sim.pawn.movement_state, Move::Jump);
    let mut apex = z0;
    let mut t = 0.0;
    let mut land_t = 0.0;
    for _ in 0..200 {
        sim.tick(DT, InputFrame { jump: true, ..Default::default() });
        t += DT;
        apex = apex.max(sim.pawn.location.z);
        if sim.pawn.physics == Physics::Walking && land_t == 0.0 {
            land_t = t;
        }
    }
    println!("apex {:.1} land {:.2} state {:?}", apex - z0, land_t, sim.pawn.movement_state);
    // 630^2 / (4*800) = 124
    assert!((apex - z0 - 124.0).abs() < 8.0, "apex {}", apex - z0);
}
