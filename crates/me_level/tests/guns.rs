//! Lane K with armed patrol cops: shooting, disarming, the player's gun.

use std::path::Path;
use tdsim::config::Config;
use tdsim::math::UeVec;
use tdsim::{InputFrame, Move, Sim, Vec3};

const DT: f32 = 1.0 / 60.0;

fn arena(armed: &[bool]) -> Option<Sim> {
    let install = me_level::install();
    let anims = me_level::anims::load_player_anims(install).ok()?;
    let (_, armed_lib) = me_level::anims::load_armed_anims(install, &anims, &tdsim::weapons::GLOCK18C).unwrap();
    let mut bots = me_level::anims::load_bot_anims(install).unwrap();
    me_level::anims::add_bot_weapon_set(install, &mut bots, "AS_AI_PatrolCop_Onehanded_Glock18").unwrap();
    let mut sim = Sim::new(tdsim::testmap::world(), Config::load(install), anims.lib.clone());
    sim.unarmed_lib = Some(anims.lib.clone());
    sim.armed_libs.insert(tdsim::weapons::GLOCK18C.name, armed_lib);
    sim.bot_lib = bots.lib.clone();
    for ((feet, yaw), &a) in tdsim::testmap::bot_spawns().into_iter().zip(armed) {
        sim.bots.push(if a { tdsim::bots::Bot::new_patrol_cop(bots.lib.clone(), feet, yaw) } else { tdsim::bots::Bot::new(bots.lib.clone(), feet, yaw) });
    }
    let s = tdsim::testmap::spawns().into_iter().find(|s| s.name.starts_with("K combat")).unwrap();
    sim.spawn(s.feet, s.yaw);
    Some(sim)
}

fn run(sim: &mut Sim, frames: usize, verbose: bool, input: impl Fn(usize, &Sim) -> InputFrame) -> Vec<Move> {
    let mut states = vec![sim.pawn.movement_state];
    let mut last = String::new();
    let mut shots = 0;
    for i in 0..frames {
        sim.events.clear();
        let inp = input(i, sim);
        sim.tick(DT, inp);
        shots += sim.events.iter().filter(|e| matches!(e, tdsim::sim::Event::Shot(_))).count();
        if *states.last().unwrap() != sim.pawn.movement_state {
            states.push(sim.pawn.movement_state);
        }
        if verbose {
            let anim = sim.anim.slots.iter().filter(|(_, s)| s.playing).map(|(k, s)| format!("{k:?}:{}", s.name)).collect::<Vec<_>>().join(",");
            let bots = sim.bots.iter().map(|b| format!("{:?}/{:?} hp{} ammo{}", b.movement_state, b.cop_state, b.health, b.weapon.as_ref().map(|w| w.ammo).unwrap_or(-1))).collect::<Vec<_>>().join(" | ");
            let line = format!("{:?} [{anim}] hp {} gun {:?} || {bots}", sim.pawn.movement_state, sim.health, sim.weapon.as_ref().map(|w| w.ammo));
            if line != last {
                println!("  {i}: {line} at {:?}", sim.pawn.location);
                last = line;
            }
        }
    }
    println!("shots fired: {shots}");
    states
}

/// Standing in view of two armed cops: they shoot in bursts and their aim tightens until
/// they hit.
#[test]
fn cops_shoot_the_player() {
    let Some(mut sim) = arena(&[false, true, true]) else { return };
    let verbose = std::env::var("VERBOSE").is_ok();
    run(&mut sim, 60 * 6, verbose, |_, _| InputFrame::default());
    println!("health {}", sim.health);
    assert!(sim.health < 100 || sim.pawn.dying, "the cops should land shots");
}

/// Walk around behind an armed cop and snatch its gun (SnatchBack): the player is armed and
/// the cop goes down.
#[test]
fn disarm_from_behind() {
    let Some(mut sim) = arena(&[true, false, false]) else { return };
    let verbose = std::env::var("VERBOSE").is_ok();
    // stand right behind the first cop, facing its back
    let b = sim.bots[0].location;
    sim.spawn(Vec3::new(b.x + 110.0, b.y, 0.0), 32768);
    sim.bots[0].rotation.yaw = 32768;
    let st = run(&mut sim, 60 * 5, verbose, |i, _| InputFrame { switch_weapon: i == 3, ..Default::default() });
    println!("states {st:?} cop {:?} hp {}", sim.bots[0].movement_state, sim.bots[0].health);
    assert!(st.contains(&Move::Snatch), "{st:?}");
    assert!(sim.weapon.is_some(), "should be holding the gun");
    assert!(!sim.bots[0].alive(), "the cop should be down");
}

/// With a gun: hold fire at a cop until it dies, then empty the clip and throw it away.
#[test]
fn shoot_a_cop() {
    let Some(mut sim) = arena(&[true, false, false]) else { return };
    let verbose = std::env::var("VERBOSE").is_ok();
    sim.give_weapon(&tdsim::weapons::GLOCK18C, 24);
    let b = sim.bots[0].location;
    sim.spawn(Vec3::new(b.x - 500.0, b.y, 0.0), 0);
    let st = run(&mut sim, 60 * 6, verbose, |i, _| InputFrame { attack: i > 10 && i < 200, ..Default::default() });
    println!("states {st:?} cop {:?} hp {} pickups {}", sim.bots[0].movement_state, sim.bots[0].health, sim.pickups.len());
    assert!(!sim.bots[0].alive(), "the cop should die");
}

