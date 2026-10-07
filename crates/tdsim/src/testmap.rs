//! The test course: one lane per family of obstacles, built from boxes. Unreal units, ground
//! top at z = 0, lanes run along +X. Red marks what Runner Vision would highlight.

use crate::collision::{Surface, World, WorldBuilder};
use crate::math::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Floor,
    Wall,
    Red,
    Blue,
    Yellow,
    Dark,
}

#[derive(Clone, Copy, Debug)]
pub struct Block {
    pub min: Vec3,
    pub max: Vec3,
    /// rotation about Z (radians) around the block centre; ramps use `pitch` instead
    pub yaw: f32,
    /// slope angle (radians) rising along +X, for ramps
    pub pitch: f32,
    pub kind: Kind,
}

/// A static mesh from the game's packages placed in the course (loaded by me_level).
#[derive(Clone, Debug)]
pub struct Prop {
    /// Package file name without extension, e.g. "P_Renovation" or "Tutorial_p".
    pub package: &'static str,
    pub mesh: &'static str,
    /// Material (instance) in the same package replacing the mesh's own.
    pub material: Option<&'static str>,
    pub location: Vec3,
    pub yaw: i32,
    /// Pitch and roll of the placement (Unreal rotator units), for pieces that aren't upright.
    pub pitch: i32,
    pub roll: i32,
    /// DrawScale3D (times DrawScale).
    pub scale: [f32; 3],
    /// The surface's TdPhysicalMaterialProperty.bEnableSoftLanding.
    pub soft_landing: bool,
    pub collides: bool,
}

pub struct Spawn {
    pub name: &'static str,
    pub feet: Vec3,
    pub yaw: i32,
}

fn b(min: [f32; 3], max: [f32; 3], kind: Kind) -> Block {
    Block { min: Vec3::from(min), max: Vec3::from(max), yaw: 0.0, pitch: 0.0, kind }
}

/// A ramp rising along +X from `x0` at ground level, `len` long, `deg` steep, between y0..y1.
fn ramp(x0: f32, len: f32, deg: f32, y0: f32, y1: f32) -> Block {
    let t = 20.0;
    Block { min: Vec3::new(x0, y0, -t), max: Vec3::new(x0 + len, y1, 0.0), yaw: 0.0, pitch: deg.to_radians(), kind: Kind::Floor }
}

