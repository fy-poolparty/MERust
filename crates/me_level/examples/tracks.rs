//! Print an AnimSet's track bone names: tracks <pkg> <set>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c = &me_level::install().join("TdGame").join("CookedPC").join("Animations");
    let p = upk::Package::open(c.join(format!("{}.upk", a[1]))).unwrap();
    let i = p.find_export(&a[2], None).unwrap();
    let s = upk::anim::read_anim_set(&p, i).unwrap();
    println!("{:?}", s.track_bone_names.iter().filter(|n| n.starts_with("Wep") || n.contains("Weapon")).collect::<Vec<_>>());
    for q in &s.seqs {
        let wi: Vec<usize> = s.track_bone_names.iter().enumerate().filter(|(_, n)| n.starts_with("Wep")).map(|(k, _)| k).collect();
        let keys: Vec<(usize, usize)> = wi.iter().map(|&k| (q.tracks[k].pos.len(), q.tracks[k].rot.len())).collect();
        println!("{} {:?}", q.name, keys);
    }
}
