#[test]
#[ignore]
fn print_root_yaw() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    for n in ["ladderclimblookright", "ladderclimblookleft", "LadderClimbUpLeftHandStill", "HangTurnRightIdle", "hangturnrightidle02"] {
        match a.lib.get(n).and_then(|s| s.root.clone()) {
            Some(r) => println!("{n}: len {:?} yaw keys {} total {:?} transl end {:?}", a.lib.get(n).map(|s| s.length), r.yaw.len(), r.yaw.last(), r.translation.last()),
            None => println!("{n}: missing"),
        }
    }
}