pub fn blocks() -> Vec<Block> {
    use Kind::*;
    let mut v = vec![
        // ---- Lane A (y -400..400): sprint strip with a marker every 500 uu
        b([-600.0, -400.0, -100.0], [5000.0, 400.0, 0.0], Floor),
        // past the end, a 700 gap to a thin slab 100 lower: a running jump catches its edge
        // with nothing under the feet (hang free / folded hang)
        b([5700.0, -400.0, -200.0], [6300.0, 400.0, -100.0], Floor),
    ];
    for i in 0..10 {
        let x = i as f32 * 500.0;
        v.push(b([x, -400.0, 0.0], [x + 10.0, -380.0, 4.0], Dark));
    }
    v.extend([
        // ---- Lane B (y 800..1600): step-ups and vaults
        b([-600.0, 800.0, -100.0], [5000.0, 1600.0, 0.0], Floor),
        b([400.0, 900.0, 0.0], [440.0, 1500.0, 12.0], Dark),   // 12: taken in stride
        b([700.0, 900.0, 0.0], [760.0, 1500.0, 35.0], Dark),   // 35: MaxStepHeight
        b([1000.0, 900.0, 0.0], [1100.0, 1500.0, 48.0], Red),  // 48: auto step-up
        b([1400.0, 900.0, 0.0], [1700.0, 1500.0, 72.0], Red),  // 72: step up onto
        b([2100.0, 900.0, 0.0], [2400.0, 1500.0, 100.0], Red), // 100: vault onto / speed vault
        b([2900.0, 900.0, 0.0], [2930.0, 1500.0, 110.0], Red), // thin 110 railing: vault over
        b([3500.0, 900.0, 0.0], [3800.0, 1500.0, 140.0], Red), // 140: climb up (grab + pull up)
        // ---- Lane C (y 2000..2800): wall run and wall climb
        b([-600.0, 2000.0, -100.0], [800.0, 2800.0, 0.0], Floor),
        b([1700.0, 2000.0, -100.0], [5000.0, 2800.0, 0.0], Floor),
        // pit under the wall-run gap
        b([800.0, 2000.0, -1100.0], [1700.0, 2800.0, -1000.0], Dark),
        // wall-run wall on the left (blue tarp), runs over the gap
        b([300.0, 1960.0, 0.0], [2200.0, 2000.0, 600.0], Blue),
        b([300.0, 1960.0, -1000.0], [2200.0, 2000.0, 0.0], Wall),
        // wall-climb wall facing the run-up: 330 high with a ledge to grab
        b([3000.0, 2100.0, 0.0], [3400.0, 2700.0, 330.0], Wall),
        b([3000.0, 2100.0, 330.0], [3060.0, 2700.0, 334.0], Red),
        // taller wall: 520, climb then fall unless you hit the ledge
        b([4200.0, 2100.0, 0.0], [4600.0, 2700.0, 520.0], Wall),
        b([4200.0, 2100.0, 520.0], [4260.0, 2700.0, 524.0], Red),
        // ---- Lane D (y 3200..4000): gaps and drops
        b([-600.0, 3200.0, -100.0], [800.0, 4000.0, 0.0], Floor),
        b([1100.0, 3200.0, -100.0], [1700.0, 4000.0, 0.0], Floor),  // 300 gap
        b([2200.0, 3200.0, -100.0], [2700.0, 4000.0, 0.0], Floor),  // 500 gap
        b([2700.0, 3200.0, -300.0], [3300.0, 4000.0, -200.0], Floor), // 200 drop
        b([3300.0, 3200.0, -700.0], [3900.0, 4000.0, -600.0], Floor), // 400 drop: roll
        b([3900.0, 3200.0, -1300.0], [5000.0, 4000.0, -1200.0], Floor), // 600 drop: hard landing
        b([800.0, 3200.0, -1300.0], [2200.0, 4000.0, -1200.0], Dark),   // catch floor under gaps
        // ---- Lane E (y 4400..5200): slide, crouch, ramps
        b([-600.0, 4400.0, -100.0], [5000.0, 5200.0, 0.0], Floor),
        // crouched cylinder is 122 tall and walks ~3uu above the floor
        b([600.0, 4400.0, 130.0], [900.0, 5200.0, 200.0], Red),    // slide-under bar
        b([1300.0, 4400.0, 130.0], [1900.0, 5200.0, 170.0], Wall), // crouch tunnel roof
        b([1300.0, 4400.0, 0.0], [1900.0, 4500.0, 130.0], Wall),
        b([1300.0, 5100.0, 0.0], [1900.0, 5200.0, 130.0], Wall),
        // ---- Lane F (y 5600..6400): hang and shimmy. 300 tall is above every vault type, so a
        // jump without forward held grabs the ledge; the block's +Y end is an outside corner.
        b([-600.0, 5600.0, -100.0], [3000.0, 6400.0, 0.0], Floor),
        b([1000.0, 5700.0, 0.0], [1400.0, 6200.0, 296.0], Wall),
        b([1000.0, 5700.0, 296.0], [1400.0, 6200.0, 300.0], Red),
        // inside corner past the block's +Y end: a wall facing -Y to grab-transfer onto (hang
        // near the end, hold D or look right, and jump)
        b([600.0, 6280.0, 0.0], [1000.0, 6380.0, 296.0], Wall),
        b([600.0, 6280.0, 296.0], [1000.0, 6380.0, 300.0], Red),
        // ---- Lane G (y 6800..7600): springboard and pipe-free climbs
        b([-600.0, 6800.0, -100.0], [5000.0, 7600.0, 0.0], Floor),
        // springboard: a ~64 step whose face is 112 in front of a block 80..148 tall
        b([800.0, 6900.0, 0.0], [912.0, 7500.0, 64.0], Yellow),
        b([912.0, 6900.0, 0.0], [1500.0, 7500.0, 140.0], Wall),
        b([912.0, 6900.0, 140.0], [972.0, 7500.0, 144.0], Red),
        // ---- Lane H (y 8000..8800): a tall tower with a ladder, landing bags at its foot
        b([-600.0, 8000.0, -100.0], [3700.0, 8800.0, 0.0], Floor),
        // low platform past the bags: jump off its -X edge into them (a 6 m drop)
        b([LOW_X, 8250.0, 0.0], [LOW_X + 300.0, 8550.0, LOW_TOP], Wall),
        b([LOW_X, 8250.0, LOW_TOP - 4.0], [LOW_X + 300.0, 8550.0, LOW_TOP], Red),
        b([TOWER_X, 8150.0, 0.0], [TOWER_X + 500.0, 8650.0, TOWER_TOP], Wall),
        b([TOWER_X, 8150.0, TOWER_TOP - 4.0], [TOWER_X + 500.0, 8650.0, TOWER_TOP], Red),
        // ---- Lane I (y 9300..10400): swing bars over a pit, then a zipline off a tower
        b([-600.0, 9300.0, -100.0], [SWING_X - 200.0, 9700.0, 0.0], Floor),
        b([SWING_X - 200.0, 9300.0, -1300.0], [SWING_LAND_X, 9700.0, -1200.0], Dark),
        b([SWING_LAND_X, 9300.0, -100.0], [SWING_LAND_X + 1000.0, 9700.0, 0.0], Floor),
        // posts holding each bar's ends
        b([SWING_X - 10.0, SWING_Y - 84.0, -1200.0], [SWING_X + 10.0, SWING_Y - 74.0, SWING_GRIP + 20.0], Wall),
        b([SWING_X - 10.0, SWING_Y + 74.0, -1200.0], [SWING_X + 10.0, SWING_Y + 84.0, SWING_GRIP + 20.0], Wall),
        b([SWING_X2 - 10.0, SWING_Y - 84.0, -1200.0], [SWING_X2 + 10.0, SWING_Y - 74.0, SWING_GRIP + 20.0], Wall),
        b([SWING_X2 - 10.0, SWING_Y + 74.0, -1200.0], [SWING_X2 + 10.0, SWING_Y + 84.0, SWING_GRIP + 20.0], Wall),
        // the zipline: floor, the start tower (ladder on its -X face), a low roof under the end of
        // the cable and the building it's anchored to
        b([-600.0, 9900.0, -100.0], [ZIP_END.x + 600.0, 10500.0, 0.0], Floor),
        b([0.0, 10000.0, 0.0], [ZIP_TOWER_X, 10400.0, ZIP_TOWER_TOP], Wall),
        b([0.0, 10000.0, ZIP_TOWER_TOP - 4.0], [ZIP_TOWER_X, 10400.0, ZIP_TOWER_TOP], Red),
        b([ZIP_END.x - 700.0, 10050.0, 0.0], [ZIP_END.x, 10350.0, ZIP_ROOF], Wall),
        b([ZIP_END.x, 10000.0, 0.0], [ZIP_END.x + 400.0, 10400.0, 1000.0], Wall),
        // ---- Lane J (y 10900..11700): balance across a pipe and a narrow beam over a pit
        b([-600.0, 10900.0, -100.0], [BAL_X0, 11700.0, 0.0], Floor),
        b([BAL_X0, 10900.0, -500.0], [BAL_X1, 11700.0, -400.0], Dark),
        b([BAL_X1, 10900.0, -100.0], [BAL_X1 + 1000.0, 11700.0, 0.0], Floor),
        b([BAL_X0, BEAM_Y - 8.0, -30.0], [BAL_X1, BEAM_Y + 8.0, 0.0], Dark),
        // ---- Lane K (y 11900..13100): a combat arena with a wall to wall-run along
        b([-600.0, 11900.0, -100.0], [2400.0, 13100.0, 0.0], Floor),
        b([-600.0, 13100.0, 0.0], [2400.0, 13140.0, 400.0], Blue),
        // a 350 ledge at the far end to jump down onto enemies from (MeleeAirAbove)
        b([1800.0, 12200.0, 0.0], [2400.0, 13100.0, 350.0], Blue),
        // a 600 tower beside it: knocked off its -X edge an enemy falls a deadly height
        b([2000.0, 11900.0, 0.0], [2400.0, 12200.0, 600.0], Blue),
        // world floor far below so misses are recoverable
        b([-3000.0, -2000.0, -3010.0], [8000.0, 10000.0, -3000.0], Dark),
    ]);
    // ramps in lane E beyond the tunnels: 20, 35 and 45 degrees (WalkableFloorZ 0.71 ~ 44.8)
    v.push(ramp(2300.0, 500.0, 20.0, 4450.0, 4700.0));
    v.push(ramp(2300.0, 400.0, 35.0, 4720.0, 4950.0));
    v.push(ramp(2300.0, 300.0, 45.0, 4970.0, 5180.0));
    v
}

