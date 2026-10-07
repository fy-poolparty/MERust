#[test]
#[ignore]
fn compare_ref_and_anim() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    let s = a.set.seqs.iter().find(|s| s.name.eq_ignore_ascii_case("Stand")).unwrap();
    for bn in ["root", "Hips", "Spine", "Spine1", "SpineX", "RightShoulder", "RightArm", "RightForeArm", "LeftUpLeg", "EyeJoint"] {
        let bi = a.upper.bones.iter().position(|b| b.name == bn).unwrap();
        let ti = a.set.track_bone_names.iter().position(|t| t == bn).unwrap();
        let (p, q) = s.sample(ti, 0.0);
        let b = &a.upper.bones[bi];
        println!("{bn:14} ref pos {:?} rot {:?}\n{:14} key pos {:?} rot {:?} (npos {} nrot {})", b.position, b.orientation, "", p, q, s.tracks[ti].pos.len(), s.tracks[ti].rot.len());
    }
}
