//! ATdPlayerPawn::CheckForLedges (0x12BE0F0) with its edge clamp (0x12B8A30), and
//! TdMove_Vertigo: walking slowly at a real drop stops you for a look over the edge, crouching
//! keeps you from walking off it.

use crate::config::Config;
use crate::math::{Rotator, UeVec, Vec3};
use crate::pawn::{Move, MoveActionHint, Slot, WalkingState};
use crate::sim::Sim;

#[derive(Clone, Debug, Default)]
pub struct Vertigo {
    pub last_vertigo_edge_position: Vec3,
    pub last_actual_vertigo_edge_position: Vec3,
    pub zoom_fov: f32,
    pub zoom_rate: f32,
    pub zoom_out_time: f32,
}

/// [TdGame.TdPlayerPawn] edge probing settings.
#[derive(Clone, Debug)]
pub struct EdgeConfig {
    pub vertigo_edge_probing_height: f32,
    pub vertigo_edge_probing_distance: f32,
    pub vertigo_effect_threshold: f32,
    pub edge_check_max_speed: f32,
    pub edge_check_distance: f32,
    pub edge_stop_min_height: f32,
}

impl EdgeConfig {
    pub fn new(cfg: &Config) -> Self {
        let ch = &["TdPlayerPawn"];
        EdgeConfig {
            vertigo_edge_probing_height: cfg.f32(ch, "VertigoEdgeProbingHeight", 1000.0),
            vertigo_edge_probing_distance: cfg.f32(ch, "VertigoEdgeProbingDistance", 70.0),
            vertigo_effect_threshold: cfg.f32(ch, "VertigoEffectThreshold", 0.9),
            edge_check_max_speed: cfg.f32(ch, "EdgeCheckMaxSpeed", 300.0),
            edge_check_distance: cfg.f32(ch, "EdgeCheckDistance", 20.0),
            edge_stop_min_height: cfg.f32(ch, "EdgeStopMinHeight", 36.0),
        }
    }
}

impl Vertigo {
    pub fn new(cfg: &Config) -> Self {
        let ch = &["TdMove_Vertigo", "TdPhysicsMove", "TdMove"];
        Vertigo {
            zoom_fov: cfg.f32(ch, "ZoomFOV", 84.0),
            zoom_rate: cfg.f32(ch, "ZoomRate", 30.0),
            zoom_out_time: cfg.f32(ch, "ZoomOutTime", 1.2),
            ..Default::default()
        }
    }
}

/// FVector::SafeNormal2D as these natives inline it (a flat unit vector passes untouched).
fn normal_2d(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, 0.0).safe_normal()
}

impl Sim {
    /// ATdPlayerPawn::CheckForLedges: crouched, the move is clamped at the edge; otherwise an
    /// edge with nothing for VertigoEdgeProbingHeight below, met head on at walking pace,
    /// starts TdMove_Vertigo (and holds the pawn still while in it).
    pub(crate) fn check_for_ledges(&mut self, _accel_dir: Vec3, delta: Vec3, _grav: Vec3, _checked_fall: &mut bool) -> (Vec3, bool) {
        let mut stay = true;
        let ms = self.pawn.movement_state;
        if self.pawn.avoid_ledges && self.moves.base(ms).avoid_ledges {
            let (clamped, edge) = self.clamp_delta_to_edge(delta);
            if clamped != delta {
                if ms == Move::Crouch {
                    return (clamped, false);
                }
                let dir = (delta - clamped).safe_normal();
                let e = self.edge.clone();
                let loc = self.pawn.location;
                let start = loc + dir * e.vertigo_edge_probing_distance;
                let mut end = start;
                end.z -= e.vertigo_edge_probing_height;
                let h = self.world.line_check(end, start, Vec3::ZERO);
                if !h.hit {
                    stay = false;
                    let d = (start - loc).safe_normal();
                    let facing = self.pawn.rotation.vector().safe_normal();
                    if facing.dot(d).abs() > e.vertigo_effect_threshold
                        && ms == Move::Walking
                        && matches!(self.pawn.current_walking_state, WalkingState::Idle | WalkingState::Sneak | WalkingState::Walk | WalkingState::Jog)
                        && (edge - self.moves.vertigo.last_actual_vertigo_edge_position).length() >= 2.0
                        && self.can_do_move(Move::Vertigo)
                    {
                        self.moves.vertigo.last_vertigo_edge_position = start;
                        self.moves.vertigo.last_actual_vertigo_edge_position = edge;
                        self.set_move(Move::Vertigo, false, false);
                    }
                    if self.pawn.movement_state == Move::Vertigo {
                        return (Vec3::ZERO, false);
                    }
                }
            }
        }
        if stay && self.pawn.movement_state == Move::Vertigo {
            self.set_move(Move::Walking, false, false);
        }
        (delta, false)
    }

