use upk::props::{read_props, export_props, find, Value};

fn dump(pkg: &upk::Package, data: &[u8], start: usize, depth: usize) -> usize {
    let (props, end) = read_props(pkg, data, start).unwrap();
    for p in &props {
        let ind = "  ".repeat(depth);
        match &p.value {
            Value::Raw if p.ty == "ArrayProperty" => {
                let n = i32::from_le_bytes(data[p.start..p.start + 4].try_into().unwrap());
                println!("{ind}{} [{}] ({} bytes)", p.name, n, p.size);
                if p.name == "AimComponents" || p.name == "Profiles" {
                    let mut off = p.start + 4;
                    for k in 0..n.min(40) {
                        println!("{ind}  #{k}");
                        off = dump(pkg, data, off, depth + 2);
                    }
                }
            }
            Value::Raw if p.ty == "StructProperty" && p.size > 32 => {
                println!("{ind}{} : {}", p.name, p.struct_name);
                if p.struct_name == "AimTransform" {
                    dump(pkg, data, p.start, depth + 2);
                }
            }
            Value::Raw => {
                let v: Vec<String> = (0..p.size / 4).map(|i| format!("{:.3}", f32::from_le_bytes(data[p.start + i * 4..p.start + i * 4 + 4].try_into().unwrap()))).collect();
                println!("{ind}{} : {} {:?}", p.name, p.struct_name, v);
            }
            v => println!("{ind}{} = {:?}", p.name, v),
        }
    }
    end
}

#[test]
#[ignore]
fn aim_offset_profiles() {
    let pp = pkg_path(); let p = std::path::Path::new(&pp);
    let pkg = upk::Package::open(p).unwrap();
    let name = std::env::var("AIMNODE").unwrap_or("TdAnimNodeAimOffset_6".into());
    let idx = pkg.find_export(&name, None).unwrap();
    let data = pkg.export_bytes(idx);
    let (props, _) = export_props(&pkg, idx).unwrap();
    let pr = find(&props, "Profiles").unwrap();
    let n = i32::from_le_bytes(data[pr.start..pr.start + 4].try_into().unwrap());
    println!("{name}: {n} profiles");
    let mut off = pr.start + 4;
    for k in 0..n {
        println!("== profile {k}");
        off = dump(&pkg, data, off, 1);
    }
}

#[test]
#[ignore]
fn node_children() {
    let pp = pkg_path(); let p = std::path::Path::new(&pp);
    let pkg = upk::Package::open(p).unwrap();
    let name = std::env::var("NODE").unwrap_or("TdAnimNodeGrabbing_0".into());
    let idx = pkg.find_export(&name, None).unwrap();
    let data = pkg.export_bytes(idx);
    let (props, _) = export_props(&pkg, idx).unwrap();
    let pr = find(&props, "Children").unwrap();
    let n = i32::from_le_bytes(data[pr.start..pr.start + 4].try_into().unwrap());
    let mut off = pr.start + 4;
    for k in 0..n {
        let (cp, end) = read_props(&pkg, data, off).unwrap();
        off = end;
        let cname = cp.iter().find(|p| p.name == "Name").map(|p| format!("{:?}", p.value)).unwrap_or_default();
        let anim = cp.iter().find(|p| p.name == "Anim").and_then(|p| if let Value::Object(o) = p.value { Some(o) } else { None });
        let mut desc = String::new();
        if let Some(o) = anim {
            desc = format!("{} ({})", pkg.object_name(o), pkg.class_of(o));
            if o > 0 {
                if let Ok((ap, _)) = export_props(&pkg, o as usize - 1) {
                    if let Some(s) = ap.iter().find(|p| p.name == "AnimSeqName") { desc += &format!(" seq {:?}", s.value); }
                }
            }
        }
        println!("child {k}: {cname} -> {desc}");
    }
}

fn node_line(pkg: &upk::Package, o: i32) -> String {
    let mut desc = format!("{} ({})", pkg.object_name(o), pkg.class_of(o));
    if o > 0 {
        if let Ok((ap, _)) = export_props(pkg, o as usize - 1) {
            for key in ["AnimSeqName", "NodeName"] {
                if let Some(s) = ap.iter().find(|p| p.name == key) { desc += &format!(" {key}={:?}", s.value); }
            }
        }
    }
    desc
}

