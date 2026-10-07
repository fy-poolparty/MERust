#[test]
#[ignore]
fn root_translation() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    for n in ["VaultOver", "VaultOnto", "HangHeaveUp", "WallRunVertical", "CrouchSlide", "crouchstill", "fallinglandroll", "evaderoll", "jumpturnlanding", "jumpturnlandingidle", "runfwd", "Stand"] {
        let Some(s) = a.set.seqs.iter().find(|s| s.name.eq_ignore_ascii_case(n)) else { continue };
        let ti = a.set.track_bone_names.iter().position(|t| t == "root").unwrap();
        let hi = a.set.track_bone_names.iter().position(|t| t == "Hips").unwrap();
        let mut line = format!("{n:20}");
        for k in 0..5 {
            let t = s.length * k as f32 / 4.0;
            let (p, _) = s.sample(ti, t);
            let (h, _) = s.sample(hi, t);
            line += &format!(" r({:.0},{:.0},{:.0}) h({:.0},{:.0},{:.0})", p[0], p[1], p[2], h[0], h[1], h[2]);
        }
        println!("{line}");
    }
}
