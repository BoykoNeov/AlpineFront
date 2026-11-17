//! Flow fields: for one destination, the walking time from every grid point that matters (Dijkstra's
//! shortest-path method run backwards from the destination). Every man in a squad follows the same
//! field; the simulation works one out per squad, covering only the area around its men and their way. Walking time per edge uses the walking-speed rule in the tuning file, so
//! routes bend around steep ground on their own.

use crate::terrain::{NEIGHBOURS, Terrain};
use crate::tuning::WalkingTuning;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

pub struct FlowField {
    pub dest: (usize, usize),
    /// Seconds to walk from each grid point to the destination; infinite if it cannot be reached.
    pub time: Vec<f32>,
}

#[derive(PartialEq)]
struct Item(f32, u32);
impl Eq for Item {}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        // Smallest time first; ties broken by index so the order is fully fixed.
        o.0.total_cmp(&self.0).then_with(|| o.1.cmp(&self.1))
    }
}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// Seconds to walk the straight line between two points on the ground.
pub fn walk_time(t: &Terrain, w: &WalkingTuning, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let run = ((bx - ax).powi(2) + (by - ay).powi(2)).sqrt();
    if run < 1e-6 {
        return 0.0;
    }
    let grade = (t.height(bx, by) - t.height(ax, ay)) / run;
    run / w.speed_mps(grade)
}

/// How far beyond the farthest man a bounded route calculation keeps going: this fraction of his
/// walking time, plus a fixed margin. Men pushed off the route stay well inside it.
const BOUND_FACTOR: f32 = 1.3;
const BOUND_MARGIN_S: f32 = 180.0;

impl FlowField {
    /// Walking times from the whole map.
    pub fn compute(t: &Terrain, w: &WalkingTuning, dest: (usize, usize)) -> FlowField {
        Self::compute_bounded(t, w, dest, &[])
    }

    /// Walking times from the area that matters for men starting at `from` (grid points): the
    /// calculation stops once all of them are covered and it has gone a margin beyond. Places it
    /// did not reach are left infinite. With `from` empty, covers the whole map.
    pub fn compute_bounded(t: &Terrain, w: &WalkingTuning, dest: (usize, usize), from: &[(usize, usize)]) -> FlowField {
        let n = t.cols * t.rows;
        let mut needed: Vec<bool> = vec![false; if from.is_empty() { 0 } else { n }];
        let mut left = 0usize;
        for &(c, r) in from {
            let i = t.idx(c, r);
            if !needed[i] {
                needed[i] = true;
                left += 1;
            }
        }
        let mut limit = f32::INFINITY;
        let mut time = vec![f32::INFINITY; n];
        let mut done = vec![false; n];
        let mut heap = BinaryHeap::new();
        let d = t.idx(dest.0, dest.1);
        time[d] = 0.0;
        heap.push(Item(0.0, d as u32));
        let diag = t.spacing * std::f32::consts::SQRT_2;
        while let Some(Item(tv, vi)) = heap.pop() {
            let vi = vi as usize;
            if done[vi] {
                continue;
            }
            if tv > limit {
                break;
            }
            done[vi] = true;
            if left > 0 && needed[vi] {
                left -= 1;
                if left == 0 {
                    limit = tv * BOUND_FACTOR + BOUND_MARGIN_S;
                }
            }
            let (vc, vr) = (vi % t.cols, vi / t.cols);
            let hv = t.heights[vi];
            for (k, (dc, dr)) in NEIGHBOURS.iter().enumerate() {
                if !t.edge_passable(vc, vr, k) {
                    continue;
                }
                let ui = (vr as i64 + *dr as i64) as usize * t.cols + (vc as i64 + *dc as i64) as usize;
                if done[ui] {
                    continue;
                }
                // Walking from u to v.
                let run = if k % 2 == 1 { diag } else { t.spacing };
                let grade = (hv - t.heights[ui]) / run;
                let cand = tv + run / w.speed_mps(grade);
                if cand < time[ui] {
                    time[ui] = cand;
                    heap.push(Item(cand, ui as u32));
                }
            }
        }
        if limit.is_finite() {
            // Only finished values are exact; leave the rest unreached.
            for (tv, d) in time.iter_mut().zip(&done) {
                if !d {
                    *tv = f32::INFINITY;
                }
            }
        }
        FlowField { dest, time }
    }

    #[inline]
    pub fn at(&self, t: &Terrain, c: usize, r: usize) -> f32 {
        self.time[t.idx(c, r)]
    }

    /// Estimated seconds from a position to the destination (via the best nearby grid point).
    pub fn time_from(&self, t: &Terrain, w: &WalkingTuning, x: f32, y: f32) -> f32 {
        let (c0, r0) = t.nearest_point(x, y);
        let mut best = f32::INFINITY;
        for dr in -1i32..=1 {
            for dc in -1i32..=1 {
                let (c, r) = (c0 as i32 + dc, r0 as i32 + dr);
                if c < 0 || r < 0 || c >= t.cols as i32 || r >= t.rows as i32 {
                    continue;
                }
                let tv = self.at(t, c as usize, r as usize);
                if tv.is_finite() {
                    let (px, py) = t.point_xy(c as usize, r as usize);
                    best = best.min(tv + walk_time(t, w, x, y, px, py));
                }
            }
        }
        best
    }

