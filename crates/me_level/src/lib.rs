//! Builds a playable level from the user's Mirror's Edge install: world-space render batches
//! and one collision mesh, all in Unreal space. Engine-agnostic (no Bevy) so the headless
//! harness and tests can run on real maps.

use tdsim::{World as MeshWorld, Vec3};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use upk::level::mesh_instances;
use upk::staticmesh::{StaticMesh, read_static_mesh};
use upk::texture::FileIndex;
use upk::{Package, export_props, find};

pub mod anims;
pub mod camera;
pub mod decode;
pub mod materials;
pub mod pose;
pub mod ragdoll;
pub mod sounds;
pub use materials::{Blend, MaterialInfo, Materials, TextureData};
pub use upk::texture::Format as TextureFormat;

/// The packages that make up a map: persistent level first, then its streamed sublevels.
pub const TUTORIAL: &[&str] = &["Tutorial_p", "Tutorial_Art", "Tutorial_Bac", "Tutorial_LW"];

/// Render batches are split on this grid (uu) so frustum culling still works.
const CELL: f32 = 6400.0;

#[derive(Default)]
pub struct Batch {
    /// Index into `Level::materials`.
    pub material: usize,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Vertex colours (RGBA), only for materials that use them (the sky).
    pub colors: Vec<[u8; 4]>,
    /// Unreal winding (clockwise when viewed from the front in a left-handed frame).
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct Start {
    pub name: String,
    pub challenges: Vec<String>,
    /// Cylinder centre, Unreal space.
    pub location: Vec3,
    pub yaw: f32,
}

pub struct Level {
    pub batches: Vec<Batch>,
    pub materials: Vec<MaterialInfo>,
    pub textures: Vec<TextureData>,
    pub collision: MeshWorld,
    pub collision_tris: usize,
    pub starts: Vec<Start>,
    pub instances: usize,
}

/// Find the install: explicit path, `MIRRORS_EDGE_DIR`, or a few usual locations.
pub fn find_install(explicit: Option<&str>) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = explicit {
        candidates.push(p.into());
    }
    if let Ok(p) = std::env::var("MIRRORS_EDGE_DIR") {
        candidates.push(p.into());
    }
    if let Some(p) = remembered_install() {
        candidates.push(p);
    }
    for p in [
        r"C:\Program Files (x86)\Steam\steamapps\common\Mirrors Edge",
        r"C:\Program Files (x86)\Origin Games\Mirror's Edge",
        r"C:\Program Files\EA Games\Mirror's Edge",
    ] {
        candidates.push(p.into());
    }
    candidates.into_iter().find(|p| is_install(p))
}

/// A Mirror's Edge install folder (it has TdGame\CookedPC).
pub fn is_install(p: &Path) -> bool {
    p.join("TdGame").join("CookedPC").is_dir()
}

/// Where the chosen install folder is remembered, per user (not in the project, so nothing
/// about anyone's machine ends up in the repository): %APPDATA%\mirrors-edge-rust\install.txt,
/// or ~/.config/mirrors-edge-rust/install.txt.
pub fn install_memory_file() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("mirrors-edge-rust").join("install.txt")
}

pub fn remembered_install() -> Option<PathBuf> {
    let s = std::fs::read_to_string(install_memory_file()).ok()?;
    let p = PathBuf::from(s.trim());
    is_install(&p).then_some(p)
}

/// Remember the install folder for next time.
pub fn remember_install(p: &Path) -> std::io::Result<()> {
    let f = install_memory_file();
    if let Some(d) = f.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(f, p.to_string_lossy().as_bytes())
}

/// The install for tests and tools: find_install(None), or a path that doesn't exist (the
/// install-dependent tests then skip or fail to open their packages).
pub fn install() -> &'static Path {
    static INSTALL: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    INSTALL.get_or_init(|| find_install(None).unwrap_or_else(|| PathBuf::from("<Mirror's Edge install not found: set MIRRORS_EDGE_DIR or pick it in the game menu>")))
}