/// Lane H tower: the ladder is on its -X face.
pub const TOWER_X: f32 = 1000.0;
pub const TOWER_TOP: f32 = 1500.0;
const LADDER_Y: f32 = 8400.0;

/// Lane H low platform, past the landing bags: 600 high, its ladder on the far (+X) face.
pub const LOW_X: f32 = 2950.0;
pub const LOW_TOP: f32 = 600.0;

/// Lane I swing bars: grips 230 over the run-up floor (so a running jump reaches the volume
/// round the grip) and 450 apart, near enough for a jump off the first to target the second.
const SWING_X: f32 = 1000.0;
const SWING_X2: f32 = 1450.0;
const SWING_Y: f32 = 9500.0;
const SWING_GRIP: f32 = 230.0;
const SWING_LAND_X: f32 = 1800.0;

/// Lane I zipline: a cable from a 1200 tower's roof edge down to a building 2900 away.
const ZIP_Y: f32 = 10200.0;
const ZIP_TOWER_X: f32 = 400.0;
const ZIP_TOWER_TOP: f32 = 1200.0;
const ZIP_START: Vec3 = Vec3::new(ZIP_TOWER_X, ZIP_Y, ZIP_TOWER_TOP + 230.0);
const ZIP_END: Vec3 = Vec3::new(3300.0, ZIP_Y, 700.0);
/// The low roof under the cable's last stretch.
const ZIP_ROOF: f32 = 360.0;

