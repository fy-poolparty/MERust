//! World collision queries with UE3 semantics.
//!
//! UE3 traces a pawn through the world as an axis-aligned box with half extents
//! (CollisionRadius, CollisionRadius, CollisionHeight), not as a cylinder; only pawn-vs-pawn uses
//! the cylinder. Zero-extent traces are rays. A blocking hit is pulled back along the trace so the
//! mover stops just short of the surface: UStaticMeshComponent::LineCheck does
//! `Time = Clamp(Time - Clamp(0.1, 0.1/Dist, 1/Dist), 0, 1)`, i.e. 0.1 to 1 uu.

use crate::math::Vec3;
use parry3d::math::Pose;
use parry3d::query::{PointQueryWithLocation, Ray, RayCast, ShapeCastOptions, cast_shapes, intersection_test};
use parry3d::shape::{Cuboid, TriMesh};

/// Per-surface flags carried over from the actor that owns the geometry (Mirror's Edge adds
/// `bExludeHandMoves` / `bExludeFootMoves` to Actor; FindLedge honours them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Surface {
    pub exclude_hand_moves: bool,
    pub exclude_foot_moves: bool,
    /// TdPhysicalMaterialProperty soft-landing flag (mattresses, boxes): Moves[SoftLanding].
    pub soft_landing: bool,
}

/// `FCheckResult`.
#[derive(Clone, Copy, Debug)]
pub struct CheckResult {
    /// Fraction of the trace completed, after pull-back. 1 when nothing was hit.
    pub time: f32,
    pub location: Vec3,
    /// Points out of the surface, towards the tracer.
    pub normal: Vec3,
    /// Something blocking was hit (`Hit.Actor != None`).
    pub hit: bool,
    pub surface: Surface,
}

impl CheckResult {
    pub fn none(end: Vec3) -> Self {
        Self { time: 1.0, location: end, normal: Vec3::ZERO, hit: false, surface: Surface::default() }
    }
}

pub struct World {
    mesh: TriMesh,
    tri_surface: Vec<u16>,
    surfaces: Vec<Surface>,
}

impl World {
    /// `surface_of_tri[i]` indexes `surfaces` for triangle `i` (empty = all default).
    pub fn new(vertices: Vec<Vec3>, indices: Vec<[u32; 3]>, surfaces: Vec<Surface>, surface_of_tri: Vec<u16>) -> Self {
        let n = indices.len();
        let mesh = TriMesh::new(vertices, indices).expect("valid collision mesh");
        let tri_surface = if surface_of_tri.len() == n { surface_of_tri } else { vec![0; n] };
        let surfaces = if surfaces.is_empty() { vec![Surface::default()] } else { surfaces };
        Self { mesh, tri_surface, surfaces }
    }

    pub fn triangle_count(&self) -> usize {
        self.tri_surface.len()
    }

    fn surface_at(&self, p: Vec3) -> Surface {
        let (_, (tri, _)) = self.mesh.project_local_point_and_get_location(p, true);
        self.surfaces[*self.tri_surface.get(tri as usize).unwrap_or(&0) as usize]
    }