fn find_package(cooked: &Path, name: &str) -> Option<PathBuf> {
    fn walk(dir: &Path, want: &str, out: &mut Option<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, want, out);
            } else if p.file_stem().is_some_and(|s| s.eq_ignore_ascii_case(want))
                && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("me1") || x.eq_ignore_ascii_case("upk"))
            {
                *out = Some(p);
            }
            if out.is_some() {
                return;
            }
        }
    }
    let mut out = None;
    walk(&cooked.join("Maps"), name, &mut out);
    if out.is_none() {
        walk(cooked, name, &mut out);
    }
    out
}

fn starts_in(pkg: &Package) -> Vec<Start> {
    let mut out = Vec::new();
    for i in 0..pkg.exports.len() {
        let class = pkg.export_class(i);
        if !(class.ends_with("PlayerStart") || class == "TdTutorialStart") {
            continue;
        }
        let Ok((props, _)) = export_props(pkg, i) else { continue };
        let d = pkg.export_bytes(i);
        let Some(loc) = find(&props, "Location").and_then(|p| p.as_vec3(d)) else { continue };
        let yaw = find(&props, "Rotation").and_then(|p| p.as_rotator(d)).map_or(0, |r| r[1]);
        let challenges = find(&props, "BelongToChallenge")
            .map(|p| {
                p.bytes(d)[4..]
                    .chunks_exact(8)
                    .map(|c| {
                        let idx = i32::from_le_bytes(c[0..4].try_into().unwrap());
                        pkg.names.get(idx as usize).cloned().unwrap_or_default().trim_start_matches("EMC_").to_string()
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(Start {
            name: pkg.name(pkg.exports[i].name),
            challenges,
            location: Vec3::from(loc),
            yaw: (yaw.rem_euclid(65536) as f32) * (std::f32::consts::TAU / 65536.0),
        });
    }
    out
}

/// Load and flatten a map. `packages` are package names (without extension) under CookedPC/Maps.
/// max_texture caps the top mip loaded (textures are uploaded compressed, mips included).
pub fn load_map(install: &Path, packages: &[&str], max_texture: u32) -> Result<Level, Box<dyn std::error::Error>> {
    let cooked = install.join("TdGame").join("CookedPC");
    let mut files = FileIndex::new(&cooked);
    let mut mats = Materials::new(max_texture);
    let mut cells: HashMap<(usize, [i32; 3]), Batch> = HashMap::new();
    let mut col_v: Vec<Vec3> = Vec::new();
    let mut col_i: Vec<[u32; 3]> = Vec::new();
    let mut starts = Vec::new();
    let mut instances = 0;

    for name in packages {
        let path = find_package(&cooked, name).ok_or_else(|| format!("package {name} not found under {}", cooked.display()))?;
        let pkg = Package::open(&path)?;
        starts.extend(starts_in(&pkg));
        let mut meshes: HashMap<i32, Option<StaticMesh>> = HashMap::new();
        for inst in mesh_instances(&pkg) {
            if inst.mesh <= 0 {
                continue; // imported from another package; not resolved yet
            }
            let Some(mesh) = meshes.entry(inst.mesh).or_insert_with(|| read_static_mesh(&pkg, inst.mesh as usize - 1).ok()) else {
                continue;
            };
            instances += 1;
            let mut out = Out { cells: &mut cells, col_v: &mut col_v, col_i: &mut col_i };
            add_instance(&pkg, &mut files, &mut mats, &mut out, mesh, &inst.transform, &inst.materials, inst.collides, inst.visible);
        }
    }

    let collision_tris = col_i.len();
    let collision = MeshWorld::new(col_v, col_i, Vec::new(), Vec::new());
    let mut batches: Vec<Batch> = cells.into_values().filter(|b| !b.indices.is_empty()).collect();
    batches.sort_by(|a, b| a.material.cmp(&b.material));
    Ok(Level { batches, materials: mats.infos, textures: mats.textures, collision, collision_tris, starts, instances })
}

struct Out<'a> {
    cells: &'a mut HashMap<(usize, [i32; 3]), Batch>,
    col_v: &'a mut Vec<Vec3>,
    col_i: &'a mut Vec<[u32; 3]>,
}

/// One placed static mesh: render batches per material and grid cell, plus its collision.
#[allow(clippy::too_many_arguments)]
fn add_instance(
    pkg: &Package,
    files: &mut FileIndex,
    mats: &mut Materials,
    out: &mut Out,
    mesh: &StaticMesh,
    xf: &upk::level::Affine,
    materials: &[i32],
    collides: bool,
    visible: bool,
) {
    let xf = *xf;
        let flip = xf.determinant() < 0.0;
        let world: Vec<[f32; 3]> = mesh.positions.iter().map(|&p| xf.point(p)).collect();

        // World-space half extents of the mesh bounds.
        let e = mesh.bounds_extent;
        let ext: [f32; 3] =
            std::array::from_fn(|k| (xf.x[k] * e[0]).abs() + (xf.y[k] * e[1]).abs() + (xf.z[k] * e[2]).abs());
        // Tiny props (cans, litter, bolts) and purely translucent meshes (litter cards, decal
        // meshes) don't stop a runner: they would otherwise snag the cylinder on every bump.
        let tiny = ext.iter().all(|&x| x < 25.0);
        let see_through = !mesh.sections.is_empty()
            && mesh.sections.iter().enumerate().all(|(si, s)| {
                let mat = materials.get(si).copied().filter(|&m| m != 0).unwrap_or(s.material);
                let mi = mats.resolve(pkg, files, mat);
                matches!(mats.infos[mi].blend, Blend::Translucent | Blend::Additive | Blend::Modulate)
                    && mats.infos[mi].opacity.is_none()
            });
        if collides && !tiny && !see_through {
            let base = out.col_v.len() as u32;
            out.col_v.extend(world.iter().map(|&p| Vec3::from(p)));
            // tdsim wants outward = (b - a) x (c - a): Unreal's static meshes wind the other
            // way, and mirrored instances flip it again
            out.col_i.extend(mesh.collision.iter().map(|t| if flip { [t[0] + base, t[1] + base, t[2] + base] } else { [t[0] + base, t[2] + base, t[1] + base] }));
        }
        if !visible {
            return;
        }

        let center = xf.point(mesh.bounds_origin);
        let cell = [(center[0] / CELL).floor() as i32, (center[1] / CELL).floor() as i32, (center[2] / CELL).floor() as i32];
        for (si, s) in mesh.sections.iter().enumerate() {
            let mat = materials.get(si).copied().filter(|&m| m != 0).unwrap_or(s.material);
            let mi = mats.resolve(pkg, files, mat);
            let sky = mats.infos[mi].sky;
            let batch = out.cells.entry((mi, cell)).or_insert_with(|| Batch { material: mi, ..Default::default() });
            let mut remap: HashMap<u32, u32> = HashMap::new();
            let first = s.first_index as usize;
            let end = (first + s.num_faces as usize * 3).min(mesh.indices.len());
            let mut tri = [0u32; 3];
            for (k, &vi) in mesh.indices[first..end].iter().enumerate() {
                let idx = *remap.entry(vi).or_insert_with(|| {
                    let n = xf.vector(mesh.normals[vi as usize]);
                    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
                    let s = if flip { -1.0 } else { 1.0 };
                    batch.positions.push(world[vi as usize]);
                    batch.normals.push([s * n[0] / l, s * n[1] / l, s * n[2] / l]);
                    batch.uvs.push(mesh.uvs[vi as usize]);
                    if sky {
                        batch.colors.push(mesh.colors[vi as usize]);
                    }
                    batch.positions.len() as u32 - 1
                });
                tri[k % 3] = idx;
                if k % 3 == 2 {
                    if flip {
                        batch.indices.extend([tri[0], tri[2], tri[1]]);
                    } else {
                        batch.indices.extend(tri);
                    }
                }
            }
        }
}

/// Props placed in the test course: render batches plus each prop's collision in world space
/// with its soft-landing flag.
pub struct Props {
    pub level: Level,
    pub collision: Vec<(Vec<Vec3>, Vec<[u32; 3]>, bool)>,
}

pub fn load_props(install: &Path, props: &[tdsim::testmap::Prop], max_texture: u32) -> Result<Props, Box<dyn std::error::Error>> {
    let cooked = install.join("TdGame").join("CookedPC");
    let mut files = FileIndex::new(&cooked);
    let mut mats = Materials::new(max_texture);
    let mut cells: HashMap<(usize, [i32; 3]), Batch> = HashMap::new();
    let mut pkgs: HashMap<&str, Package> = HashMap::new();
    let mut meshes: HashMap<(&str, &str), StaticMesh> = HashMap::new();
    let mut collision = Vec::new();
    for p in props {
        if !pkgs.contains_key(p.package) {
            let path = find_package(&cooked, p.package).ok_or_else(|| format!("package {} not found", p.package))?;
            pkgs.insert(p.package, Package::open(&path)?);
        }
        let pkg = &pkgs[p.package];
        if !meshes.contains_key(&(p.package, p.mesh)) {
            let i = pkg.find_export(p.mesh, Some("StaticMesh")).ok_or_else(|| format!("{} not in {}", p.mesh, p.package))?;
            meshes.insert((p.package, p.mesh), read_static_mesh(pkg, i)?);
        }
        let mesh = &meshes[&(p.package, p.mesh)];
        let material = p.material.and_then(|m| pkg.find_export(m, None)).map(|i| i as i32 + 1).unwrap_or(0);
        let materials = vec![material; mesh.sections.len()];
        let xf = upk::level::Affine::srt(p.scale, [p.pitch, p.yaw, p.roll], [p.location.x, p.location.y, p.location.z]);
        let (mut col_v, mut col_i) = (Vec::new(), Vec::new());
        let mut out = Out { cells: &mut cells, col_v: &mut col_v, col_i: &mut col_i };
        add_instance(pkg, &mut files, &mut mats, &mut out, mesh, &xf, &materials, p.collides, true);
        collision.push((col_v, col_i, p.soft_landing));
    }
    let mut batches: Vec<Batch> = cells.into_values().filter(|b| !b.indices.is_empty()).collect();
    batches.sort_by(|a, b| a.material.cmp(&b.material));
    let level = Level {
        batches,
        materials: mats.infos,
        textures: mats.textures,
        // unused: the props' collision goes into testmap_world (one far-away triangle keeps the
        // collision structure valid)
        collision: MeshWorld::new(vec![Vec3::new(0.0, 0.0, -1e6), Vec3::new(1.0, 0.0, -1e6), Vec3::new(0.0, 1.0, -1e6)], vec![[0, 1, 2]], Vec::new(), Vec::new()),
        collision_tris: 0,
        starts: Vec::new(),
        instances: props.len(),
    };
    Ok(Props { level, collision })
}

/// The test course's collision: the blocks plus the props (soft ones flagged for soft landing).
pub fn testmap_world(props: &Props) -> MeshWorld {
    let mut wb = tdsim::WorldBuilder::default();
    let s = wb.surface(tdsim::collision::Surface::default());
    let soft = wb.surface(tdsim::collision::Surface { soft_landing: true, ..Default::default() });
    for bl in tdsim::testmap::blocks() {
        wb.add_hexahedron(tdsim::testmap::corners(&bl), s);
    }
    for (v, i, is_soft) in &props.collision {
        wb.add_mesh(v, i, if *is_soft { soft } else { s });
    }
    wb.build()
}