/// Face an armed cop up close: it swings the gun; RMB inside the disarm window snatches it
/// (SnatchFwd*), RMB before the swing misses (SnatchFail).
#[test]
fn disarm_counter_from_the_front() {
    let Some(mut sim) = arena(&[true, false, false]) else { return };
    let verbose = std::env::var("VERBOSE").is_ok();
    let b = sim.bots[0].location;
    sim.spawn(Vec3::new(b.x - 120.0, b.y, 0.0), 0);
    let st = run(&mut sim, 60 * 6, verbose, |_, s| {
        let b = &s.bots[0];
        let window = b.movement_state == tdsim::bots::BotMove::Melee && b.melee_active_time > 0.15;
        InputFrame { switch_weapon: window, ..Default::default() }
    });
    println!("states {st:?} cop {:?} hp {} gun {:?}", sim.bots[0].movement_state, sim.bots[0].health, sim.weapon.as_ref().map(|w| w.ammo));
    assert!(st.contains(&Move::Snatch), "{st:?}");
    assert!(sim.weapon.is_some());

    let Some(mut sim) = arena(&[true, false, false]) else { return };
    sim.spawn(Vec3::new(b.x - 300.0, b.y, 0.0), 0);
    let st = run(&mut sim, 60, verbose, |i, _| InputFrame { switch_weapon: i == 5, ..Default::default() });
    println!("early: states {st:?} anim {:?}", sim.anim.slots.iter().filter(|(_, s)| s.playing).map(|(_, s)| s.name.clone()).collect::<Vec<_>>());
    assert!(sim.weapon.is_none(), "too early: no gun");
}

/// The gun cues resolve in the sound bank.
#[test]
fn gun_sounds_load() {
    let install = me_level::install();
    if !install.exists() {
        return;
    }
    let cues: Vec<String> = tdsim::weapons::weapon_sound_cues().into_iter().map(String::from).collect();
    let bank = me_level::sounds::load_sound_bank(install, &cues);
    for c in &cues {
        println!("{c}: {}", bank.cues.contains_key(c));
    }
    assert!(cues.iter().all(|c| bank.cues.contains_key(c)), "missing gun cues");
}

/// The ragdoll of a shot cop settles on the floor (it doesn't fly off or sink).
#[test]
fn ragdoll_settles() {
    let Some(mut sim) = arena(&[true, false, false]) else { return };
    let install = me_level::install();
    let mut bots = me_level::anims::load_bot_anims(install).unwrap();
    me_level::anims::add_bot_weapon_set(install, &mut bots, "AS_AI_PatrolCop_Onehanded_Glock18").unwrap();
    let pkg = upk::Package::open(install.join(r"TdGame\CookedPC\Characters\CH_TKY_Cop_Patrol.upk")).unwrap();
    let pa = upk::physics::read_physics_asset(&pkg, pkg.find_export("Male3p_Physics", Some("PhysicsAsset")).unwrap()).unwrap();
    let mut pose = me_level::pose::PoseEvaluator::new(bots.set.clone(), &bots.skel);
    let r = bots.skel.rot_origin;
    let rot = tdsim::Rotator::new(r[0], r[1], r[2]);
    let t: usize = std::env::var("DEATH").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
    sim.bots[0].active_death_anim_type = t as u8;
    let floor = sim.bots[0].location.z - sim.bots[0].collision_height;
    let mut rd: Option<me_level::ragdoll::Ragdoll> = None;
    let mut m0 = None;
    let mut worst = (0.0f32, 0.0f32);
    for i in 0..60 * 5 {
        sim.events.clear();
        if i == 5 {
            // a killing shot
            let h = sim.bots[0].health;
            let loc = sim.bots[0].location;
            if t == 4 {
                sim.bot_take_bullet_damage(0, h as f32, loc, tdsim::Vec3::new(-12.0, 0.0, 0.0), &tdsim::weapons::GLOCK18C);
            } else {
                // a punch from the front (Melee.TestHit: facing * 150)
                sim.bot_take_damage(0, h, loc + tdsim::Vec3::new(-30.0, 0.0, 60.0), tdsim::Vec3::new(150.0, 0.0, 0.0), tdsim::combat::DamageType::MeleeLeft);
            }
        }
        sim.tick(DT, InputFrame::default());
        let b = &sim.bots[0];
        let m = me_level::pose::bot_mesh_to_world(b, rot, bots.skel.origin);
        pose.update_bot(b, DT, rot);
        if rd.is_none() {
            if let Some(d) = b.death.filter(|_| b.movement_state == tdsim::bots::BotMove::Dying) {
                let anim: Vec<_> = pose.globals().into_iter().map(|g| me_level::ragdoll::to_ue(m * g)).collect();
                let vel = vec![glam::Vec3::ZERO; anim.len()];
                let p = &pose;
                rd = Some(me_level::ragdoll::Ragdoll::new(&pa, |n| p.bone_index(n), &anim, &vel, d, (glam::Vec3::ZERO, glam::Vec3::new(-12.0, 0.0, 0.0)), sim.pawn.world_gravity_z));
                m0 = Some(m);
            }
        }
        if let Some(r) = rd.as_mut() {
            r.drive(&mut pose, m, m0.unwrap(), DT, &sim.world, glam::Vec3::new(b.velocity.x, b.velocity.y, b.velocity.z));
            let (lo, hi) = r.height_range();
            worst = (worst.0.min(lo - floor), worst.1.max(hi - floor));
            if std::env::var("RAGDBG").is_ok() && (6..=40).contains(&i) && i % 3 == 0 {
                let v: Vec<String> = r.debug_bodies().iter().map(|(n, z, sp, f)| format!("{n}:{:.0}/{:.0}{}", z - floor, sp, if *f { "F" } else { "" })).collect();
                println!("    {i}: {}", v.join(" "));
            }
            if i % 6 == 0 {
                let hips = pose.bone_index("Hips").unwrap();
                let head = pose.bone_index("Neck").unwrap();
                let tr: std::collections::HashMap<usize, me_level::ragdoll::Xform> = r.body_transforms().collect();
                let (hp, np) = (tr.get(&hips).map(|x| x.pos), tr.get(&head).map(|x| x.pos));
                let upright = match (hp, np) { (Some(h), Some(n)) => (n - h).normalize().z, _ => 0.0 };
                let travel = hp.map(|h| (h.x - sim.bots[0].home.x).hypot(h.y - sim.bots[0].home.y)).unwrap_or(0.0);
                println!("  {i}: t {:.2} bodies z {:.0}..{:.0} above floor, pawn z {:.0}, hips travel {:.0}, upright {:.2}", r.time, lo - floor, hi - floor, b.location.z - floor, travel, upright);
            }
        }
    }
    let (lo, hi) = rd.as_ref().unwrap().height_range();
    for (n, th, ps) in rd.as_ref().unwrap().debug_joints() {
        println!("joint {n:<13} swing {th:6.1} twist {ps:6.1}");
    }
    println!("end {:.0}..{:.0}, extremes {worst:?}", lo - floor, hi - floor);
    assert!(hi - floor < 60.0, "should end lying down");
    assert!(worst.1 < 250.0, "shouldn't fly");
    assert!(worst.0 > -20.0, "shouldn't sink");
}

