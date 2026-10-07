//! Numbers for building ladders in the test map: exit root motion, hand heights, bag bounds.

#[test]
#[ignore]
fn ladder_numbers() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    for n in std::env::var("ANIMS").map(|v| v.split(',').map(String::from).collect::<Vec<_>>()).unwrap_or_else(|_| ["LadderExitTopLeftHand", "LadderExitTopRightHand", "LadderEnterTop", "LadderClimbUpLeftHand", "LadderClimbHangStart"].map(String::from).to_vec()) {
        let n = n.as_str();
        match a.lib.get(n).and_then(|s| s.root.clone()) {
            Some(r) => println!("{n}: len {:?} transl end {:?} yaw {:?}", a.lib.get(n).map(|s| s.length), r.translation.last(), r.yaw.last()),
            None => println!("{n}: no root"),
        }
    }
    let pe = me_level::pose::PoseEvaluator::new(a.set.clone(), &a.upper);
    let mut wb = tdsim::collision::WorldBuilder::default();
    let sf = wb.surface(tdsim::collision::Surface::default());
    wb.add_box(tdsim::Vec3::new(0.0, 0.0, -500.0), tdsim::Vec3::new(10.0, 10.0, 10.0), 0.0, sf);
    let sim = tdsim::Sim::new(wb.build(), tdsim::config::Config::load(install), a.lib.clone());
    let m = me_level::pose::mesh_to_world(&sim, a.lib.mesh_rot, a.upper.origin);
    for n in ["LadderClimbUpLeftHandStill", "LadderClimbUpRightHandStill"] {
        let g = pe.clip_globals(n, 0.0).unwrap();
        let at = |b: &str| { let t = (m * g[pe.bone_index(b).unwrap()]).w_axis; (t.x * 100.0, t.z * 100.0, t.y * 100.0) };
        println!("{n}: R {:?} L {:?} RFoot {:?} LFoot {:?} eye {:?}", at("RightHand"), at("LeftHand"), at("RightFoot"), at("LeftFoot"), at("EyeJoint"));
    }
    let pkg = upk::Package::open(&install.join(r"TdGame\CookedPC\Props\P_Renovation.upk")).unwrap();
    for n in ["S_ConstructionPackages_01a", "S_ConstructionPackages_01b", "S_ConstructionPackages_01c"] {
        let i = pkg.find_export(n, None).unwrap();
        let sm = upk::staticmesh::read_static_mesh(&pkg, i).unwrap();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &sm.positions { for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
        println!("{n}: verts {} tris {} col {} min {lo:?} max {hi:?}", sm.positions.len(), sm.indices.len() / 3, sm.collision.len());
    }
}