/// Lane J: a pipe (the tutorial's S_PipeSystem_03h, 8.5 round, its top level with the floor)
/// and a 16 wide beam across a 1248 pit.
const BAL_X0: f32 = 400.0;
const BAL_X1: f32 = BAL_X0 + 3.0 * 416.0;
const PIPE_BEAM_Y: f32 = 11100.0;
const PIPE_BEAM_R: f32 = 8.5;
const BEAM_Y: f32 = 11500.0;

/// The course's TdBalanceWalkVolumes: the spline along the middle of the pipe (as the
/// tutorial's runs through its pipe's axis) and along the top of the beam, the box reaching 260
/// up and 40 to each side.
pub fn balances() -> Vec<crate::volumes::SplineVolume> {
    vec![
        crate::volumes::balance_volume(Vec3::new(BAL_X0, PIPE_BEAM_Y, -PIPE_BEAM_R), Vec3::new(BAL_X1, PIPE_BEAM_Y, -PIPE_BEAM_R), 40.0, 260.0),
        crate::volumes::balance_volume(Vec3::new(BAL_X0, BEAM_Y, 0.0), Vec3::new(BAL_X1, BEAM_Y, 0.0), 40.0, 260.0),
    ]
}

/// Lane K's enemies: (feet, yaw) facing the run-up.
pub fn bot_spawns() -> Vec<(Vec3, i32)> {
    vec![
        (Vec3::new(700.0, 12500.0, 0.0), 32768),
        (Vec3::new(1100.0, 12200.0, 0.0), 32768),
        (Vec3::new(1100.0, 12850.0, 0.0), 32768),
    ]
}

