use tdsim::collision::{Surface, WorldBuilder};
use tdsim::Vec3;

#[test]
fn parallel_face_sweep() {
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(0.0, 0.0, -50.0), Vec3::new(5000.0, 5000.0, 50.0), 0.0, s);
    b.add_box(Vec3::new(700.0, 0.0, 125.0), Vec3::new(300.0, 500.0, 125.0), 0.0, s);
    let w = b.build();
    let ext = Vec3::new(30.0, 30.0, 90.0);
    for gap in [0.1f32, 0.05, 0.01, 0.0] {
        let x = 400.0 - 30.0 - gap;
        let start = Vec3::new(x, 0.0, 93.15);
        let down = w.line_check(start - Vec3::new(0.0, 0.0, 4.4), start, ext);
        let fwd = w.line_check(start + Vec3::new(3.0, 0.0, 0.0), start, ext);
        let up = w.line_check(start + Vec3::new(0.0, 0.0, 35.0), start, ext);
        println!("gap {gap}: down hit={} t={:.4} n={:?} | fwd hit={} t={:.4} n={:?} | up hit={} t={:.4} n={:?}",
            down.hit, down.time, down.normal, fwd.hit, fwd.time, fwd.normal, up.hit, up.time, up.normal);
    }
}

#[test]
fn winding_outward() {
    let mut b = WorldBuilder::default();
    let s = b.surface(Surface::default());
    b.add_box(Vec3::new(0.0, 0.0, 0.0), Vec3::new(50.0, 50.0, 50.0), 0.0, s);
    let mut up = 0;
    let mut down = 0;
    for t in &b.indices {
        let (a, bb, c) = (b.vertices[t[0] as usize], b.vertices[t[1] as usize], b.vertices[t[2] as usize]);
        let n = (bb - a).cross(c - a);
        let centre = (a + bb + c) / 3.0;
        if n.dot(centre) > 0.0 { up += 1 } else { down += 1 }
    }
    println!("outward {up} inward {down}");
}
