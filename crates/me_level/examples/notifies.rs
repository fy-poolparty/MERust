//! Print notifies of sequences matching a filter: notifies <pkg> <set> <filter>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c = &me_level::install().join("TdGame").join("CookedPC").join("Animations");
    let p = upk::Package::open(c.join(format!("{}.upk", a[1]))).unwrap();
    let i = p.find_export(&a[2], None).unwrap();
    let s = upk::anim::read_anim_set(&p, i).unwrap();
    for q in s.seqs.iter().filter(|q| q.name.to_lowercase().contains(&a[3].to_lowercase())) {
        println!("{} len {:.2}", q.name, q.length);
        for n in &q.notifies {
            println!("   {:.3} {:?}", n.time, n.kind);
        }
    }
}