/// Lane K's cops: the front one is the unarmed sparring dummy, the two behind it are armed
/// patrol cops (Glock).
pub fn bot_spawns_armed() -> Vec<(Vec3, i32, bool)> {
    bot_spawns().into_iter().enumerate().map(|(i, (f, y))| (f, y, i > 0)).collect()
}

/// The course's swing bars (TdSwingVolume, swinging along +X).
pub fn swings() -> Vec<crate::volumes::SwingVolume> {
    let mut first = crate::volumes::SwingVolume::new(Vec3::new(SWING_X, SWING_Y, SWING_GRIP), 0);
    first.thick_grip = true;
    let mut second = crate::volumes::SwingVolume::new(Vec3::new(SWING_X2, SWING_Y, SWING_GRIP), 0);
    second.thick_grip = false;
    vec![first, second]
}

/// The course's zipline (TdZiplineVolume): the cable sags like the tutorial's, its Middle
/// 130 under the line's midpoint.
pub fn ziplines() -> Vec<crate::volumes::ZiplineVolume> {
    let mid = (ZIP_START + ZIP_END) * 0.5 - Vec3::new(0.0, 0.0, 130.0);
    vec![crate::volumes::ZiplineVolume::new(ZIP_START, mid, ZIP_END, 64.0, 260.0, 60.0)]
}

/// A TdLadderVolume up the wall face at `wall_x` of a block `roof` high, climbed facing `dir`
/// (+1 = +X), laid out like the tutorial's ladders: the stored climb positions sit 12 in front
/// of the wall (64 inside the volume's outer face), the volume reaches 96 over the roof so the
/// last position is level with it (where the ladder's hooked top piece starts), it is 130 deep,
/// and its bottom reaches 16 under the floor so the lowest step sits low enough that climbing
/// down into it meets the floor (HitWall -> LetGo), like the tutorial's (34 over the bottom).
fn ladder_volume(wall_x: f32, dir: f32, y: f32, roof: f32) -> crate::ladder::LadderVolume {
    let loc_x = wall_x - 12.0 * dir;
    let outer = loc_x - 64.0 * dir;
    let depth = 130.0;
    let top = roof + 96.0;
    let bottom = -16.0;
    crate::ladder::LadderVolume::new(
        Vec3::new(outer + depth * 0.5 * dir, y, (top + bottom) * 0.5),
        Vec3::new(depth * 0.5, 40.0, (top - bottom) * 0.5),
        if dir > 0.0 { 0 } else { 32768 },
        crate::ladder::LadderType::Ladder,
    )
}

/// Lane H pipe, beside the tower's ladder on the same face (Right of it as you climb).
const PIPE_Y: f32 = 8220.0;
/// Top of the pipe's straight run; its elbow turns into the wall 120 above.
const PIPE_TOP: f32 = 1280.0;
/// The tutorial's pipes (TdLadderVolume_4/_5, LT_Pipe, bCanExitAtTop false) stand 120 off the
/// wall on elbows; their climb positions sit 23 behind the pipe, 97 off the wall, and the top
/// position is 18 over the pipe's straight run.
const PIPE_OFF_WALL: f32 = 120.0;
const PIPE_POSITIONS_OFF_WALL: f32 = 97.0;

/// A pipe's TdLadderVolume up the wall face at `wall_x` (climbed facing `dir`): positions laid
/// out like the tutorial's, the volume 64 past them on the open side and reaching the wall, its
/// top 96 over the top position and its bottom under the floor like the ladders'.
fn pipe_volume(wall_x: f32, dir: f32, y: f32, pipe_top: f32) -> crate::ladder::LadderVolume {
    let loc_x = wall_x - PIPE_POSITIONS_OFF_WALL * dir;
    let outer = loc_x - 64.0 * dir;
    let depth = PIPE_POSITIONS_OFF_WALL + 64.0;
    let top = pipe_top + 18.0 + 96.0;
    let bottom = -16.0;
    let mut v = crate::ladder::LadderVolume::new(
        Vec3::new(outer + depth * 0.5 * dir, y, (top + bottom) * 0.5),
        Vec3::new(depth * 0.5, 40.0, (top - bottom) * 0.5),
        if dir > 0.0 { 0 } else { 32768 },
        crate::ladder::LadderType::Pipe,
    );
    v.can_exit_at_top = false;
    v
}