fn tree(pkg: &upk::Package, o: i32, depth: usize, seen: &mut std::collections::HashSet<i32>) {
    if o <= 0 || !seen.insert(o) { return; }
    let idx = o as usize - 1;
    let data = pkg.export_bytes(idx);
    let Ok((props, _)) = export_props(pkg, idx) else { return };
    let Some(pr) = find(&props, "Children") else { return };
    let n = i32::from_le_bytes(data[pr.start..pr.start + 4].try_into().unwrap());
    let mut off = pr.start + 4;
    for _ in 0..n {
        let Ok((cp, end)) = read_props(pkg, data, off) else { return };
        off = end;
        let cname = cp.iter().find(|p| p.name == "Name").map(|p| format!("{:?}", p.value)).unwrap_or_default();
        let anim = cp.iter().find(|p| p.name == "Anim").and_then(|p| if let Value::Object(o) = p.value { Some(o) } else { None });
        if let Some(a) = anim {
            println!("{}{cname} -> {}", "  ".repeat(depth), node_line(pkg, a));
            tree(pkg, a, depth + 1, seen);
        }
    }
}

#[test]
#[ignore]
fn anim_tree() {
    let pp = pkg_path(); let p = std::path::Path::new(&pp);
    let pkg = upk::Package::open(p).unwrap();
    let name = std::env::var("NODE").unwrap_or("AT_C1P".into());
    let idx = pkg.find_export(&name, None).unwrap();
    tree(&pkg, idx as i32 + 1, 0, &mut Default::default());
}

#[test]
#[ignore]
fn name_lookup() {
    let pp = pkg_path(); let p = std::path::Path::new(&pp);
    let pkg = upk::Package::open(p).unwrap();
    for i in [0x1C1usize, 0x151] {
        println!("{i:#x} = {:?}", pkg.names[i]);
    }
}

#[test]
#[ignore]
fn skel_controls() {
    let pp = pkg_path(); let p = std::path::Path::new(&pp);
    let pkg = upk::Package::open(p).unwrap();
    let idx = pkg.find_export(&std::env::var("PKG").unwrap_or("AT_C1P".into()), None).unwrap();
    let data = pkg.export_bytes(idx);
    let (props, _) = export_props(&pkg, idx).unwrap();
    let pr = find(&props, "SkelControlLists").unwrap();
    for head in upk::props::struct_array(&pkg, data, pr) {
        let bone = head.iter().find(|p| p.name == "BoneName").map(|p| format!("{:?}", p.value)).unwrap_or_default();
        let mut c = head.iter().find(|p| p.name == "ControlHead").and_then(|p| if let Value::Object(o) = p.value { Some(o) } else { None });
        println!("bone {bone}");
        while let Some(o) = c.filter(|o| *o > 0) {
            println!("  {} ({})", pkg.object_name(o), pkg.class_of(o));
            let (cp, _) = export_props(&pkg, o as usize - 1).unwrap();
            for q in &cp {
                if std::env::var("ALL").is_ok() || ["ControlName", "BlendInTime", "BlendOutTime", "JointTargetLocation", "EffectorLocation", "bInvertBoneAxis", "bLeftHand", "JointTargetLocationSpace", "EffectorLocationSpace", "BoneAxis", "JointAxis", "bMaintainEffectorRelRot", "MinLocation", "MaxLocation", "HandOffset", "TargetLocation"].contains(&q.name.as_str()) {
                    println!("     {} = {:?}", q.name, q.value);
                }
            }
            c = cp.iter().find(|p| p.name == "NextControl").and_then(|p| if let Value::Object(o) = p.value { Some(o) } else { None });
        }
    }
}

fn pkg_path() -> String {
    let pk = std::env::var("PKG").unwrap_or("AT_C1P".into());
    me_level::install().join("TdGame").join("CookedPC").join("Characters").join(format!("{pk}.upk")).to_string_lossy().into_owned()
}
