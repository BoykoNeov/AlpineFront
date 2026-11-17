//! Line of sight over the triangle ground surface.
//!
//! Between two places where the sight line crosses a triangle edge, both the ground and the sight
//! line are straight, so the closest the ground comes to the line is always at one of those
//! crossings. Checking every crossing is therefore exact against the drawn terrain, with no
//! sampling step to miss a thin ridge. Earth curvature (less the usual bending of light by air) is
//! added as a bulge in the ground between the two ends.

use crate::terrain::Terrain;
use crate::tuning::LosTuning;
use rayon::prelude::*;

#[derive(Clone, Copy, Debug)]
pub struct LosParams {
    /// Ground bulge in metres = coef * d1 * d2, where d1, d2 are the distances to the two ends.
    pub bulge_coef: f32,
}

impl LosParams {
    pub fn from_tuning(t: &LosTuning) -> Self {
        LosParams { bulge_coef: (1.0 - t.refraction_coefficient) / (2.0 * t.earth_radius_m) }
    }
    pub fn flat_earth() -> Self {
        LosParams { bulge_coef: 0.0 }
    }
}

/// How far the sight line from `a` to `b` (each x, y, z) passes above the ground at its lowest
/// point, in metres. Negative means the ground blocks it. With `stop_when_blocked`, returns as soon
/// as any blocking point is found.
pub fn clearance(t: &Terrain, p: &LosParams, a: [f32; 3], b: [f32; 3], stop_when_blocked: bool) -> f32 {
    let s = t.spacing;
    let (u0, v0) = ((a[0] - t.origin_x) / s, (a[1] - t.origin_y) / s);
    let (u1, v1) = ((b[0] - t.origin_x) / s, (b[1] - t.origin_y) / s);
    let (du, dv) = (u1 - u0, v1 - v0);
    let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    let mut worst = f32::INFINITY;

    let mut check = |tt: f32| -> bool {
        if tt <= 0.0 || tt >= 1.0 {
            return false;
        }
        let x = a[0] + (b[0] - a[0]) * tt;
        let y = a[1] + (b[1] - a[1]) * tt;
        let line = a[2] + (b[2] - a[2]) * tt;
        let bulge = p.bulge_coef * (len * tt) * (len * (1.0 - tt));
        let gap = line - t.height(x, y) - bulge;
        if gap < worst {
            worst = gap;
        }
        stop_when_blocked && gap < 0.0
    };

    // Crossings of the three families of triangle edges: columns (u = k), rows (v = k) and cell
    // diagonals (u - v = k).
    for (start, delta) in [(u0, du), (v0, dv), (u0 - v0, du - dv)] {
        if delta.abs() < 1e-9 {
            continue;
        }
        let (lo, hi) = if delta > 0.0 { (start, start + delta) } else { (start + delta, start) };
        let mut k = lo.floor() + 1.0;
        while k < hi {
            if check((k - start) / delta) {
                return worst;
            }
            k += 1.0;
        }
    }
    worst
}

/// Whether a point at `b` can be seen from an eye at `a`.
pub fn visible(t: &Terrain, p: &LosParams, a: [f32; 3], b: [f32; 3]) -> bool {
    clearance(t, p, a, b, true) > 0.0
}

/// For every grid point within `radius` of the observer's eye: 1 if a point `target_height` above
/// the ground there can be seen, else 0. Rows south to north, as the terrain. Splits rows across
/// the current thread pool.
pub fn viewshed(t: &Terrain, p: &LosParams, eye: [f32; 3], radius: f32, target_height: f32) -> Vec<u8> {
    let r2 = radius * radius;
    let mut out = vec![0u8; t.cols * t.rows];
    out.par_chunks_mut(t.cols).enumerate().for_each(|(r, row)| {
        for (c, cell) in row.iter_mut().enumerate() {
            let (x, y) = t.point_xy(c, r);
            let d2 = (x - eye[0]).powi(2) + (y - eye[1]).powi(2);
            if d2 > r2 {
                continue;
            }
            let target = [x, y, t.h(c, r) + target_height];
            *cell = visible(t, p, eye, target) as u8;
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flat ground with a wall one grid line wide.
    fn wall(height: f32) -> Terrain {
        let (cols, rows) = (21, 21);
        let mut h = vec![100.0; cols * rows];
        for r in 0..rows {
            h[r * cols + 10] = 100.0 + height;
        }
        Terrain::from_heights(cols, rows, 10.0, h, 89.0)
    }

    #[test]
    fn a_thin_ridge_blocks() {
        let t = wall(5.0);
        let p = LosParams::flat_earth();
        // Eyes 1.6 m up on both sides of a 5 m ridge, 80 m apart: blocked.
        assert!(!visible(&t, &p, [-40.0, 3.0, 101.6], [40.0, -7.0, 101.6]));
        // Raise the eyes above the ridge: visible.
        assert!(visible(&t, &p, [-40.0, 3.0, 106.0], [40.0, -7.0, 106.0]));
        // Same side of the ridge: visible.
        assert!(visible(&t, &p, [-90.0, 3.0, 101.6], [-10.0, -7.0, 101.6]));
    }

    #[test]
    fn clearance_is_exact_on_a_slope_line() {
        let t = wall(0.0);
        let p = LosParams::flat_earth();
        let c = clearance(&t, &p, [-50.0, 0.0, 102.0], [50.0, 0.0, 104.0], false);
        // Ground is flat at 100. The ends are not checked (a man's own feet don't block his view),
        // so the lowest checked point is the first grid line crossed, 10 m in, where the line is
        // 2.2 m up.
        assert!((c - 2.2).abs() < 1e-3, "{c}");
    }

    #[test]
    fn symmetric() {
        let t = wall(1.0);
        let p = LosParams::flat_earth();
        for (a, b) in [([-60.0, 5.0, 101.6], [50.0, -3.0, 101.6]), ([-30.0, 0.0, 101.0], [30.0, 0.0, 101.2])] {
            assert_eq!(visible(&t, &p, a, b), visible(&t, &p, b, a));
        }
    }
}