    /// Where a man at (x, y) should walk next: the grid point within two cells that minimises
    /// (time to walk there in a straight line + its time to the destination), among those he can
    /// reach in a straight line without crossing ground too steep to stand on.
    pub fn next_waypoint(&self, t: &Terrain, w: &WalkingTuning, x: f32, y: f32) -> Option<(f32, f32)> {
        let (c0, r0) = t.nearest_point(x, y);
        let mut best: Option<((f32, f32), f32)> = None;
        for dr in -2i32..=2 {
            for dc in -2i32..=2 {
                let (c, r) = (c0 as i32 + dc, r0 as i32 + dr);
                if c < 0 || r < 0 || c >= t.cols as i32 || r >= t.rows as i32 {
                    continue;
                }
                let tv = self.at(t, c as usize, r as usize);
                if !tv.is_finite() {
                    continue;
                }
                let (px, py) = t.point_xy(c as usize, r as usize);
                let d = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
                // A point he is already standing on is no step at all (unless it is the end).
                if d < 1.0 && (c as usize, r as usize) != self.dest {
                    continue;
                }
                // Among near-equal choices prefer the farther point: straighter, smoother paths.
                let score = tv + walk_time(t, w, x, y, px, py) - 0.01 * d;
                if best.is_some_and(|(_, s)| score >= s) {
                    continue;
                }
                if t.segment_walkable(x, y, px, py) {
                    best = Some(((px, py), score));
                }
            }
        }
        best.map(|(p, _)| p)
    }

    /// The next grid point along the route from grid point (c, r): the walkable neighbour through
    /// which the walking time to the destination is shortest. Always strictly closer in time, so
    /// following it can never go round in circles. None at the destination or if cut off.
    pub fn parent(&self, t: &Terrain, w: &WalkingTuning, c: usize, r: usize) -> Option<(usize, usize)> {
        let here = self.at(t, c, r);
        if !here.is_finite() || (c, r) == self.dest {
            return None;
        }
        let diag = t.spacing * std::f32::consts::SQRT_2;
        let mut best: Option<((usize, usize), f32)> = None;
        for (k, (dc, dr)) in NEIGHBOURS.iter().enumerate() {
            if !t.edge_passable(c, r, k) {
                continue;
            }
            let (nc, nr) = ((c as i32 + dc) as usize, (r as i32 + dr) as usize);
            let tn = self.at(t, nc, nr);
            if tn >= here {
                continue;
            }
            let run = if k % 2 == 1 { diag } else { t.spacing };
            let total = tn + run / w.speed_mps((t.h(nc, nr) - t.h(c, r)) / run);
            if best.is_none_or(|(_, b)| total < b) {
                best = Some(((nc, nr), total));
            }
        }
        best.map(|(p, _)| p)
    }

    /// For a man at (x, y) who has reached route point `at`: the next point to head for. Looks up
    /// to four points ahead along the route and takes the farthest he can walk to in a straight line
    /// over ground he can stand on (cutting corners). None if `at` is the destination.
    pub fn advance(&self, t: &Terrain, w: &WalkingTuning, x: f32, y: f32, at: (f32, f32)) -> Option<(f32, f32)> {
        let mut chain = Vec::with_capacity(4);
        let mut g = t.nearest_point(at.0, at.1);
        for _ in 0..4 {
            match self.parent(t, w, g.0, g.1) {
                Some(p) => {
                    chain.push(p);
                    g = p;
                }
                None => break,
            }
        }
        for &(c, r) in chain.iter().rev() {
            let (px, py) = t.point_xy(c, r);
            if t.segment_walkable(x, y, px, py) {
                return Some((px, py));
            }
        }
        chain.first().map(|&(c, r)| t.point_xy(c, r))
    }

    /// The route a man at (x, y) would take, as a list of points (for drawing).
    pub fn route_from(&self, t: &Terrain, w: &WalkingTuning, x: f32, y: f32, max_points: usize) -> Vec<(f32, f32)> {
        let mut pts = vec![(x, y)];
        let Some(mut p) = self.next_waypoint(t, w, x, y) else { return pts };
        pts.push(p);
        for _ in 0..max_points {
            match self.advance(t, w, p.0, p.1, p) {
                Some(q) if q != p => {
                    pts.push(q);
                    p = q;
                }
                _ => break,
            }
        }
        pts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::WalkingTuning;

    fn walking() -> WalkingTuning {
        WalkingTuning {
            tobler_peak_kmh: 6.0,
            tobler_steepness: 3.5,
            tobler_offset: 0.05,
            load_factor: 1.0,
            steepest_walkable_deg: 45.0,
        }
    }

    #[test]
    fn flat_ground_time_matches_speed() {
        let t = Terrain::from_heights(21, 21, 10.0, vec![0.0; 441], 45.0);
        let w = walking();
        let f = FlowField::compute(&t, &w, (10, 10));
        let expect = 100.0 / w.speed_mps(0.0);
        assert!((f.at(&t, 20, 10) - expect).abs() < 0.01);
    }

    #[test]
    fn goes_around_a_wall() {
        // A cliff wall across the map with a gap at the north end.
        let (cols, rows) = (21, 21);
        let mut h = vec![0.0; cols * rows];
        for r in 0..rows - 3 {
            h[r * cols + 10] = 50.0;
        }
        let t = Terrain::from_heights(cols, rows, 10.0, h, 45.0);
        let w = walking();
        let f = FlowField::compute(&t, &w, (15, 2));
        let tv = f.at(&t, 5, 2);
        assert!(tv.is_finite());
        // Must be much longer than the 100 m straight line.
        assert!(tv > 300.0 / w.speed_mps(0.0), "{tv}");
    }
}