/// God mode takes no damage; dying drops the gun (TossInventory).
#[test]
fn god_mode_and_death_drop() {
    let Some(mut sim) = arena(&[false, false, false]) else { return };
    sim.god_mode = true;
    sim.take_damage(500);
    assert_eq!(sim.health, 100);
    sim.god_mode = false;
    sim.give_weapon(&tdsim::weapons::GLOCK18C, 24);
    sim.play_weapon_deploy();
    sim.take_damage(500);
    assert!(sim.weapon.is_none(), "the gun should leave the hand");
    assert_eq!(sim.pickups.len(), 1);
}

/// A cop turns in place with the StandTurn anims when the player moves round him.
#[test]
fn cop_turns_in_place() {
    let Some(mut sim) = arena(&[true, false, false]) else { return };
    sim.god_mode = true;
    let b = sim.bots[0].location;
    // stand off to his side, out of melee range
    sim.spawn(Vec3::new(b.x, b.y + 400.0, 0.0), -16384);
    let mut turns = Vec::new();
    for _ in 0..120 {
        sim.events.clear();
        sim.tick(DT, InputFrame::default());
        let c = &sim.bots[0];
        if c.movement_state == tdsim::bots::BotMove::TurnStanding {
            let a = c.anim.slots.get(&tdsim::pawn::Slot::FullBody).map(|s| s.name.clone()).unwrap_or_default();
            if turns.last() != Some(&a) {
                turns.push(a);
            }
        }
    }
    let to = sim.pawn.location - sim.bots[0].location;
    let want = tdsim::Rotator::from_vector(to).yaw;
    let off = tdsim::math::norm_axis(want - sim.bots[0].rotation.yaw).abs() as f32 * 360.0 / 65536.0;
    println!("turns {turns:?}, facing off by {off:.1} deg");
    assert!(!turns.is_empty(), "should have stepped round");
    assert!(off < 22.5);
}

/// Every enemy type loads and, facing the player, shoots her; the player can fire every gun
/// (and kill an enemy with it).
#[test]
fn every_loadout_and_gun() {
    let install = me_level::install();
    let Some(anims) = me_level::anims::load_player_anims(install).ok() else { return };
    let armed = me_level::anims::load_all_armed_anims(install, &anims);
    assert_eq!(armed.len(), tdsim::weapons::WEAPONS.len(), "armed anims for every gun");
    let s = tdsim::testmap::spawns().into_iter().find(|s| s.name.starts_with("K combat")).unwrap();
    for l in tdsim::weapons::LOADOUTS.iter().skip(1) {
        let npc = me_level::anims::load_npc_anims(install, l).unwrap_or_else(|e| panic!("{}: {e}", l.label));
        let w = l.weapon.unwrap();
        let mut sim = Sim::new(tdsim::testmap::world(), Config::load(install), anims.lib.clone());
        sim.unarmed_lib = Some(anims.lib.clone());
        for (c, _, lib, _) in &armed {
            sim.armed_libs.insert(c.name, lib.clone());
        }
        let (feet, _) = tdsim::testmap::bot_spawns()[0];
        sim.spawn(Vec3::new(feet.x - 500.0, feet.y, 0.0), 0);
        sim.bots.push(tdsim::bots::Bot::with_loadout(npc.lib.clone(), feet, 32768, l));
        let ammo0 = sim.bots[0].weapon.as_ref().map(|w| w.ammo).unwrap_or(0);
        // the enemy shoots first (god mode keeps the player standing)
        sim.god_mode = true;
        for _ in 0..60 * 4 {
            sim.events.clear();
            sim.tick(DT, InputFrame::default());
        }
        let enemy_shots = ammo0 - sim.bots[0].weapon.as_ref().map(|w| w.ammo).unwrap_or(0);
        // then the player gets the gun and holds the trigger at its chest
        sim.give_weapon(w, w.max_ammo);
        sim.play_weapon_deploy();
        let mut shots = 0;
        for i in 0..60 * 8 {
            sim.events.clear();
            if sim.bots[0].alive() {
                let eye = sim.pawn.location + Vec3::new(0.0, 0.0, sim.pawn.base_eye_height);
                let r = tdsim::Rotator::from_vector(sim.bots[0].location + Vec3::new(0.0, 0.0, 20.0) - eye);
                sim.pc.rotation = tdsim::Rotator::new(r.pitch, r.yaw, 0);
            }
            sim.tick(DT, InputFrame { attack: i > 30 && (i / 10) % 2 == 0, ..Default::default() });
            for ev in &sim.events {
                if let tdsim::sim::Event::Shot(sh) = ev {
                    if sh.bot.is_none() {
                        shots += 1;
                        if std::env::var("VERBOSE").is_ok() {
                            println!("  {i}: shot {:?} -> {:?} bot at {:?} hp {} state {:?}", sh.start, sh.end, sim.bots[0].location, sim.bots[0].health, sim.bots[0].movement_state);
                        }
                    }
                }
            }
        }
        println!("{:<28} {:<22} heavy {:<5} enemy shots {enemy_shots:<3} player shots {shots:<3} enemy alive {}", l.label, w.name, w.heavy, sim.bots[0].alive());
        assert!(shots > 0, "{}: the player could not fire", w.name);
        assert!(enemy_shots > 0, "{}: the enemy never fired", l.label);
        assert!(!sim.bots[0].alive(), "{}: the enemy should die", l.label);
    }
}

