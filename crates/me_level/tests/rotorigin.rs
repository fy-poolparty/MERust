#[test]
#[ignore]
fn print_rot_origin() {
    let install = me_level::install();
    let a = me_level::anims::load_player_anims(install).unwrap();
    println!("upper rot_origin {:?} origin {:?} lib mesh_rot {:?}", a.upper.rot_origin, a.upper.origin, a.lib.mesh_rot);
}