    /// `GWorld->SingleLineCheck(Hit, Owner, End, Start, TRACE_World, Extent)`.
    pub fn line_check(&self, end: Vec3, start: Vec3, extent: Vec3) -> CheckResult {
        let delta = end - start;
        let dist = delta.length();
        if dist < 1e-6 {
            return CheckResult::none(end);
        }
        let (toi, normal, witness) = if extent.x == 0.0 && extent.y == 0.0 && extent.z == 0.0 {
            // Unreal's triangle collision is one-sided: a ray only stops on a front face, so one
            // starting inside a block passes out through its far side instead of "hitting" it.
            let mut t0 = 0.0f32;
            let mut found = None;
            for _ in 0..16 {
                let ray = Ray::new(start + delta * t0, delta);
                let Some(h) = self.mesh.cast_local_ray_and_get_normal(&ray, 1.0 - t0, false) else { break };
                let t = t0 + h.time_of_impact;
                let tri = self.mesh.triangle(h.subshape);
                let n = (tri.b - tri.a).cross(tri.c - tri.a);
                if n.dot(delta) < 0.0 {
                    found = Some((t, n, start + delta * t));
                    break;
                }
                t0 = t + 0.01 / dist;
                if t0 >= 1.0 {
                    break;
                }
            }
            match found {
                Some(f) => f,
                None => return CheckResult::none(end),
            }
        } else {
            let pose = Pose::from_translation(start);
            let shape = Cuboid::new(extent);
            let opts = ShapeCastOptions {
                max_time_of_impact: 1.0,
                target_distance: 0.0,
                stop_at_penetration: false,
                compute_impact_geometry_on_penetration: true,
            };
            match cast_shapes(&pose, delta, &shape, &Pose::identity(), Vec3::ZERO, &self.mesh, opts) {
                Ok(Some(h)) => (h.time_of_impact, h.normal2, h.witness2),
                _ => return CheckResult::none(end),
            }
        };
        let mut n = normal;
        if !n.is_finite() || n.length_squared() < 1e-8 {
            n = -delta / dist;
        } else {
            n = n.normalize();
            // Normals face the tracer.
            if n.dot(delta) > 0.0 {
                n = -n;
            }
        }
        let pull = 0.1f32.clamp(0.1 / dist, 1.0 / dist);
        let time = (toi - pull).clamp(0.0, 1.0);
        CheckResult { time, location: start + delta * time, normal: n, hit: true, surface: self.surface_at(witness) }
    }

    /// The collision triangles (vertices, indices).
    pub fn triangles(&self) -> (Vec<Vec3>, Vec<[u32; 3]>) {
        (self.mesh.vertices().to_vec(), self.mesh.indices().to_vec())
    }

    /// `PointCheck` / encroachment test: does a box of `extent` at `location` overlap the world?
    pub fn point_check(&self, location: Vec3, extent: Vec3) -> bool {
        let pose = Pose::from_translation(location);
        intersection_test(&pose, &Cuboid::new(extent), &Pose::identity(), &self.mesh).map(|r| r.intersecting).unwrap_or(false)
    }
}

/// Builds axis-aligned or rotated boxes into a triangle soup (test maps).
#[derive(Default)]
pub struct WorldBuilder {
    pub vertices: Vec<Vec3>,
    pub indices: Vec<[u32; 3]>,
    pub surfaces: Vec<Surface>,
    pub tri_surface: Vec<u16>,
}

impl WorldBuilder {
    pub fn surface(&mut self, s: Surface) -> u16 {
        if let Some(i) = self.surfaces.iter().position(|x| *x == s) {
            return i as u16;
        }
        self.surfaces.push(s);
        (self.surfaces.len() - 1) as u16
    }

    /// Box from 8 corners in the order (-x-y-z, +x-y-z, -x+y-z, +x+y-z, ...z+), outward faces.
    pub fn add_hexahedron(&mut self, c: [Vec3; 8], surface: u16) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&c);
        const F: [[u32; 3]; 12] = [
            [0, 2, 1], [1, 2, 3], [4, 5, 6], [5, 7, 6], [0, 1, 4], [1, 5, 4],
            [2, 6, 3], [3, 6, 7], [0, 4, 2], [2, 4, 6], [1, 3, 5], [3, 7, 5],
        ];
        for f in F {
            self.indices.push([f[0] + base, f[1] + base, f[2] + base]);
            self.tri_surface.push(surface);
        }
    }

    pub fn add_box(&mut self, center: Vec3, half: Vec3, yaw_rad: f32, surface: u16) {
        let (s, c) = yaw_rad.sin_cos();
        let corners = std::array::from_fn(|k| {
            let l = Vec3::new(
                if k & 1 == 0 { -half.x } else { half.x },
                if k & 2 == 0 { -half.y } else { half.y },
                if k & 4 == 0 { -half.z } else { half.z },
            );
            center + Vec3::new(l.x * c - l.y * s, l.x * s + l.y * c, l.z)
        });
        self.add_hexahedron(corners, surface);
    }

    /// Adds an arbitrary triangle list (level geometry).
    pub fn add_mesh(&mut self, vertices: &[Vec3], indices: &[[u32; 3]], surface: u16) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(vertices);
        for t in indices {
            self.indices.push([t[0] + base, t[1] + base, t[2] + base]);
            self.tri_surface.push(surface);
        }
    }

    pub fn build(self) -> World {
        World::new(self.vertices, self.indices, self.surfaces, self.tri_surface)
    }
}