/// Jump-kick from above onto a cop: MeleeAir turns into MeleeAirAbove (MarioMove) and the cop
/// dies from the 300 damage.
#[test]
fn land_on_enemy() {
    let Some(mut sim) = arena(&[true]) else { return };
    let verbose = std::env::var("VERBOSE").is_ok();
    let b = sim.bots[0].location;
    // 250 above its head, 150 behind, moving at it
    sim.spawn(Vec3::new(b.x - 150.0, b.y, b.z + 400.0), 0);
    sim.pawn.velocity = Vec3::new(300.0, 0.0, -100.0);
    sim.set_physics(tdsim::pawn::Physics::Falling);
    sim.set_move(Move::Falling, false, false);
    sim.pc.rotation.pitch = -12000;
    let st = run(&mut sim, 60 * 4, verbose, |i, _| InputFrame { attack: i == 4, forward: 1.0, ..Default::default() });
    println!("states {st:?} cop {:?} hp {}", sim.bots[0].movement_state, sim.bots[0].health);
    assert!(st.contains(&Move::MeleeAirAbove), "{st:?}");
    assert!(!sim.bots[0].alive(), "the cop should die");
}

/// A cop off a high ledge stumbles over the edge (StumbleFalling) and dies on landing.
#[test]
fn cop_dies_from_a_fall() {
    let Some(mut sim) = arena(&[true, false, false]) else { return };
    let b = sim.bots[0].location;
    sim.bots[0].location = Vec3::new(b.x, b.y, b.z + 1200.0);
    let st = run(&mut sim, 60 * 3, std::env::var("VERBOSE").is_ok(), |_, _| InputFrame::default());
    println!("states {st:?} cop {:?} hp {}", sim.bots[0].movement_state, sim.bots[0].health);
    assert!(!sim.bots[0].alive(), "the cop should die from the fall");
}

/// The snatched gun isn't current until the disarm ends (no firing, no gun crosshair), and
/// then only after EquipTime.
#[test]
fn no_fire_during_disarm() {
    let Some(mut sim) = arena(&[true, false, false]) else { return };
    let b = sim.bots[0].location;
    sim.spawn(Vec3::new(b.x + 110.0, b.y, 0.0), 32768);
    sim.bots[0].rotation.yaw = 32768;
    let mut fired_in_snatch = false;
    let mut first_shot = None;
    let mut snatch_end = None;
    for i in 0..60 * 6 {
        sim.events.clear();
        sim.tick(DT, InputFrame { switch_weapon: i == 3, attack: i > 10, ..Default::default() });
        let shot = sim.events.iter().any(|e| matches!(e, tdsim::sim::Event::Shot(s) if s.bot.is_none()));
        if sim.pawn.movement_state == Move::Snatch {
            fired_in_snatch |= shot || sim.has_weapon();
        } else if sim.weapon.is_some() && snatch_end.is_none() {
            snatch_end = Some(i);
        }
        if shot && first_shot.is_none() {
            first_shot = Some(i);
        }
    }
    println!("snatch end {snatch_end:?} first shot {first_shot:?}");
    assert!(!fired_in_snatch, "fired (or armed) during the snatch");
    let (e, f) = (snatch_end.unwrap(), first_shot.unwrap());
    assert!((f - e) as f32 * DT >= 0.7, "fired before EquipTime");
}

/// Armor: a SWAT cop takes 30% less from bullets and 40% less from punches than a patrol cop.
#[test]
fn armor_by_cop() {
    let Some(mut sim) = arena(&[true, false, false]) else { return };
    let install = me_level::install();
    let l = &tdsim::weapons::LOADOUTS[6];
    let npc = me_level::anims::load_npc_anims(install, l).unwrap();
    let (f, y) = tdsim::testmap::bot_spawns()[1];
    sim.bots[1] = tdsim::bots::Bot::with_loadout(npc.lib.clone(), f, y, l);
    for i in [0, 1] {
        let loc = sim.bots[i].location;
        sim.bot_take_bullet_damage(i, 20.0, loc, Vec3::ZERO, &tdsim::weapons::GLOCK18C);
        sim.bot_take_damage(i, 30, loc, Vec3::ZERO, tdsim::combat::DamageType::Melee);
    }
    println!("patrol {} swat {}", sim.bots[0].health, sim.bots[1].health);
    assert_eq!(sim.bots[0].health, 100 - 20 - 30);
    assert_eq!(sim.bots[1].health, 100 - 14 - 18);
}

/// Dead: no punches or disarms during the death animation.
#[test]
fn no_moves_while_dying() {
    let Some(mut sim) = arena(&[false, false, false]) else { return };
    sim.take_damage(1000);
    assert!(sim.pawn.dying);
    let st = run(&mut sim, 60, false, |i, _| InputFrame { attack: i % 10 == 0, switch_weapon: i % 10 == 5, ..Default::default() });
    assert!(!st.iter().any(|m| matches!(m, Move::Melee | Move::Snatch)), "{st:?}");
}

