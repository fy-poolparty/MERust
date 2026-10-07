#[test]
#[ignore]
fn tutorial_winding() {
    let install = me_level::install();
    let level = me_level::load_map(install, me_level::TUTORIAL, 64).unwrap();
    let (v, i) = level.collision.triangles();
    let (mut up, mut down) = (0, 0);
    for t in &i {
        let (a, b, c) = (v[t[0] as usize], v[t[1] as usize], v[t[2] as usize]);
        let n = (b - a).cross(c - a);
        if n.length() < 1e-3 { continue; }
        let nz = n.z / n.length();
        if nz > 0.9 { up += 1 } else if nz < -0.9 { down += 1 }
    }
    println!("horizontal up {up} down {down}");
}

#[test]
#[ignore]
fn floor_under_start() {
    let install = me_level::install();
    let level = me_level::load_map(install, me_level::TUTORIAL, 64).unwrap();
    let (v, idx) = level.collision.triangles();
    for st in level.starts.iter().take(5) {
        let p = st.location;
        let mut best: Option<(f32, f32)> = None;
        for t in &idx {
            let (a, b, c) = (v[t[0] as usize], v[t[1] as usize], v[t[2] as usize]);
            let n = (b - a).cross(c - a);
            if n.z.abs() < 1e-3 { continue; }
            // barycentric in XY
            let d = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
            let l1 = ((b.y - c.y) * (p.x - c.x) + (c.x - b.x) * (p.y - c.y)) / d;
            let l2 = ((c.y - a.y) * (p.x - c.x) + (a.x - c.x) * (p.y - c.y)) / d;
            let l3 = 1.0 - l1 - l2;
            if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 { continue; }
            let z = l1 * a.z + l2 * b.z + l3 * c.z;
            if z < p.z && best.map_or(true, |(bz, _)| z > bz) { best = Some((z, n.z / n.length())); }
        }
        println!("start {:?}: floor z/nz {:?}", p, best);
    }
}
