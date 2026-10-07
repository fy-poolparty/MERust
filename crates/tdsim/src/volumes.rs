//! TdMovementVolume's spline (Start, Middle, End and the SplineLocations sampled from them) and
//! the volumes built on it: TdSwingVolume (a bar to swing on) and TdZiplineVolume (a cable to
//! slide down).

use crate::math::{Rotator, UeVec, Vec3};

/// A movement volume of the level, by kind and index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeRef {
    Ladder(usize),
    Swing(usize),
    Zipline(usize),
    Balance(usize),
}

/// The TdMovementVolume part: an oriented box brush plus the movement spline.
#[derive(Clone, Debug)]
pub struct SplineVolume {
    pub location: Vec3,
    pub rotation: Rotator,
    /// The brush as a box in the volume's local frame (DrawScale applied).
    pub box_min: Vec3,
    pub box_max: Vec3,
    pub move_direction: Vec3,
    pub start: Vec3,
    pub middle: Vec3,
    pub end: Vec3,
    pub num_spline_segments: i32,
    pub spline_locations: Vec<Vec3>,
}

impl SplineVolume {
    /// A box volume with its spline through `start`, `middle` and `end` (NumSplineSegments 10, as
    /// the volumes in the maps are saved).
    pub fn new(location: Vec3, rotation: Rotator, box_min: Vec3, box_max: Vec3, move_direction: Vec3, start: Vec3, middle: Vec3, end: Vec3) -> Self {
        let mut v = SplineVolume { location, rotation, box_min, box_max, move_direction, start, middle, end, num_spline_segments: 10, spline_locations: Vec::new() };
        // the saved SplineLocations are the curve at even parameter steps
        v.spline_locations = (0..=v.num_spline_segments).map(|i| v.location_on_spline(i as f32 / v.num_spline_segments as f32)).collect();
        v
    }

    /// GetLocationOnSpline (0x12A9490): the quadratic Bezier through Start, Middle and End.
    pub fn location_on_spline(&self, t: f32) -> Vec3 {
        let t = t.clamp(0.0, 1.0);
        let u = 1.0 - t;
        self.start * (u * u) + self.middle * (2.0 * u * t) + self.end * (t * t)
    }

    /// GetSlopeOnSpline (0x12A9580): the curve's direction.
    pub fn slope_on_spline(&self, t: f32) -> Vec3 {
        let t = t.clamp(0.0, 1.0);
        ((self.middle - self.start) * (1.0 - t) + (self.end - self.middle) * t).safe_normal()
    }

    /// FindClosestPointOnDSpline (0x12A9740): the nearest SplineLocation (searched outwards from
    /// `hint`, or every one below NumSplineSegments without a hint), then the projection on the
    /// segment towards the nearer neighbour. The parameter is in segments (0..count-1).
    pub fn find_closest_point_on_dspline(&self, p: Vec3, hint: i32) -> (Vec3, f32) {
        let pts = &self.spline_locations;
        let n = pts.len() as i32;
        let dist = |i: i32| (pts[i as usize] - p).length_squared();
        let segs = self.num_spline_segments.min(n);
        let mut best = f32::MAX;
        let mut bi = 0;
        if hint < 0 {
            for i in 0..segs {
                if dist(i) < best {
                    best = dist(i);
                    bi = i;
                }
            }
        } else {
            let mut i = hint;
            while i < segs {
                let d = dist(i);
                if best <= d {
                    break;
                }
                best = d;
                bi = i;
                i += 1;
            }
            let mut i = hint - 1;
            while i >= 0 {
                let d = dist(i);
                if best <= d {
                    break;
                }
                best = d;
                bi = i;
                i -= 1;
            }
        }
        if n == 0 {
            return (Vec3::ZERO, -1.0);
        }
        if n < 2 {
            return (pts[0], 0.0);
        }
        if bi == 0 {
            bi = 1;
        }
        if bi != n - 1 && dist(bi - 1) > dist(bi + 1) {
            bi += 1;
        }
        let a = pts[bi as usize - 1];
        let d = pts[bi as usize] - a;
        let t = (p - a).dot(d) / d.length_squared();
        let closest = a + d * t;
        // |t|, clamped to the segment
        let frac = ((d * t).length_squared() / d.length_squared()).sqrt().clamp(0.0, 1.0);
        (closest, (bi - 1) as f32 + frac)
    }