/// Slide kick (bent-over stumble), then the soccer kick kills (DeathAnimType 8): the ragdoll
/// shouldn't flip over or fly.
#[test]
fn soccer_kick_ragdoll() {
    let Some(mut sim) = arena(&[false]) else { return };
    let install = me_level::install();
    let bots = me_level::anims::load_npc_anims(install, &tdsim::weapons::LOADOUTS[0]).unwrap();
    let pa = me_level::anims::load_physics_asset(install, "CH_TKY_Cop_Patrol").unwrap();
    let mut pose = me_level::pose::PoseEvaluator::new(bots.set.clone(), &bots.skel);
    let r = bots.skel.rot_origin;
    let rot = tdsim::Rotator::new(r[0], r[1], r[2]);
    let floor = sim.bots[0].location.z - sim.bots[0].collision_height;
    let b0 = sim.bots[0].location;
    // the player in front of the bot (it faces -X toward the arena start)
    let yaw = sim.bots[0].rotation.yaw;
    let fwd = tdsim::Rotator::new(0, yaw, 0).vector();
    sim.spawn(b0 + fwd * 120.0, yaw + 32768);
    let kick_at: usize = std::env::var("KICK").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    let mut rd: Option<me_level::ragdoll::Ragdoll> = None;
    let mut m0 = None;
    let mut worst = (0.0f32, 0.0f32);
    let mut prev_up: Option<glam::Vec3> = None;
    let mut max_turn = 0.0f32;
    let mut total_turn = 0.0f32;
    for i in 0..60 * 5 {
        sim.events.clear();
        let loc = sim.bots[0].location;
        let pfwd = sim.pawn.rotation.vector();
        if i == 2 {
            sim.bot_take_damage(0, 1, loc - pfwd * 30.0, pfwd * 150.0, tdsim::combat::DamageType::MeleeSlide);
        }
        if i == kick_at {
            println!("stumble {:?} move {:?}", sim.bots[0].stumble_state, sim.bots[0].movement_state);
            sim.bots[0].active_death_anim_type = 8;
            let h = sim.bots[0].health;
            let m = (pfwd + tdsim::Vec3::new(0.0, 0.0, 0.5)) * 200.0;
            sim.bot_take_damage(0, h + 100, loc - pfwd * 30.0 + tdsim::Vec3::new(0.0, 0.0, 20.0), m, tdsim::combat::DamageType::MeleeSoccerKick);
        }
        sim.tick(DT, InputFrame::default());
        let b = &sim.bots[0];
        let m = me_level::pose::bot_mesh_to_world(b, rot, bots.skel.origin);
        pose.update_bot(b, DT, rot);
        if rd.is_none() {
            if let Some(d) = b.death.filter(|_| b.movement_state == tdsim::bots::BotMove::Dying) {
                let anim: Vec<_> = pose.globals().into_iter().map(|g| me_level::ragdoll::to_ue(m * g)).collect();
                let vel = vec![glam::Vec3::ZERO; anim.len()];
                let p = &pose;
                let (hl, hm) = b.death_hit;
                rd = Some(me_level::ragdoll::Ragdoll::new(&pa, |n| p.bone_index(n), &anim, &vel, d, (glam::Vec3::new(hl.x, hl.y, hl.z), glam::Vec3::new(hm.x, hm.y, hm.z)), sim.pawn.world_gravity_z));
                m0 = Some(m);
            }
        }
        if let Some(r) = rd.as_mut() {
            r.drive(&mut pose, m, m0.unwrap(), DT, &sim.world, glam::Vec3::new(b.velocity.x, b.velocity.y, b.velocity.z));
            let (lo, hi) = r.height_range();
            worst = (worst.0.min(lo - floor), worst.1.max(hi - floor));
            let hips = pose.bone_index("Hips").unwrap();
            if let Some((_, x)) = r.body_transforms().find(|(bi, _)| *bi == hips) {
                let up = x.rot * glam::Vec3::Z;
                if let Some(p) = prev_up {
                    let a = p.angle_between(up);
                    max_turn = max_turn.max(a / DT);
                    total_turn += a;
                }
                prev_up = Some(up);
                if i % 6 == 0 || std::env::var("RAGDBG").is_ok() {
                    println!("  {i}: t {:.2} hips z {:.0} up {:.2},{:.2},{:.2} bodies {:.0}..{:.0} pawn v {:?}", r.time, x.pos.z - floor, up.x, up.y, up.z, lo - floor, hi - floor, b.velocity);
                }
            }
        }
    }
    let (lo, hi) = rd.as_ref().unwrap().height_range();
    println!("end {:.0}..{:.0}, extremes {worst:?}, hips max turn {max_turn:.1} rad/s total {total_turn:.1} rad", lo - floor, hi - floor);
    assert!(hi - floor < 60.0, "should end lying down");
    assert!(worst.1 < 250.0, "shouldn't fly");
    // a flip ends with the hips' up axis pointing down (the old impulse bug: z -0.86)
    let end_up = prev_up.unwrap_or(glam::Vec3::Z);
    assert!(end_up.z > 0.0, "flipped over: hips up axis {end_up:?} (turned {total_turn:.1} rad in all)");
}

/// From the lane K ledge: run off toward a cop below and attack in the air (the way a player
/// would): MeleeAirAbove.
#[test]
fn land_on_enemy_from_the_ledge() {
    let Some(mut sim) = arena(&[true]) else { return };
    let verbose = std::env::var("VERBOSE").is_ok();
    let s = tdsim::testmap::spawns().into_iter().find(|s| s.name.starts_with("K ledge")).unwrap();
    sim.spawn(s.feet, s.yaw);
    // the cop where B puts one: 300 ahead, on the floor below the edge
    let at = s.feet + tdsim::Rotator::new(0, s.yaw, 0).vector() * 300.0;
    sim.bots[0].location = Vec3::new(at.x, at.y, sim.bots[0].collision_height);
    sim.bots[0].rotation.yaw = 0;
    let press: usize = std::env::var("PRESS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let st = run(&mut sim, 60 * 4, verbose, |i, s| {
        // over the edge and the cop just ahead
        let ahead = s.pawn.location.x - s.bots[0].location.x;
        let airborne = i > 10 && s.pawn.physics == tdsim::pawn::Physics::Falling;
        InputFrame { forward: if i < 60 { 1.0 } else { 0.0 }, attack: if press > 0 { i == press } else { airborne && s.pawn.velocity.z < -50.0 && ahead > -20.0 && ahead < 150.0 }, ..Default::default() }
    });
    println!("states {st:?} cop {:?} hp {}", sim.bots[0].movement_state, sim.bots[0].health);
    assert!(st.contains(&Move::MeleeAirAbove), "{st:?}");
}

