//! Inspect a package from the user's install.
//!   upk_dump <pkg> summary            class histogram and table sizes
//!   upk_dump <pkg> list [class]       exports (optionally of one class)
//!   upk_dump <pkg> props <export>     tagged properties of an export (name or 1-based index)

use std::collections::BTreeMap;
use upk::{Package, Value, export_props};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let pkg = Package::open(&args[1])?;
    let cmd = args.get(2).map(String::as_str).unwrap_or("summary");
    match cmd {
        "summary" => {
            let s = &pkg.summary;
            println!(
                "{} v{}/{} engine {} cooker {} flags {:#x} compression {} chunks {}",
                pkg.name, s.file_version, s.licensee_version, s.engine_version, s.cooker_version,
                s.package_flags, s.compression_flags, s.chunks.len()
            );
            println!("names {} imports {} exports {}", pkg.names.len(), pkg.imports.len(), pkg.exports.len());
            let mut hist: BTreeMap<String, usize> = BTreeMap::new();
            for i in 0..pkg.exports.len() {
                *hist.entry(pkg.export_class(i)).or_default() += 1;
            }
            let mut v: Vec<_> = hist.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            for (c, n) in v {
                println!("{n:6} {c}");
            }
        }
        "skel" => {
            for i in 0..pkg.exports.len() {
                if pkg.export_class(i) != "SkeletalMesh" {
                    continue;
                }
                match upk::skelmesh::read_skeleton(&pkg, i) {
                    Ok((m, end)) => {
                        println!("{}: origin {:?} rot {:?} bones {} depth {} (next byte {end} of {})", m.name, m.origin, m.rot_origin, m.bones.len(), m.skeletal_depth, pkg.export_bytes(i).len());
                        for (bi, b) in m.bones.iter().enumerate() {
                            println!("  [{bi:3}] {:24} parent {:3} pos {:?} rot {:?}", b.name, b.parent, b.position, b.orientation);
                        }
                    }
                    Err(e) => println!("{}: {e}", pkg.object_name(i as i32 + 1)),
                }
            }
        }
        "anims" => {
            for i in 0..pkg.exports.len() {
                if !pkg.export_class(i).ends_with("AnimSet") {
                    continue;
                }
                let Some(set) = upk::anim::read_anim_set(&pkg, i) else {
                    println!("failed to read {}", pkg.object_name(i as i32 + 1));
                    continue;
                };
                println!("{}: {} tracks, {} sequences; bones {:?}", set.name, set.track_bone_names.len(), set.seqs.len(), &set.track_bone_names[..set.track_bone_names.len().min(8)]);
                for s in &set.seqs {
                    let rk: usize = s.tracks.iter().map(|t| t.rot.len()).sum();
                    let root = s.tracks.first().map(|t| (t.pos.len(), t.pos.first().copied(), t.pos.last().copied()));
                    println!("  {:28} len {:5.2} frames {:4} rate {:.2} rotkeys {:6} root {:?}", s.name, s.length, s.num_frames, s.rate_scale, rk, root);
                }
            }
        }
        "nested" => {
            // nested props of a struct property: nested <export> <prop>
            let idx = pkg.find_export(&args[3], None).ok_or("no such export")?;
            let (props, _) = export_props(&pkg, idx)?;
            let data = pkg.export_bytes(idx);
            let p = upk::props::find(&props, &args[4]).ok_or("no prop")?;
            let (inner, _) = upk::props::read_props(&pkg, data, p.start)?;
            for q in &inner {
                println!("  {} : {} {} size {}", q.name, q.ty, q.struct_name, q.size);
                if q.ty == "ArrayProperty" {
                    for (k, el) in upk::props::struct_array(&pkg, data, q).iter().enumerate() {
                        for e in el {
                            let b = &data[e.start.saturating_sub(16)..e.start + e.size];
                            println!("      size {} start {}", e.size, e.start); let fl: Vec<f32> = b.chunks(4).take(20).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
                            println!("    [{k}] {} : {} {} {:?} {:?}", e.name, e.ty, e.struct_name, e.value, fl);
                        }
                    }
                }
            }
        }
        "phys" => {
            let idx = pkg.find_export(&args[3], Some("PhysicsAsset")).ok_or("no such export")?;
            let pa = upk::physics::read_physics_asset(&pkg, idx)?;
            for b in &pa.bodies {
                println!("{}:", b.bone);
                for s in &b.shapes {
                    println!("   {s:?}");
                }
            }
            for c in &pa.constraints {
                println!("{c:?}");
            }
        }
        "imports" => {
            for i in 0..pkg.imports.len() {
                let idx = -(i as i32) - 1;
                println!("{idx:6} {:30} {}", pkg.class_of(idx), pkg.object_path(idx));
            }
        }
        "list" => {
            let filter = args.get(3);
            for (i, e) in pkg.exports.iter().enumerate() {
                let class = pkg.export_class(i);
                if filter.is_some_and(|f| !class.eq_ignore_ascii_case(f)) {
                    continue;
                }
                println!("{:6} {:30} {} ({} bytes)", i + 1, class, pkg.object_path(i as i32 + 1), e.serial_size);
            }
        }
        "props" => {
            let key = &args[3];
            let idx = match key.parse::<usize>() {
                Ok(n) => n - 1,
                Err(_) => pkg.find_export(key, None).ok_or("no such export")?,
            };
            let data = pkg.export_bytes(idx);
            println!("{} : {}", pkg.object_path(idx as i32 + 1), pkg.export_class(idx));
            let (props, end) = export_props(&pkg, idx)?;
            for p in &props {
                let v = match &p.value {
                    Value::Object(o) => format!("{} ({})", pkg.object_path(*o), pkg.class_of(*o)),
                    Value::Raw if p.struct_name == "Vector" => format!("{:?}", p.as_vec3(data)),
                    Value::Raw if p.struct_name == "Rotator" => format!("{:?}", p.as_rotator(data)),
                    Value::Raw if p.ty == "ArrayProperty" => {
                        let a = p.as_i32_array(data);
                        if a.len() <= 16 && !a.is_empty() {
                            let refs: Vec<String> = a.iter().map(|&o| pkg.object_name(o)).collect();
                            format!("[{}] {:?} {:?}", a.len(), a, refs)
                        } else if p.size <= 64 {
                            format!("<{:02x?}>", p.bytes(data))
                        } else {
                            format!("<{} bytes>", p.size)
                        }
                    }
                    Value::Raw if p.size <= 32 => format!("<{} {:02x?}>", p.struct_name, p.bytes(data)),
                    Value::Raw => format!("<{} {} bytes>", p.struct_name, p.size),
                    other => format!("{other:?}"),
                };
                let idx = if p.array_index != 0 { format!("[{}]", p.array_index) } else { String::new() };
                println!("  {}{} : {} = {}", p.name, idx, p.ty, v);
            }
            println!("native data: {} bytes after properties", data.len() - end);
        }
        "meshes" => {
            // Parse every StaticMesh export and report failures.
            let (mut ok, mut bad) = (0, 0);
            for i in 0..pkg.exports.len() {
                if pkg.export_class(i) != "StaticMesh" {
                    continue;
                }
                match upk::staticmesh::read_static_mesh(&pkg, i) {
                    Ok(m) => {
                        ok += 1;
                        if args.get(3).is_some_and(|a| a == "-v") {
                            println!(
                                "ok  {} verts {} tris {} sections {}",
                                pkg.object_path(i as i32 + 1),
                                m.positions.len(),
                                m.indices.len() / 3,
                                m.sections.len()
                            );
                        }
                    }
                    Err(e) => {
                        bad += 1;
                        println!("ERR {} : {e}", pkg.object_path(i as i32 + 1));
                    }
                }
            }
            println!("{ok} static meshes parsed, {bad} failed");
        }
        "level" => {
            let inst = upk::level::mesh_instances(&pkg);
            let mut tris = 0usize;
            let mut ctris = 0usize;
            let mut cache = std::collections::HashMap::new();
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for m in &inst {
                let mesh = cache
                    .entry(m.mesh)
                    .or_insert_with(|| upk::staticmesh::read_static_mesh(&pkg, m.mesh as usize - 1).ok());
                if let Some(mesh) = mesh {
                    tris += mesh.indices.len() / 3;
                    ctris += mesh.collision.len();
                    let p = m.transform.point(mesh.bounds_origin);
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
            }
            println!("{} instances of {} meshes, {tris} render tris, {ctris} collision tris", inst.len(), cache.len());
            println!("bounds {lo:?} .. {hi:?}");
            for i in 0..pkg.exports.len() {
                let c = pkg.export_class(i);
                if c.contains("PlayerStart") {
                    let (props, _) = export_props(&pkg, i)?;
                    let d = pkg.export_bytes(i);
                    let loc = upk::find(&props, "Location").and_then(|p| p.as_vec3(d));
                    let rot = upk::find(&props, "Rotation").and_then(|p| p.as_rotator(d));
                    println!("{c} {} at {loc:?} rot {rot:?}", pkg.object_path(i as i32 + 1));
                }
            }
        }
        "normals" => {
            // Compare stored vertex normals with face normals from the index winding.
            for i in 0..pkg.exports.len() {
                if pkg.export_class(i) != "StaticMesh" {
                    continue;
                }
                let Ok(m) = upk::staticmesh::read_static_mesh(&pkg, i) else { continue };
                let (mut sum, mut n) = (0.0f32, 0usize);
                for t in m.indices.chunks_exact(3) {
                    let p = |k: usize| m.positions[t[k] as usize];
                    let (a, b, c) = (p(0), p(1), p(2));
                    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                    let f = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                    let l = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
                    if l < 1e-4 {
                        continue;
                    }
                    let vn = m.normals[t[0] as usize];
                    sum += (f[0] * vn[0] + f[1] * vn[1] + f[2] * vn[2]) / l;
                    n += 1;
                }
                if n > 0 {
                    println!("{:+.2} {:6} {}", sum / n as f32, n, pkg.object_path(i as i32 + 1));
                }
            }
        }
        "big" => {
            // Largest placed instances (by world-space bounds extent).
            let inst = upk::level::mesh_instances(&pkg);
            let mut v = Vec::new();
            for m in &inst {
                let Ok(mesh) = upk::staticmesh::read_static_mesh(&pkg, m.mesh as usize - 1) else { continue };
                let e = m.transform.vector(mesh.bounds_extent);
                let r = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
                let mats: Vec<String> = mesh.sections.iter().map(|s| pkg.object_name(s.material)).collect();
                v.push((r, pkg.object_path(m.mesh), mats, m.actor.clone()));
            }
            v.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (r, mesh, mats, actor) in v.iter().take(15) {
                println!("{r:10.0} {mesh} {mats:?} {actor}");
            }
        }
        "textures" => {
            // Try to load every texture's largest mip <= 512 and report.
            let cooked = std::path::Path::new(&args[1]).ancestors().find(|p| p.ends_with("CookedPC")).ok_or("not under CookedPC")?;
            let mut files = upk::texture::FileIndex::new(cooked);
            let (mut ok, mut bad) = (0, 0);
            let mut fmts = BTreeMap::new();
            for i in 0..pkg.exports.len() {
                if pkg.export_class(i) != "Texture2D" {
                    continue;
                }
                let t = match upk::texture::read_texture(&pkg, i) {
                    Ok(t) => t,
                    Err(e) => {
                        bad += 1;
                        println!("ERR {} : {e}", pkg.object_path(i as i32 + 1));
                        continue;
                    }
                };
                *fmts.entry(format!("{:?}", t.format)).or_insert(0) += 1;
                let Some(mi) = t.mips.iter().position(|m| m.width <= 512 && m.height <= 512) else { continue };
                match t.mip_data(&pkg, mi, &mut files) {
                    Ok(d) if d.len() == t.format.mip_size(t.mips[mi].width, t.mips[mi].height) => ok += 1,
                    Ok(d) => {
                        bad += 1;
                        println!("SIZE {} got {} want {}", t.name, d.len(), t.format.mip_size(t.mips[mi].width, t.mips[mi].height));
                    }
                    Err(e) => {
                        bad += 1;
                        println!("ERR {} mip {mi}: {e}", t.name);
                    }
                }
            }
            println!("{ok} textures loaded, {bad} failed; formats {fmts:?}");
        }
        "skel" => {
            for i in 0..pkg.exports.len() {
                if pkg.export_class(i) != "SkeletalMesh" {
                    continue;
                }
                match upk::skelmesh::read_skeleton(&pkg, i) {
                    Ok((m, end)) => {
                        println!("{}: origin {:?} rot {:?} bones {} depth {} (next byte {end} of {})", m.name, m.origin, m.rot_origin, m.bones.len(), m.skeletal_depth, pkg.export_bytes(i).len());
                        for (bi, b) in m.bones.iter().enumerate() {
                            println!("  [{bi:3}] {:24} parent {:3} pos {:?} rot {:?}", b.name, b.parent, b.position, b.orientation);
                        }
                    }
                    Err(e) => println!("{}: {e}", pkg.object_name(i as i32 + 1)),
                }
            }
        }
        "anims" => {
            // AnimSequence name, length and rate (optionally filtered by substring).
            let filter = args.get(3).map(|s| s.to_ascii_lowercase());
            for i in 0..pkg.exports.len() {
                if pkg.export_class(i) != "AnimSequence" {
                    continue;
                }
                let Ok((props, _)) = export_props(&pkg, i) else { continue };
                let get = |n: &str| upk::find(&props, n).map(|p| p.value.clone());
                let name = match get("SequenceName") {
                    Some(Value::Name(n)) => n,
                    _ => pkg.name(pkg.exports[i].name),
                };
                if filter.as_ref().is_some_and(|f| !name.to_ascii_lowercase().contains(f.as_str())) {
                    continue;
                }
                let f = |v: Option<Value>, d: f32| match v {
                    Some(Value::Float(x)) => x,
                    _ => d,
                };
                let frames = match get("NumFrames") {
                    Some(Value::Int(n)) => n,
                    _ => 0,
                };
                println!("{:32} len {:6.3}s rate {:.2} frames {frames}", name, f(get("SequenceLength"), 0.0), f(get("RateScale"), 1.0));
            }
        }
        "names" => {
            let a: usize = args[3].parse()?;
            let b: usize = args[4].parse()?;
            for i in a..=b.min(pkg.names.len() - 1) {
                println!("{i:#x} {}", pkg.names[i]);
            }
        }
        "hex" => {
            let idx: usize = args[3].parse::<usize>()? - 1;
            let e = &pkg.exports[idx];
            println!("{} flags {:#018x} size {}", pkg.object_path(idx as i32 + 1), e.flags, e.serial_size);
            let data = pkg.export_bytes(idx);
            let n: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(128);
            for (i, w) in data[..n.min(data.len())].chunks(4).enumerate() {
                if w.len() < 4 {
                    break;
                }
                let v = i32::from_le_bytes([w[0], w[1], w[2], w[3]]);
                let f = f32::from_le_bytes([w[0], w[1], w[2], w[3]]);
                let name = pkg.names.get(v as usize).map(String::as_str).unwrap_or("");
                println!("{:5} {:02x?} {:12} {:14.4} {}", i * 4, w, v, f, name);
            }
        }
        _ => return Err(format!("unknown command {cmd}").into()),
    }
    Ok(())
}
