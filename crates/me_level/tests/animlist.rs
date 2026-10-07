#[test]
#[ignore]
fn list_anims() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    println!("rotation_only {} tracks {}", a.set.anim_rotation_only, a.set.track_bone_names.len());
    let mut names: Vec<String> = a.set.seqs.iter().map(|s| format!("{}({:.2})", s.name, s.length)).collect();
    names.sort();
    println!("{}", names.join(" "));
}