/// The tutorial's ladders: stored PawnLadderLocations vs the volume, and where the ladder
/// meshes sit.
#[test]
#[ignore]
fn tutorial_ladders() {
    let install = me_level::install();
    let pkg = upk::Package::open(&install.join(r"TdGame\CookedPC\Maps\SP00\Tutorial_p.me1")).unwrap();
    for name in ["TdLadderVolume_0", "TdLadderVolume_1", "TdLadderVolume_4", "TdLadderVolume_5"] {
        let idx = pkg.find_export(name, None).unwrap();
        let data = pkg.export_bytes(idx);
        let (props, _) = upk::export_props(&pkg, idx).unwrap();
        let pr = upk::find(&props, "PawnLadderLocations").unwrap();
        let n = i32::from_le_bytes(data[pr.start..pr.start + 4].try_into().unwrap()) as usize;
        let v = |k: usize| -> [f32; 3] {
            let o = pr.start + 4 + k * 12;
            std::array::from_fn(|c| f32::from_le_bytes(data[o + c * 4..o + c * 4 + 4].try_into().unwrap()))
        };
        println!("{name}: {n} locations first {:?} second {:?} last {:?}", v(0), v(1), v(n - 1));
    }
    for inst in upk::level::mesh_instances(&pkg) {
        if inst.mesh > 0 {
            let n = pkg.object_name(inst.mesh);
            if n.contains("Ladder") {
                let t = inst.transform;
                println!("{n}: origin {:?} x {:?} y {:?} z {:?}", t.point([0.0, 0.0, 0.0]), t.x, t.y, t.z);
            }
        }
    }
    for n in ["S_LadderSystem_01a", "S_LadderSystem_01b"] {
        let i = pkg.find_export(n, None).unwrap();
        let sm = upk::staticmesh::read_static_mesh(&pkg, i).unwrap();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &sm.positions { for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
        println!("{n}: min {lo:?} max {hi:?} col {}", sm.collision.len());
    }
}

#[test]
#[ignore]
fn props_render_info() {
    let install = me_level::install();
    let p = me_level::load_props(install, &tdsim::testmap::props(), 256).unwrap();
    for b in &p.level.batches {
        let m = &p.level.materials[b.material];
        println!("batch mat {} tris {} -> {:?}", b.material, b.indices.len() / 3, m);
    }
}

#[test]
#[ignore]
fn red_mi_params() {
    let install = me_level::install();
    let pkg = upk::Package::open(&install.join(r"TdGame\CookedPC\Props\P_Renovation.upk")).unwrap();
    let i = pkg.find_export("MI_ConstructionPackages_01_RED", None).unwrap();
    let data = pkg.export_bytes(i);
    let (props, _) = upk::export_props(&pkg, i).unwrap();
    for n in ["VectorParameterValues", "ScalarParameterValues"] {
        let pr = upk::find(&props, n).unwrap();
        let cnt = i32::from_le_bytes(data[pr.start..pr.start + 4].try_into().unwrap());
        let mut off = pr.start + 4;
        for _ in 0..cnt {
            let (cp, end) = upk::props::read_props(&pkg, data, off).unwrap();
            for p in &cp {
                let raw = &data[p.start..p.start + p.size.min(32)];
                println!("{n}: {} = {:?} raw {:?}", p.name, p.value, raw.chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap_or([0;4]))).collect::<Vec<_>>());
            }
            off = end;
        }
    }
}

#[test]
#[ignore]
fn f3p_has_new_clips() {
    let install = me_level::install();
    let (set, _) = me_level::anims::load_player_3p(install).unwrap();
    for n in ["LadderClimbUpLeftHandStill", "LadderClimbUpRightHandStill", "LadderClimbDownFast", "ladderclimbdownfast02", "ladderclimbdownfast03", "ladderclimblookleft", "ladderclimblookright", "fallinguncontrolled", "fallinguncontrolledbwd", "fallinglandintosoftlanding", "PipeClimbUpLeftHandStill"] {
        println!("{n}: {}", set.seqs.iter().any(|s| s.name.eq_ignore_ascii_case(n)));
    }
}

#[test]
#[ignore]
fn tutorial_ladder_collision() {
    let install = me_level::install();
    let pkg = upk::Package::open(&install.join(r"TdGame\CookedPC\Maps\SP00\Tutorial_p.me1")).unwrap();
    for inst in upk::level::mesh_instances(&pkg) {
        if inst.mesh > 0 && pkg.object_name(inst.mesh).contains("Ladder") {
            println!("{} collides {} visible {}", pkg.object_name(inst.mesh), inst.collides, inst.visible);
        }
    }
}

