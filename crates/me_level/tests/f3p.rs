#[test]
#[ignore]
fn f3p_names() {
    let c = &me_level::install().join("TdGame").join("CookedPC");
    let pkg = upk::Package::open(c.join("Animations").join("AS_F3P_Unarmed.upk")).unwrap();
    let idx = pkg.find_export("AS_F3P_Unarmed", None).unwrap();
    let set = upk::anim::read_anim_set(&pkg, idx).unwrap();
    let a = me_level::anims::load_player_anims(me_level::install()).unwrap();
    let f3: std::collections::HashSet<String> = set.seqs.iter().map(|s| s.name.to_ascii_lowercase()).collect();
    let missing: Vec<&String> = a.set.seqs.iter().map(|s| &s.name).filter(|n| !f3.contains(&n.to_ascii_lowercase())).collect();
    println!("f3p seqs {} tracks {} rotonly {} ; 1p seqs missing in f3p: {}", set.seqs.len(), set.track_bone_names.len(), set.anim_rotation_only, missing.len());
    println!("{:?}", missing);
}