/// The course's ladders: up the tower's -X face, up the low platform's +X face, and the pipe
/// beside the tower's ladder (climb it, then jump across to the ladder: GrabTransfer).
pub fn ladders() -> Vec<crate::ladder::LadderVolume> {
    vec![
        ladder_volume(TOWER_X, 1.0, LADDER_Y, TOWER_TOP),
        ladder_volume(LOW_X + 300.0, -1.0, LADDER_Y, LOW_TOP),
        pipe_volume(TOWER_X, 1.0, PIPE_Y, PIPE_TOP),
        ladder_volume(0.0, 1.0, ZIP_Y, ZIP_TOWER_TOP),
    ]
}

/// The tutorial's pipe pieces for the tower pipe: S_PipeSystem_03h straight runs (416 long,
/// origin at their upper end), S_PipeRound_01 clamps (at 0.25 scale) and the S_PipeSystem_03f
/// elbow into the wall at the top, as Tutorial_p places them around TdLadderVolume_5.
fn pipe_props() -> Vec<Prop> {
    let axis_x = TOWER_X - PIPE_OFF_WALL;
    let piece = |mesh: &'static str, location: Vec3, rot: [i32; 3], scale: f32, collides: bool| Prop {
        package: "Tutorial_p",
        mesh,
        material: None,
        location,
        pitch: rot[0],
        yaw: rot[1],
        roll: rot[2],
        scale: [scale; 3],
        soft_landing: false,
        collides,
    };
    let mut v = Vec::new();
    // straight runs pointing up (local X up), stacked down into the floor; not colliding, like
    // TdLadderVolume_4's pipe in the tutorial, so a GrabTransfer onto the pipe isn't blocked by it
    let mut z = PIPE_TOP;
    while z > 0.0 {
        v.push(piece("S_PipeSystem_03h", Vec3::new(axis_x, PIPE_Y, z), [16384, 0, 0], 1.0, false));
        z -= 416.0;
    }
    // the elbow: local X into the wall, local Y up from the pipe's top to the wall
    v.push(piece("S_PipeSystem_03f", Vec3::new(TOWER_X, PIPE_Y, PIPE_TOP + 120.0), [0, 0, -16384], 1.0, true));
    // clamps round the pipe (ring normal up); visual only, so they don't block the
    // GrabTransfer fit trace onto the pipe
    for cz in [40.0, 440.0, 840.0, PIPE_TOP - 40.0] {
        v.push(piece("S_PipeRound_01", Vec3::new(axis_x, PIPE_Y, cz), [0, 0, -16384], 0.25, false));
    }
    v
}