/// Circle the player round a cop: the body keeps facing her (it aims all the time), the legs
/// step round with BotTurnStanding, and it keeps shooting through the turns.
#[test]
fn cop_tracks_while_turning() {
    let Some(mut sim) = arena(&[true]) else { return };
    sim.god_mode = true;
    let c = sim.bots[0].location;
    let mut worst = 0.0f32;
    let mut turns = 0;
    let mut shots_in_turn = 0;
    let mut prev = sim.bots[0].movement_state;
    for i in 0..60 * 8 {
        // a quarter turn every 2 s, 400 away
        let a = i as f32 / 60.0 * std::f32::consts::FRAC_PI_4;
        sim.spawn(Vec3::new(c.x + 400.0 * a.cos(), c.y + 400.0 * a.sin(), 0.0), 0);
        sim.events.clear();
        sim.tick(DT, InputFrame::default());
        let b = &sim.bots[0];
        if b.movement_state == tdsim::bots::BotMove::TurnStanding && prev != b.movement_state {
            turns += 1;
        }
        prev = b.movement_state;
        if b.movement_state == tdsim::bots::BotMove::TurnStanding {
            shots_in_turn += sim.events.iter().filter(|e| matches!(e, tdsim::sim::Event::Shot(s) if s.bot == Some(0))).count();
        }
        let want = tdsim::Rotator::from_vector(sim.pawn.location - b.location).yaw;
        let err = tdsim::math::norm_axis(want - b.rotation.yaw) as f32 * 360.0 / 65536.0;
        if i > 60 {
            worst = worst.max(err.abs());
        }
    }
    println!("turns {turns}, worst body aim error {worst:.1} deg, shots during turns {shots_in_turn}");
    assert!(turns > 0);
    assert!(worst < 30.0, "the body should keep facing the player");
}

/// The player on the ledge with a cop below near it: does it keep shooting up at her?
#[test]
fn cop_shoots_up_at_the_ledge() {
    let Some(mut sim) = arena(&[true]) else { return };
    sim.god_mode = true;
    let off: f32 = std::env::var("OFF").ok().and_then(|v| v.parse().ok()).unwrap_or(150.0);
    sim.spawn(Vec3::new(1830.0, 12650.0, 350.0), 32768);
    sim.bots[0].location = Vec3::new(1830.0 - off, 12650.0, sim.bots[0].collision_height);
    sim.bots[0].rotation.yaw = 0;
    sim.bots[0].leg_yaw = 0;
    let mut shots = 0;
    for _ in 0..60 * 4 {
        sim.events.clear();
        sim.tick(DT, InputFrame::default());
        shots += sim.events.iter().filter(|e| matches!(e, tdsim::sim::Event::Shot(s) if s.bot == Some(0))).count();
    }
    println!("offset {off}: cop shots {shots}, visible {}", sim.bot_visible_to_player(0));
    assert!(shots > 0);
}

/// Punch a SWAT cop over and over: after MeleeAttackLimit hits in a row it blocks (immune)
/// and shoves the player into a stumble.
#[test]
fn swat_blocks_punch_spam() {
    let Some(mut sim) = arena(&[true]) else { return };
    let install = me_level::install();
    let l = &tdsim::weapons::LOADOUTS[6];
    let npc = me_level::anims::load_npc_anims(install, l).unwrap();
    let (f, _) = tdsim::testmap::bot_spawns()[0];
    sim.bots[0] = tdsim::bots::Bot::with_loadout(npc.lib.clone(), f, 32768, l);
    sim.god_mode = true;
    sim.spawn(Vec3::new(f.x - 110.0, f.y, 0.0), 0);
    let mut blocked = false;
    let mut stumbled = false;
    if std::env::var("VERBOSE").is_ok() {
        run(&mut sim, 60 * 6, true, |i, _| InputFrame { attack: i % 8 < 4, forward: 1.0, ..Default::default() });
        return;
    }
    for i in 0..60 * 6 {
        sim.events.clear();
        // stay in reach, facing it (the player steering in)
        if sim.pawn.movement_state == Move::Walking || sim.pawn.movement_state == Move::Melee {
            let b = sim.bots[0].location;
            let d = (sim.pawn.location - b).safe_normal_2d();
            sim.pawn.location = Vec3::new(b.x + d.x * 110.0, b.y + d.y * 110.0, sim.pawn.location.z);
            let yaw = tdsim::Rotator::from_vector(b - sim.pawn.location).yaw;
            sim.pawn.rotation.yaw = yaw;
            sim.pc.rotation.yaw = yaw;
        }
        sim.tick(DT, InputFrame { attack: i % 8 < 4, ..Default::default() });
        blocked |= sim.bots[0].movement_state == tdsim::bots::BotMove::Block;
        stumbled |= sim.pawn.movement_state == Move::Stumble;
    }
    println!("blocked {blocked} stumbled {stumbled} swat hp {} alive {}", sim.bots[0].health, sim.bots[0].alive());
    assert!(blocked, "the SWAT cop should block");
}