    /// ATdPlayerPawn 0x12B8A30: three probes down past the feet - straight ahead of the move
    /// and to its front-left and front-right - for a drop deeper than MaxStepHeight. Under
    /// EdgeCheckMaxSpeed with no up/down hint, a found edge's face (traced for at foot level)
    /// takes the move's component into it away, or stops the move between two edges.
    /// Returns the delta and the edge spot.
    pub fn clamp_delta_to_edge(&mut self, delta: Vec3) -> (Vec3, Vec3) {
        let e = self.edge.clone();
        let p = &self.pawn;
        let loc = p.location;
        let h = p.collision_height;
        let r = p.collision_radius;
        let wfz = p.walkable_floor_z;
        let len2 = delta.x * delta.x + delta.y * delta.y;
        let dir = normal_2d(delta);
        let ecd = e.edge_check_distance;
        let drop = h + e.edge_stop_min_height + 4.0;
        let ratio = (h + p.max_step_height) / drop;
        let probe = |s: &Sim, at: Vec3| {
            let end = at - Vec3::new(0.0, 0.0, drop);
            s.world.line_check(end, at, Vec3::ZERO)
        };
        let open = |c: &crate::collision::CheckResult| c.time > ratio || wfz > c.normal.z;
        let none = |c: &crate::collision::CheckResult| c.time == 1.0 || wfz > c.normal.z;
        let mut edge_spot = Vec3::ZERO;
        // ahead
        let t1 = probe(self, loc + delta + dir * ecd);
        let perp = Vec3::new(-dir.y, dir.x, 0.0);
        let diag_r = perp + dir;
        let diag_l = dir - perp;
        let half = len2.sqrt() * std::f32::consts::FRAC_1_SQRT_2;
        let side = half + ecd;
        let t2 = probe(self, loc + diag_r * side);
        let speed2 = self.pawn.velocity.x * self.pawn.velocity.x + self.pawn.velocity.y * self.pawn.velocity.y;
        let fast = speed2 > e.edge_check_max_speed * e.edge_check_max_speed;
        let hint_vertical = matches!(self.pawn.move_action_hint, MoveActionHint::Up | MoveActionHint::Down);
        let open12 = open(&t1) as i32 + open(&t2) as i32;
        let mut checked = false;
        if open12 == 2 {
            if fast || hint_vertical {
                return (delta, edge_spot);
            }
            checked = true;
        }
        let t3 = probe(self, loc + diag_l * side);
        if !checked && (fast || hint_vertical) {
            return (delta, edge_spot);
        }
        let feet = Vec3::new(loc.x, loc.y, loc.z - h);
        let mut l3 = none(&t3);
        let r2 = none(&t2);
        let a1 = none(&t1);
        if !(l3 || r2 || a1) {
            return (delta, edge_spot);
        }
        let reach = r - 4.0;
        let ecd2 = ecd * ecd;
        // a horizontal trace back towards the pawn at foot level, for the edge's face
        let face = |s: &Sim, d: Vec3| {
            let at = Vec3::new(feet.x, feet.y, feet.z - 8.0);
            s.world.line_check(at - d * reach, at + d * reach, Vec3::ZERO)
        };
        let is_face = |c: &crate::collision::CheckResult| c.hit && c.time < 1.0 && c.time > 0.0 && wfz > c.normal.z;
        let mut d = delta;
        if !(r2 && l3) {
            let td = if a1 {
                dir
            } else if l3 {
                diag_l
            } else if r2 {
                diag_r
            } else {
                Vec3::ZERO
            };
            let c = face(self, td);
            if !is_face(&c) {
                return (d, edge_spot);
            }
            let n = normal_2d(c.normal);
            let t = (c.location - loc).dot(n) / n.dot(n);
            if ecd2 >= (t * n.x) * (t * n.x) + (t * n.y) * (t * n.y) - len2 {
                d -= n * n.dot(d);
            }
            edge_spot = c.location + Vec3::new(0.0, 0.0, 8.0);
            return (d, edge_spot);
        }
        // both diagonals open: the faces on each side
        let ca = face(self, diag_l);
        if is_face(&ca) {
            edge_spot = ca.location + Vec3::new(0.0, 0.0, 8.0);
        } else {
            l3 = false;
        }
        let cb = face(self, diag_r);
        let hb = is_face(&cb);
        if hb {
            edge_spot = cb.location + Vec3::new(0.0, 0.0, 8.0);
        }
        let na = normal_2d(ca.normal);
        let nb = normal_2d(cb.normal);
        let slide_a = d - ca.normal * d.dot(ca.normal);
        let slide_b = d - cb.normal * d.dot(cb.normal);
        let dist = |c: &crate::collision::CheckResult, n: Vec3, s: Vec3| {
            let t = (c.location - loc).dot(n) / n.dot(n);
            (t * n.x - s.x) * (t * n.x - s.x) + (t * n.y - s.y) * (t * n.y - s.y)
        };
        let va = if l3 { dist(&ca, na, slide_a) } else { 0.0 };
        let vb = if hb { dist(&cb, nb, slide_b) } else { 0.0 };
        let (pick, keep) = 'pick: {
            if ecd2 >= vb {
                if ecd2 < va {
                    break 'pick (None, false);
                }
                if l3 && hb {
                    return (Vec3::ZERO, edge_spot);
                }
            }
            if ecd2 >= va && l3 {
                break 'pick (Some(slide_a), ecd2 < vb);
            }
            (None, false)
        };
        let (s, keep) = match pick {
            Some(s) => (s, keep),
            None => {
                if ecd2 < vb || !hb {
                    return (d, edge_spot);
                }
                (slide_b, ecd2 < va)
            }
        };
        if !keep {
            return (Vec3::ZERO, edge_spot);
        }
        d = s;
        (d, edge_spot)
    }

    /// TdMove_Vertigo.CanDoMove.
    pub fn vertigo_can_do_move(&mut self, _m: Move) -> bool {
        // (no super.CanDoMove; the weapon check doesn't apply unarmed)
        let p = &self.pawn;
        let facing = p.rotation.vector();
        if (self.moves.vertigo.last_vertigo_edge_position - p.location).safe_normal().dot(facing) < 0.0 {
            return false;
        }
        let mut start = p.location;
        start.z -= p.collision_height * 0.2;
        let mut end = start + facing.safe_normal() * p.collision_radius * 2.0;
        end.z = start.z;
        let extent = Vec3::new(p.collision_radius, p.collision_radius, p.collision_height * 0.7);
        !self.movement_trace_for_blocking(end, start, extent)
    }

    /// TdMove_Vertigo.StartMove.
    pub fn vertigo_start_move(&mut self, m: Move) {
        self.end_zoom();
        self.physics_move_start_move(m);
        let mut target = self.pawn.rotation;
        target.pitch = -15000;
        self.set_look_at_target_angle(m, target, 0.28, -1.0);
        let v = self.moves.vertigo.clone();
        self.set_move_countdown(m, v.zoom_out_time);
        self.play_move_anim(m, Slot::FullBodyDir, "edgedetection", 1.0, 0.28, 0.28, false, false);
        self.start_zoom(v.zoom_fov, v.zoom_rate, 0.0);
    }

    /// TdMove_Vertigo.UpdateViewRotation: looking away from the edge ends it.
    pub fn vertigo_after_view_rotation(&mut self, _m: Move) {
        let to_edge = (self.moves.vertigo.last_vertigo_edge_position - self.pawn.location).safe_normal();
        let edge_heading = Rotator::from_vector(to_edge);
        let d = (edge_heading - self.pawn.rotation).normalize();
        if d.yaw < -8000 || d.yaw > 8000 {
            self.set_move(Move::Walking, false, false);
        }
    }

    /// TdMove_Vertigo.OnTimer.
    pub fn vertigo_on_timer(&mut self, m: Move) {
        self.unzoom();
        self.abort_look_at_target(m);
    }

    /// TdMove_Vertigo.StopMove.
    pub fn vertigo_stop_move(&mut self, m: Move) {
        self.physics_move_stop_move(m);
        self.anim.stop(Slot::FullBodyDir, 0.4);
        self.unzoom();
    }

    /// TdPlayerController.StartZoom.
    pub fn start_zoom(&mut self, fov: f32, rate: f32, delay: f32) {
        self.pc.fov_zoom_rate = rate;
        self.pc.desired_fov = fov;
        self.pc.fov_zoom_delay = delay;
    }

    /// TdPlayerController.EndZoom.
    pub fn end_zoom(&mut self) {
        self.pc.desired_fov = self.pc.default_fov;
        self.pc.fov = self.pc.default_fov;
        self.pc.fov_zoom_rate = 0.0;
    }

    /// TdPlayerController.UnZoom (no weapon zoom here).
    pub fn unzoom(&mut self) {
        self.pc.desired_fov = self.pc.default_fov;
        self.pc.fov_zoom_rate = 20.0;
    }

    /// TdPlayerController.AdjustFOV.
    pub(crate) fn adjust_fov(&mut self, dt: f32) {
        let pc = &mut self.pc;
        pc.fov_zoom_delay -= dt;
        if pc.fov != pc.desired_fov && pc.fov_zoom_delay <= 0.0 {
            pc.fov_zoom_delay = 0.0;
            if pc.fov_zoom_rate > 0.0 {
                let d = pc.fov_zoom_rate * dt;
                pc.fov = if pc.fov > pc.desired_fov { pc.desired_fov.max(pc.fov - d) } else { pc.desired_fov.min(pc.fov + d) };
            } else {
                pc.fov = pc.desired_fov;
            }
        }
    }
}
