//! A neighbour-lookup grid: soldiers sorted by the square they stand in, so "who is near me" only
//! looks at a few squares. Rebuilt every tick; the sorted order makes every query visit neighbours
//! in the same order on every run.

pub struct SpatialGrid {
    cell: f32,
    /// (square key, soldier index), sorted.
    entries: Vec<(u64, u32)>,
}

fn key(cx: i32, cy: i32) -> u64 {
    ((cx as u32 as u64) << 32) | cy as u32 as u64
}

impl SpatialGrid {
    pub fn new(cell: f32) -> Self {
        SpatialGrid { cell, entries: Vec::new() }
    }

    fn square(&self, x: f32, y: f32) -> (i32, i32) {
        ((x / self.cell).floor() as i32, (y / self.cell).floor() as i32)
    }

    pub fn rebuild(&mut self, positions: impl Iterator<Item = (f32, f32)>) {
        self.entries.clear();
        for (i, (x, y)) in positions.enumerate() {
            let (cx, cy) = self.square(x, y);
            self.entries.push((key(cx, cy), i as u32));
        }
        self.entries.sort_unstable();
    }

    /// Calls `f` with the index of every soldier whose square lies within `radius` of (x, y).
    /// The caller still checks the exact distance.
    pub fn for_each_near(&self, x: f32, y: f32, radius: f32, mut f: impl FnMut(u32)) {
        let (x0, y0) = self.square(x - radius, y - radius);
        let (x1, y1) = self.square(x + radius, y + radius);
        for cx in x0..=x1 {
            for cy in y0..=y1 {
                let k = key(cx, cy);
                let start = self.entries.partition_point(|e| e.0 < k);
                for e in &self.entries[start..] {
                    if e.0 != k {
                        break;
                    }
                    f(e.1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_neighbours() {
        let mut g = SpatialGrid::new(4.0);
        let pts = [(0.0, 0.0), (1.0, 1.0), (10.0, 10.0), (-3.0, 0.5)];
        g.rebuild(pts.iter().copied());
        let mut found = Vec::new();
        g.for_each_near(0.0, 0.0, 3.5, |i| found.push(i));
        found.sort();
        assert_eq!(found, vec![0, 1, 3]);
    }
}