/// The tutorial's two pipes (TdLadderVolume_4/_5, LT_Pipe): every climb position, and the
/// meshes placed around them (to find the pipe pieces and how they line up with the positions).
#[test]
#[ignore]
fn tutorial_pipes() {
    let install = me_level::install();
    let pkg = upk::Package::open(&install.join(r"TdGame\CookedPC\Maps\SP00\Tutorial_p.me1")).unwrap();
    let mut all = Vec::new();
    for name in ["TdLadderVolume_4", "TdLadderVolume_5"] {
        let idx = pkg.find_export(name, None).unwrap();
        let data = pkg.export_bytes(idx);
        let (props, _) = upk::export_props(&pkg, idx).unwrap();
        let pr = upk::find(&props, "PawnLadderLocations").unwrap();
        let n = i32::from_le_bytes(data[pr.start..pr.start + 4].try_into().unwrap()) as usize;
        let v: Vec<[f32; 3]> = (0..n).map(|k| {
            let o = pr.start + 4 + k * 12;
            std::array::from_fn(|c| f32::from_le_bytes(data[o + c * 4..o + c * 4 + 4].try_into().unwrap()))
        }).collect();
        println!("{name}: {n} locations {v:?}");
        all.extend(v);
    }
    for inst in upk::level::mesh_instances(&pkg) {
        if inst.mesh <= 0 {
            continue;
        }
        let t = inst.transform;
        let o = t.point([0.0, 0.0, 0.0]);
        let near = all.iter().any(|p| ((p[0] - o[0]).powi(2) + (p[1] - o[1]).powi(2)).sqrt() < 250.0 && (p[2] - o[2]).abs() < 900.0);
        if near {
            let sm = upk::staticmesh::read_static_mesh(&pkg, inst.mesh as usize - 1).unwrap();
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in &sm.positions { for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
            println!("{} [{}]: origin {:?} x {:?} z {:?} local min {lo:?} max {hi:?} collides {} col {}", pkg.object_path(inst.mesh), inst.actor, o, t.x, t.z, inst.collides, sm.collision.len());
        }
    }
}

/// Static meshes near a point of Tutorial_p (env AT="x,y,z", R radius).
#[test]
#[ignore]
fn tutorial_meshes_near() {
    let install = me_level::install();
    let pkg = upk::Package::open(&install.join(r"TdGame\CookedPC\Maps\SP00\Tutorial_p.me1")).unwrap();
    let at: Vec<f32> = std::env::var("AT").unwrap().split(',').map(|v| v.trim().parse().unwrap()).collect();
    let r: f32 = std::env::var("R").ok().and_then(|v| v.parse().ok()).unwrap_or(400.0);
    for inst in upk::level::mesh_instances(&pkg) {
        if inst.mesh <= 0 {
            continue;
        }
        let t = inst.transform;
        let o = t.point([0.0, 0.0, 0.0]);
        let d = ((o[0] - at[0]).powi(2) + (o[1] - at[1]).powi(2) + (o[2] - at[2]).powi(2)).sqrt();
        if d < r {
            let sm = upk::staticmesh::read_static_mesh(&pkg, inst.mesh as usize - 1).unwrap();
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in &sm.positions { for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); } }
            println!("{} [{}]: origin {:?} x {:?} y {:?} z {:?} local min {lo:?} max {hi:?} collides {}", pkg.object_path(inst.mesh), inst.actor, o, t.x, t.y, t.z, inst.collides);
        }
    }
}

/// SplineLocations of Tutorial_p's movement volumes (env NAMES).
#[test]
#[ignore]
fn tutorial_spline_locations() {
    let install = me_level::install();
    let pkg = upk::Package::open(&install.join(r"TdGame\CookedPC\Maps\SP00\Tutorial_p.me1")).unwrap();
    for name in std::env::var("NAMES").unwrap_or("TdZiplineVolume_0,TdSwingVolume_0,TdSwingVolume_2".into()).split(',') {
        let idx = pkg.find_export(name, None).unwrap();
        let data = pkg.export_bytes(idx);
        let (props, _) = upk::export_props(&pkg, idx).unwrap();
        let pr = upk::find(&props, "SplineLocations").unwrap();
        let n = i32::from_le_bytes(data[pr.start..pr.start + 4].try_into().unwrap()) as usize;
        let v: Vec<[f32; 3]> = (0..n).map(|k| {
            let o = pr.start + 4 + k * 12;
            std::array::from_fn(|c| f32::from_le_bytes(data[o + c * 4..o + c * 4 + 4].try_into().unwrap()))
        }).collect();
        println!("{name}: {v:?}");
    }
}