/// Lane I's game meshes, as Tutorial_p builds its swing bars and zipline: S_SwingPole_01c
/// bars (128 long along their local -X) with S_SwingPole_01d end caps, and the zipline's
/// S_Cable_01 (224 long along its local -Y, stretched to the cable's length) from an
/// S_Antenna_10 mast on the tower to an S_ZipLineBase_01c anchor on the building.
fn swing_zip_props() -> Vec<Prop> {
    let piece = |mesh: &'static str, location: Vec3, rot: [i32; 3], scale: [f32; 3], collides: bool| Prop {
        package: "Tutorial_p",
        mesh,
        material: None,
        location,
        pitch: rot[0],
        yaw: rot[1],
        roll: rot[2],
        scale,
        soft_landing: false,
        collides,
    };
    let mut v = Vec::new();
    for x in [SWING_X, SWING_X2] {
        // the bar from y - 64 to y + 64 (local X along world -Y), a cap at each end facing out
        v.push(piece("S_SwingPole_01c", Vec3::new(x, SWING_Y - 64.0, SWING_GRIP), [0, -16384, 0], [1.0; 3], false));
        v.push(piece("S_SwingPole_01d", Vec3::new(x, SWING_Y - 64.0, SWING_GRIP), [0, -16384, 0], [1.0; 3], false));
        v.push(piece("S_SwingPole_01d", Vec3::new(x, SWING_Y + 64.0, SWING_GRIP), [0, 16384, 0], [1.0; 3], false));
    }
    // the cable: local -Y from the start towards the end, local Z up; a roll tilts -Y down the
    // slope (Y = (cos h cos r, sin h cos r, -sin r) for yaw h - 90 deg)
    let d = ZIP_START - ZIP_END;
    let len = d.length();
    let heading = d.y.atan2(d.x) * crate::math::URU_PER_RAD;
    let elevation = d.z.atan2((d.x * d.x + d.y * d.y).sqrt()) * crate::math::URU_PER_RAD;
    v.push(piece("S_Cable_01", ZIP_START, [0, heading as i32 - 16384, -(elevation as i32)], [4.0, len / 224.0, 4.0], false));
    // the mast stands beside the run-up so it doesn't block the jump at the cable
    v.push(piece("S_Antenna_10", Vec3::new(ZIP_TOWER_X - 40.0, ZIP_Y + 100.0, ZIP_TOWER_TOP), [0, 0, 0], [1.0; 3], true));
    v.push(piece("S_ZipLineBase_01c", ZIP_END, [0, 0, 0], [1.0; 3], false));
    // the balance pipe: three straight runs end to end along +X (local X back along the run)
    for k in 0..3 {
        let x = BAL_X0 + 416.0 * (k + 1) as f32;
        v.push(piece("S_PipeSystem_03h", Vec3::new(x, PIPE_BEAM_Y, -PIPE_BEAM_R), [0, 0, 0], [1.0; 3], true));
    }
    v
}

/// Game meshes in the course: the tutorial's ladder pieces up each ladder (128 tall segments
/// ending at the last climb position, the hooked top piece above it) and red
/// construction-package stacks, Mirror's Edge's soft landings, between the tower and the low
/// platform.
pub fn props() -> Vec<Prop> {
    let mut v = pipe_props();
    v.extend(swing_zip_props());
    for l in ladders().into_iter().filter(|l| l.ladder_type == crate::ladder::LadderType::Ladder) {
        let last = l.pawn_ladder_locations.last().copied().unwrap_or(Vec3::ZERO);
        // the tutorial's pieces face +Y for a ladder whose volume faces +X
        let yaw = l.rotation.yaw + 16384;
        let ladder = |mesh: &'static str, z: f32| Prop {
            package: "Tutorial_p",
            mesh,
            material: None,
            location: Vec3::new(last.x, last.y, z),
            yaw,
            pitch: 0,
            roll: 0,
            scale: [1.0; 3],
            soft_landing: false,
            // the tutorial's ladder pieces mostly don't collide (the hooked top never does)
            collides: false,
        };
        let mut z = last.z;
        while z > 0.0 {
            v.push(ladder("S_LadderSystem_01b", z));
            z -= 128.0;
        }
        v.push(ladder("S_LadderSystem_01a", last.z));
    }
    // a 5 x 3 bed of stacks (each 234 x 165, overlapping a little) from the tower's far edge out
    // to where a running jump off the roof comes down
    let bags = (0..5).flat_map(|i| (0..3).map(move |j| (TOWER_X + 620.0 + 200.0 * i as f32, LADDER_Y - 150.0 + 150.0 * j as f32, 0)));
    for (x, y, yaw) in bags {
        v.push(Prop {
            package: "P_Renovation",
            mesh: "S_ConstructionPackages_01a",
            material: Some("MI_ConstructionPackages_01_RED"),
            location: Vec3::new(x, y, 0.0),
            yaw,
            pitch: 0,
            roll: 0,
            scale: [1.0; 3],
            soft_landing: true,
            collides: true,
        });
    }
    v
}

