//! Dump AT_C1P's TdAnimNodeWeaponPoseOffset profiles.
use upk::{export_props, find, struct_array, Package, Value};
fn main() {
    let c = &me_level::install().join("TdGame").join("CookedPC");
    let tree = std::env::var("TREE").unwrap_or("AT_C1P".into());
    let pkg = Package::open(c.join("Characters").join(format!("{tree}.upk"))).unwrap();
    let mesh = Package::open(c.join("Characters").join("CH_TKY_Crim_Fixer_1P.upk")).unwrap();
    let mi = mesh.find_export("SK_UpperBody", Some("SkeletalMesh")).unwrap();
    let (skel, _) = upk::skelmesh::read_skeleton(&mesh, mi).unwrap();
    let i = pkg.find_export("TdAnimNodeWeaponPoseOffset_0", None).unwrap();
    let (props, _) = export_props(&pkg, i).unwrap();
    let data = pkg.export_bytes(i);
    for which in ["WeaponPoseProfiles", "Profiles"] {
        let arr = find(&props, which).unwrap();
        for el in struct_array(&pkg, data, arr) {
            let name = el.iter().find(|p| p.name == "Name").map(|p| format!("{:?}", p.value)).unwrap_or_default();
            print!("{which} {name}:");
            for p in &el {
                match p.name.as_str() {
                    "BoneIndices" => {
                        let v = p.as_i32_array(data);
                        print!(" bones {:?}", v.iter().map(|&b| skel.bones.get(b as usize).map(|x| x.name.clone()).unwrap_or_default()).collect::<Vec<_>>());
                    }
                    "MatrixTransforms" => {
                        let b = p.bytes(data);
                        let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                        print!(" {n} matrices");
                        let f = |k: usize| f32::from_le_bytes([b[4 + k * 4], b[5 + k * 4], b[6 + k * 4], b[7 + k * 4]]);
                        if n > 0 && std::env::var("FULL").is_ok() {
                            println!();
                            for m in 0..(n as usize).min(4) {
                                let v: Vec<String> = (0..16).map(|k| format!("{:7.3}", f(m * 16 + k))).collect();
                                println!("    m{m}: {}", v.join(" "));
                            }
                        }
                    }
                    "BoneNames" | "AnimationPoseName" | "Recursive" | "AnimationSet" | "WeaponAnimationSet" => print!(" {}={:?}", p.name, if p.ty == "ArrayProperty" { format!("{:?}", p.as_i32_array(data)) } else { format!("{:?}", p.value) }),
                    _ => {}
                }
            }
            println!();
        }
    }
    let _ = Value::Int(0);
}