    /// AVolume::Encompasses for the box brush.
    pub fn encompasses(&self, p: Vec3) -> bool {
        let (x, y, z) = self.rotation.axes();
        let d = p - self.location;
        let l = Vec3::new(d.dot(x), d.dot(y), d.dot(z));
        l.x >= self.box_min.x && l.x <= self.box_max.x && l.y >= self.box_min.y && l.y <= self.box_max.y && l.z >= self.box_min.z && l.z <= self.box_max.z
    }

    /// Where a line from `start` to `end` first enters the box, as a fraction of the line
    /// (a zero-extent trace against the volume's brush).
    pub fn line_entry(&self, start: Vec3, end: Vec3) -> Option<f32> {
        let (x, y, z) = self.rotation.axes();
        let local = |v: Vec3| {
            let d = v - self.location;
            [d.dot(x), d.dot(y), d.dot(z)]
        };
        let (s, e) = (local(start), local(end));
        let (mn, mx) = ([self.box_min.x, self.box_min.y, self.box_min.z], [self.box_max.x, self.box_max.y, self.box_max.z]);
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for k in 0..3 {
            let dv = e[k] - s[k];
            if dv.abs() < 1e-6 {
                if s[k] < mn[k] || s[k] > mx[k] {
                    return None;
                }
            } else {
                let (mut a, mut b) = ((mn[k] - s[k]) / dv, (mx[k] - s[k]) / dv);
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                t0 = t0.max(a);
                t1 = t1.min(b);
                if t0 > t1 {
                    return None;
                }
            }
        }
        Some(t0)
    }
}

/// TdSwingVolume: the grip is the actor location, the bar runs along the volume's Y axis.
#[derive(Clone, Debug)]
pub struct SwingVolume {
    pub vol: SplineVolume,
    pub snap_to_center: bool,
    pub thick_grip: bool,
}

impl SwingVolume {
    /// The Tutorial_p swing volume brush (192 x 192, 92 below to 128 above the grip), turned to
    /// `yaw` (X is the swing direction, Y the bar).
    pub fn new(grip: Vec3, yaw: i32) -> Self {
        let rotation = Rotator::new(0, yaw, 0);
        let (box_min, box_max) = (Vec3::new(-96.0, -96.0, -92.0), Vec3::new(96.0, 96.0, 128.0));
        let (_, _, z) = rotation.axes();
        // MoveDirection / Start / End along the box's Z, through its centre
        let center = grip + z * ((box_min.z + box_max.z) * 0.5);
        let half = (box_max.z - box_min.z) * 0.5;
        let vol = SplineVolume::new(grip, rotation, box_min, box_max, z, center - z * half, center, center + z * half);
        SwingVolume { vol, snap_to_center: false, thick_grip: true }
    }
}

/// TdBalanceWalkVolume: a box round a beam, the spline along its middle.
pub fn balance_volume(start: Vec3, end: Vec3, half_width: f32, height: f32) -> SplineVolume {
    let along = end - start;
    let rotation = Rotator::from_vector(along);
    let len = along.length();
    SplineVolume::new(
        start,
        rotation,
        Vec3::new(0.0, -half_width, 0.0),
        Vec3::new(len, half_width, height),
        along.safe_normal_2d(),
        start,
        (start + end) * 0.5,
        end,
    )
}

/// TdZiplineVolume: the cable is the spline, MoveDirection is the way down it.
#[derive(Clone, Debug)]
pub struct ZiplineVolume {
    pub vol: SplineVolume,
    pub landing_strip: f32,
}

impl ZiplineVolume {
    /// A cable from `start` to `end` sagging through `middle`; the brush is a box along it,
    /// `half_width` to each side and from `below` under the cable to `above` over it.
    pub fn new(start: Vec3, middle: Vec3, end: Vec3, half_width: f32, below: f32, above: f32) -> Self {
        let along = end - start;
        let rotation = Rotator::from_vector(along);
        let len = along.length();
        let location = start;
        let vol = SplineVolume::new(
            location,
            rotation,
            Vec3::new(0.0, -half_width, -below),
            Vec3::new(len, half_width, above),
            along.safe_normal_2d(),
            start,
            middle,
            end,
        );
        ZiplineVolume { vol, landing_strip: 500.0 }
    }
}