pub fn spawns() -> Vec<Spawn> {
    vec![
        Spawn { name: "A sprint strip", feet: Vec3::new(-400.0, 0.0, 0.0), yaw: 0 },
        Spawn { name: "B step-ups and vaults", feet: Vec3::new(-400.0, 1200.0, 0.0), yaw: 0 },
        Spawn { name: "C wall run / wall climb", feet: Vec3::new(-400.0, 2150.0, 0.0), yaw: 0 },
        Spawn { name: "D gaps and drops", feet: Vec3::new(-400.0, 3600.0, 0.0), yaw: 0 },
        Spawn { name: "E slide, crouch, ramps", feet: Vec3::new(-400.0, 4800.0, 0.0), yaw: 0 },
        Spawn { name: "F hang, shimmy, grab transfer", feet: Vec3::new(-400.0, 6000.0, 0.0), yaw: 0 },
        Spawn { name: "G springboard", feet: Vec3::new(-400.0, 7200.0, 0.0), yaw: 0 },
        Spawn { name: "H tower, ladder, landing bags", feet: Vec3::new(-400.0, 8400.0, 0.0), yaw: 0 },
        Spawn { name: "H low platform (6 m) over the landing bags", feet: Vec3::new(3550.0, 8400.0, 0.0), yaw: 32768 },
        Spawn { name: "I swing bars (run and jump at the bar)", feet: Vec3::new(-400.0, SWING_Y, 0.0), yaw: 0 },
        Spawn { name: "I zipline (run off the roof edge and jump at the cable)", feet: Vec3::new(-150.0 + ZIP_TOWER_X * 0.5, ZIP_Y, ZIP_TOWER_TOP), yaw: 0 },
        Spawn { name: "J balance: pipe", feet: Vec3::new(-300.0, PIPE_BEAM_Y, 0.0), yaw: 0 },
        Spawn { name: "J balance: narrow beam", feet: Vec3::new(-300.0, BEAM_Y, 0.0), yaw: 0 },
        Spawn { name: "K combat arena (B spawns an enemy, LMB attack / fire, RMB disarm / drop / pick up)", feet: Vec3::new(-300.0, 12500.0, 0.0), yaw: 0 },
        Spawn { name: "K ledge: run off and attack onto an enemy below (B spawns one ahead)", feet: Vec3::new(2000.0, 12650.0, 350.0), yaw: 32768 },
        Spawn { name: "K tower (600): B spawns an enemy at its edge; knock it off", feet: Vec3::new(2350.0, 12050.0, 600.0), yaw: 32768 },
    ]
}

/// Corners of a block in the order WorldBuilder::add_hexahedron expects.
pub fn corners(bl: &Block) -> [Vec3; 8] {
    let c = (bl.min + bl.max) * 0.5;
    let h = (bl.max - bl.min) * 0.5;
    let (sy, cy) = bl.yaw.sin_cos();
    let (sp, cp) = bl.pitch.sin_cos();
    std::array::from_fn(|k| {
        let mut l = Vec3::new(
            if k & 1 == 0 { -h.x } else { h.x },
            if k & 2 == 0 { -h.y } else { h.y },
            if k & 4 == 0 { -h.z } else { h.z },
        );
        if bl.pitch != 0.0 {
            // ramps: rotate about Y at the low edge so the top surface rises along +X
            let x = l.x + h.x;
            let z = l.z + h.z;
            let rx = x * cp - z * sp;
            let rz = x * sp + z * cp;
            l = Vec3::new(rx - h.x, l.y, rz - h.z);
        }
        c + Vec3::new(l.x * cy - l.y * sy, l.x * sy + l.y * cy, l.z)
    })
}

pub fn world() -> World {
    let mut wb = WorldBuilder::default();
    let s = wb.surface(Surface::default());
    for bl in blocks() {
        wb.add_hexahedron(corners(&bl), s);
    }
    wb.build()
}
