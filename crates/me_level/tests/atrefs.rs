#[test]
#[ignore]
fn who_references_hang45() {
    let p = &me_level::install().join("TdGame").join("CookedPC").join("Characters").join("AT_C1P.upk");
    let pkg = upk::Package::open(p).unwrap();
    let n = pkg.exports.len();
    let mut targets = Vec::new();
    for i in 0..n {
        if pkg.export_class(i) != "TdAnimNodeSequence" { continue; }
        let d = pkg.export_bytes(i);
        let (props, _) = upk::props::export_props(&pkg, i).unwrap();
        for pr in &props {
            if pr.name == "AnimSeqName" {
                if let upk::props::Value::Name(s) = &pr.value {
                    if s.to_ascii_lowercase().contains("hang") { targets.push((i, s.clone())); }
                }
            }
        }
        let _ = d;
    }
    for (t, s) in &targets {
        let needle = (*t as i32 + 1).to_le_bytes();
        for i in 0..n {
            let d = pkg.export_bytes(i);
            if d.windows(4).any(|w| w == needle) {
                println!("{s:24} <- {} {}", pkg.export_class(i), pkg.object_name(i as i32 + 1));
            }
        }
    }
}