/// A cop near the edge of the 350 ledge (under twice its height) gets punched toward the
/// drop: it stays up there.
#[test]
fn cop_not_pushed_off_a_safe_ledge() {
    let Some(mut sim) = arena(&[false]) else { return };
    sim.god_mode = true;
    // on the ledge, 40 from its -X edge, facing +X; the player on the ledge at +X punching
    sim.bots[0].location = Vec3::new(1840.0, 12650.0, 350.0 + sim.bots[0].collision_height);
    sim.bots[0].rotation.yaw = 0;
    sim.bots[0].leg_yaw = 0;
    sim.spawn(Vec3::new(1950.0, 12650.0, 350.0), 32768);
    let mut low = f32::MAX;
    for i in 0..60 * 5 {
        sim.events.clear();
        sim.tick(DT, InputFrame { attack: i % 10 < 5, ..Default::default() });
        low = low.min(sim.bots[0].location.z);
    }
    println!("cop lowest z {low:.0}, state {:?}, hp {}", sim.bots[0].movement_state, sim.bots[0].health);
    assert!(low > 350.0, "pushed off a non-lethal ledge");
}

/// From the K tower (600, over twice a cop's height) a punched cop goes over the edge,
/// flails (HitMeleeOverEdgeLoop) and dies on landing.
#[test]
fn cop_knocked_off_the_tower() {
    let Some(mut sim) = arena(&[false]) else { return };
    sim.god_mode = true;
    let s = tdsim::testmap::spawns().into_iter().find(|s| s.name.starts_with("K tower")).unwrap();
    sim.spawn(s.feet, s.yaw);
    let at = s.feet + tdsim::Rotator::new(0, s.yaw, 0).vector() * 260.0;
    sim.bots[0].location = Vec3::new(at.x, at.y, 600.0 + sim.bots[0].collision_height);
    sim.bots[0].rotation.yaw = 0;
    sim.bots[0].leg_yaw = 0;
    let mut states = Vec::new();
    let mut anims = Vec::new();
    for i in 0..60 * 6 {
        sim.events.clear();
        // walk up and punch
        sim.tick(DT, InputFrame { attack: i % 10 < 5, forward: if i < 30 { 1.0 } else { 0.0 }, ..Default::default() });
        let b = &sim.bots[0];
        if states.last() != Some(&b.movement_state) {
            states.push(b.movement_state);
        }
        for (_, sl) in b.anim.slots.iter().filter(|(_, s)| s.playing) {
            if !anims.contains(&sl.name.to_string()) {
                anims.push(sl.name.to_string());
            }
        }
    }
    println!("cop states {states:?} anims {anims:?} alive {}", sim.bots[0].alive());
    assert!(states.contains(&tdsim::bots::BotMove::StumbleFalling), "{states:?}");
    assert!(!sim.bots[0].alive());
}

/// SnatchBack: the view and the pawn per frame (looking for jitter).
#[test]
fn snatch_back_smooth() {
    let Some(mut sim) = arena(&[true]) else { return };
    let b = sim.bots[0].location;
    sim.spawn(Vec3::new(b.x + 110.0, b.y, 0.0), 32768);
    sim.bots[0].rotation.yaw = 32768;
    sim.bots[0].leg_yaw = 32768;
    let mut prev: Option<(tdsim::Rotator, Vec3)> = None;
    let mut prev_d: Option<(i32, i32, f32)> = None;
    let mut flips = 0;
    let mut worst = 0i32;
    for i in 0..60 * 4 {
        sim.events.clear();
        sim.tick(DT, InputFrame { switch_weapon: i == 3, ..Default::default() });
        if sim.pawn.movement_state != Move::Snatch {
            prev = None;
            continue;
        }
        let (r, l) = (sim.pc.rotation, sim.pawn.location);
        if let Some((pr, pl)) = prev {
            let d = (tdsim::math::norm_axis(r.yaw - pr.yaw), tdsim::math::norm_axis(r.pitch - pr.pitch), (l - pl).length());
            if let Some(pd) = prev_d {
                // a sign change in the per-frame yaw step = shake
                if d.0.signum() * pd.0.signum() < 0 && d.0.abs() > 20 && pd.0.abs() > 20 {
                    flips += 1;
                }
                worst = worst.max((d.0 - pd.0).abs());
            }
            if std::env::var("VERBOSE").is_ok() {
                println!("{i}: yaw step {} pitch step {} move {:.1} anim {:?}", d.0, d.1, d.2, sim.anim.slots.iter().filter(|(_, s)| s.playing).map(|(k, s)| format!("{k:?}:{}", s.name)).collect::<Vec<_>>());
            }
            prev_d = Some(d);
        }
        prev = Some((r, l));
    }
    println!("yaw step reversals {flips}, worst step change {worst}");
}

/// SnatchBack as rendered: the EyeJoint camera position and the camera animation per frame,
/// with the second differences (shake shows up as large accelerations that flip sign).
#[test]
fn snatch_back_camera() {
    let install = me_level::install();
    let Some(mut sim) = arena(&[true]) else { return };
    let anims = me_level::anims::load_player_anims(install).unwrap();
    let mut pose = me_level::pose::PoseEvaluator::new(anims.set.clone(), &anims.upper);
    for (class, seqs, _, n) in me_level::anims::load_all_armed_anims(install, &anims) {
        pose.add_armed_seqs(class.name, seqs, n);
    }
    let eye = pose.bone_index("EyeJoint").unwrap();
    let b = sim.bots[0].location;
    sim.spawn(Vec3::new(b.x + 110.0, b.y, 0.0), 32768);
    sim.bots[0].rotation.yaw = 32768;
    sim.bots[0].leg_yaw = 32768;
    let mut hist: Vec<(glam::Vec3, tdsim::Rotator)> = Vec::new();
    for i in 0..60 * 3 {
        sim.events.clear();
        sim.tick(DT, InputFrame { switch_weapon: i == 3, ..Default::default() });
        pose.armed = sim.anim_weapon;
        pose.update(&mut sim, DT);
        let m = me_level::pose::mesh_to_world(&sim, anims.lib.mesh_rot, anims.upper.origin) * pose.globals()[eye];
        let rot = me_level::camera::camera_rotation(sim.pc.rotation, pose.camera_animation());
        if sim.pawn.movement_state == Move::Snatch || (i > 3 && i < 100) {
            hist.push((m.w_axis.truncate() * 100.0, rot));
        }
        if std::env::var("BOTDBG").is_ok() && i < 70 {
            let bb = &sim.bots[0];
            println!("{i}: bot {:?} yaw {} leg {} at {:.1},{:.1} anim {:?}", bb.movement_state, bb.rotation.yaw, bb.leg_yaw, bb.location.x, bb.location.y, bb.anim.slots.iter().filter(|(_, s)| s.playing).map(|(k, s)| format!("{k:?}:{}:{:.2}", s.name, s.weight)).collect::<Vec<_>>());
        }
    }
    let mut worst = (0.0f32, 0usize);
    for k in 2..hist.len() {
        let a = hist[k].0 - hist[k - 1].0 * 2.0 + hist[k - 2].0;
        let ry = |r: tdsim::Rotator| (r.yaw, r.pitch, r.roll);
        let (y0, p0, r0) = ry(hist[k - 2].1);
        let (y1, p1, r1) = ry(hist[k - 1].1);
        let (y2, p2, r2) = ry(hist[k].1);
        let acc = |a: i32, b: i32, c: i32| tdsim::math::norm_axis(c - b) - tdsim::math::norm_axis(b - a);
        let rot_acc = (acc(y0, y1, y2).abs().max(acc(p0, p1, p2).abs()).max(acc(r0, r1, r2).abs())) as f32;
        if std::env::var("VERBOSE").is_ok() {
            println!("{k}: eye acc {:.2} uu/f2  rot acc {rot_acc}  rot {:?}", a.length(), hist[k].1);
        }
        if a.length() > worst.0 {
            worst = (a.length(), k);
        }
    }
    println!("worst eye acceleration {:.2} uu/frame^2 at {}", worst.0, worst.1);
}

/// Slide kick + soccer kick follow-up on a SWAT cop, twice: the damage each hit does.
#[test]
fn swat_slide_kick_combos() {
    let Some(mut sim) = arena(&[true]) else { return };
    let install = me_level::install();
    let l = &tdsim::weapons::LOADOUTS[6];
    let npc = me_level::anims::load_npc_anims(install, l).unwrap();
    let (f, _) = tdsim::testmap::bot_spawns()[0];
    sim.bots[0] = tdsim::bots::Bot::with_loadout(npc.lib.clone(), f, 32768, l);
    sim.god_mode = true;
    let mut hp = sim.bots[0].health;
    let mut hits = Vec::new();
    for round in 0..2 {
        sim.spawn(Vec3::new(f.x - 700.0, f.y, 0.0), 0);
        let mut kicked = false;
        for i in 0..60 * 5 {
            let s = &sim;
            let d = (s.bots[0].location - s.pawn.location).length();
            let sliding = matches!(s.pawn.movement_state, Move::Slide | Move::MeleeSlide);
            let stunned = s.bots[0].movement_state == tdsim::bots::BotMove::Stumble;
            let inp = InputFrame {
                forward: if sliding || d > 110.0 { 1.0 } else { 0.0 },
                crouch: d < 450.0 && !stunned && s.pawn.movement_state != Move::Melee && !kicked,
                attack: (sliding && d < 200.0) || (stunned && !sliding && i % 10 < 2 && !kicked),
                ..Default::default()
            };
            sim.events.clear();
            sim.tick(DT, inp);
            if sim.bots[0].health != hp {
                hits.push(format!("round {round}: {} -> {} ({:?})", hp, sim.bots[0].health, sim.anim.slots.iter().filter(|(_, s)| s.playing).map(|(_, s)| s.name.to_string()).collect::<Vec<_>>()));
                if sim.pawn.movement_state == Move::Melee { kicked = true; }
                hp = sim.bots[0].health;
            }
            if !sim.bots[0].alive() { break; }
            if kicked && sim.bots[0].movement_state == tdsim::bots::BotMove::Walking { break; }
        }
    }
    for h in &hits { println!("{h}"); }
    println!("alive {}", sim.bots[0].alive());
}

/// RMB in front of a cop that isn't swinging: SnatchFail. The player shouldn't orbit it.
#[test]
fn snatch_miss_no_spin() {
    let Some(mut sim) = arena(&[true]) else { return };
    sim.god_mode = true;
    let b = sim.bots[0].location;
    let d: f32 = std::env::var("DIST").ok().and_then(|v| v.parse().ok()).unwrap_or(140.0);
    // coming in at an angle, the cop off to one side of the view
    sim.spawn(Vec3::new(b.x - d * 0.8, b.y - d * 0.6, 0.0), 0);
    let yaw0 = sim.pawn.rotation.yaw;
    let mut worst = 0i32;
    let mut states = Vec::new();
    for i in 0..60 * 2 {
        sim.events.clear();
        sim.tick(DT, InputFrame { switch_weapon: i == 3, ..Default::default() });
        if states.last() != Some(&sim.pawn.movement_state) { states.push(sim.pawn.movement_state); }
        let dy = tdsim::math::norm_axis(sim.pawn.rotation.yaw - yaw0).abs();
        worst = worst.max(dy);
        if std::env::var("VERBOSE").is_ok() && i < 70 {
            println!("{i}: {:?} yaw {} pc yaw {} at {:.1},{:.1} bot at {:.1},{:.1} targeting {:?}", sim.pawn.movement_state, sim.pawn.rotation.yaw, sim.pc.rotation.yaw, sim.pawn.location.x, sim.pawn.location.y, sim.bots[0].location.x, sim.bots[0].location.y, sim.pc.targeting_pawn);
        }
    }
    println!("states {states:?} worst yaw change {:.1} deg", worst as f32 * 360.0 / 65536.0);
    assert!(states.contains(&Move::Snatch));
    assert!(worst as f32 * 360.0 / 65536.0 < 30.0, "spun round");
}
