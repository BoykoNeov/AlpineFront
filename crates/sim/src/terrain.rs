//! The ground: a grid of heights, read as a surface of flat triangles.
//!
//! Coordinates are metres from the map centre (the Col di Lana summit): x east, y north, z up.
//! Grid point (c, r) sits at x = origin_x + c * spacing, y = origin_y + r * spacing.
//!
//! Each grid cell is split into two triangles along the diagonal from its south-west corner to its
//! north-east corner. The renderer draws exactly these triangles, and every height, slope and line
//! of sight question in the simulation uses them too, so what you see is what the soldiers stand on.

use serde::Deserialize;
use std::path::Path;

/// The eight grid neighbours, counter-clockwise from east.
pub const NEIGHBOURS: [(i32, i32); 8] =
    [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

#[derive(Debug, Deserialize)]
struct TerrainMeta {
    name: String,
    heights_file: String,
    columns: usize,
    rows: usize,
    spacing_m: f32,
}

pub struct Terrain {
    pub name: String,
    pub cols: usize,
    pub rows: usize,
    pub spacing: f32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub heights: Vec<f32>,
    pub steepest_walkable_deg: f32,
    /// Per cell: bit 0 = lower triangle walkable, bit 1 = upper triangle walkable.
    tri_walkable: Vec<u8>,
    /// Per grid point: bit k = the edge to NEIGHBOURS[k] can be walked along.
    edge_pass: Vec<u8>,
    /// Per grid point: which walkable region it belongs to (u32::MAX = none).
    component: Vec<u32>,
    /// The largest walkable region.
    pub main_component: u32,
}

/// A triangle of the ground surface: cell column, cell row, and whether it is the upper one
/// (north-west of the cell diagonal) or the lower one (south-east of it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tri {
    pub c: usize,
    pub r: usize,
    pub upper: bool,
}

impl Terrain {
    /// Loads a terrain described by `<dir>/<name>.toml`.
    pub fn load(dir: &Path, name: &str, steepest_walkable_deg: f32) -> Result<Terrain, String> {
        let meta_path = dir.join(format!("{name}.toml"));
        let text = std::fs::read_to_string(&meta_path)
            .map_err(|e| format!("{}: {e}", meta_path.display()))?;
        let meta: TerrainMeta =
            toml::from_str(&text).map_err(|e| format!("{}: {e}", meta_path.display()))?;
        let bytes = std::fs::read(dir.join(&meta.heights_file))
            .map_err(|e| format!("{}: {e}", meta.heights_file))?;
        if bytes.len() != meta.columns * meta.rows * 4 {
            return Err(format!("{}: wrong size", meta.heights_file));
        }
        let heights = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let mut t = Terrain::from_heights(meta.columns, meta.rows, meta.spacing_m, heights, steepest_walkable_deg);
        t.name = meta.name;
        Ok(t)
    }

    /// The game map, `data/terrain/col_di_lana`.
    pub fn load_col_di_lana(steepest_walkable_deg: f32) -> Result<Terrain, String> {
        Self::load(&crate::data_dir().join("terrain"), "col_di_lana", steepest_walkable_deg)
    }

    /// Builds a terrain from raw heights (rows south to north, each row west to east), centred on
    /// the origin.
    pub fn from_heights(
        cols: usize,
        rows: usize,
        spacing: f32,
        heights: Vec<f32>,
        steepest_walkable_deg: f32,
    ) -> Terrain {
        assert!(cols >= 2 && rows >= 2 && heights.len() == cols * rows);
        let mut t = Terrain {
            name: String::new(),
            cols,
            rows,
            spacing,
            origin_x: -((cols - 1) as f32) * spacing / 2.0,
            origin_y: -((rows - 1) as f32) * spacing / 2.0,
            heights,
            steepest_walkable_deg,
            tri_walkable: Vec::new(),
            edge_pass: Vec::new(),
            component: Vec::new(),
            main_component: u32::MAX,
        };
        t.build_walkability();
        t
    }

    // ---- basic geometry -------------------------------------------------------------------

    #[inline]
    pub fn idx(&self, c: usize, r: usize) -> usize {
        r * self.cols + c
    }

    #[inline]
    pub fn h(&self, c: usize, r: usize) -> f32 {
        self.heights[r * self.cols + c]
    }

    pub fn point_xy(&self, c: usize, r: usize) -> (f32, f32) {
        (self.origin_x + c as f32 * self.spacing, self.origin_y + r as f32 * self.spacing)
    }

    pub fn min_x(&self) -> f32 {
        self.origin_x
    }
    pub fn max_x(&self) -> f32 {
        self.origin_x + (self.cols - 1) as f32 * self.spacing
    }
    pub fn min_y(&self) -> f32 {
        self.origin_y
    }
    pub fn max_y(&self) -> f32 {
        self.origin_y + (self.rows - 1) as f32 * self.spacing
    }

    pub fn in_bounds(&self, x: f32, y: f32) -> bool {
        x >= self.min_x() && x <= self.max_x() && y >= self.min_y() && y <= self.max_y()
    }

    /// Nearest grid point, clamped to the map.
    pub fn nearest_point(&self, x: f32, y: f32) -> (usize, usize) {
        let c = ((x - self.origin_x) / self.spacing).round().clamp(0.0, (self.cols - 1) as f32);
        let r = ((y - self.origin_y) / self.spacing).round().clamp(0.0, (self.rows - 1) as f32);
        (c as usize, r as usize)
    }

    /// Cell and position inside it (0..1 each way), clamped to the map.
    #[inline]
    fn locate(&self, x: f32, y: f32) -> (usize, usize, f32, f32) {
        let u = ((x - self.origin_x) / self.spacing).clamp(0.0, (self.cols - 1) as f32);
        let v = ((y - self.origin_y) / self.spacing).clamp(0.0, (self.rows - 1) as f32);
        let c = (u.floor() as usize).min(self.cols - 2);
        let r = (v.floor() as usize).min(self.rows - 2);
        (c, r, u - c as f32, v - r as f32)
    }

    /// Ground height at a point, on the triangle surface.
    #[inline]
    pub fn height(&self, x: f32, y: f32) -> f32 {
        let (c, r, fx, fy) = self.locate(x, y);
        self.height_in_cell(c, r, fx, fy)
    }

    #[inline]
    fn height_in_cell(&self, c: usize, r: usize, fx: f32, fy: f32) -> f32 {
        let ha = self.h(c, r);
        let hd = self.h(c + 1, r + 1);
        if fx >= fy {
            let hb = self.h(c + 1, r);
            ha + (hb - ha) * fx + (hd - hb) * fy
        } else {
            let hc = self.h(c, r + 1);
            ha + (hc - ha) * fy + (hd - hc) * fx
        }
    }

    pub fn tri_at(&self, x: f32, y: f32) -> Tri {
        let (c, r, fx, fy) = self.locate(x, y);
        Tri { c, r, upper: fy > fx }
    }

    /// Height gradient (rise per metre east, rise per metre north) of a triangle.
    pub fn tri_gradient(&self, t: Tri) -> (f32, f32) {
        let s = self.spacing;
        let ha = self.h(t.c, t.r);
        let hd = self.h(t.c + 1, t.r + 1);
        if t.upper {
            let hc = self.h(t.c, t.r + 1);
            ((hd - hc) / s, (hc - ha) / s)
        } else {
            let hb = self.h(t.c + 1, t.r);
            ((hb - ha) / s, (hd - hb) / s)
        }
    }

    pub fn tri_slope_deg(&self, t: Tri) -> f32 {
        let (gx, gy) = self.tri_gradient(t);
        (gx * gx + gy * gy).sqrt().atan().to_degrees()
    }

    /// Slope in degrees of the ground triangle at a point.
    pub fn slope_deg(&self, x: f32, y: f32) -> f32 {
        self.tri_slope_deg(self.tri_at(x, y))
    }

    #[inline]
    pub fn tri_walkable(&self, t: Tri) -> bool {
        let bits = self.tri_walkable[t.r * (self.cols - 1) + t.c];
        bits & if t.upper { 2 } else { 1 } != 0
    }

    /// Whether a man may stand at this point.
    #[inline]
    pub fn walkable_at(&self, x: f32, y: f32) -> bool {
        self.in_bounds(x, y) && self.tri_walkable(self.tri_at(x, y))
    }

    /// Whether every point on the straight line from a to b may be stood on. Exact: finds every
    /// place the line crosses a triangle edge and checks the triangle on each piece in between, so
    /// even a sliver of steep ground is caught.
    pub fn segment_walkable(&self, ax: f32, ay: f32, bx: f32, by: f32) -> bool {
        if !self.walkable_at(ax, ay) || !self.walkable_at(bx, by) {
            return false;
        }
        let s = self.spacing;
        let (u0, v0) = ((ax - self.origin_x) / s, (ay - self.origin_y) / s);
        let (du, dv) = ((bx - ax) / s, (by - ay) / s);
        let mut cuts: Vec<f32> = Vec::with_capacity(16);
        cuts.push(0.0);
        cuts.push(1.0);
        for (start, delta) in [(u0, du), (v0, dv), (u0 - v0, du - dv)] {
            if delta.abs() < 1e-9 {
                continue;
            }
            let (lo, hi) = if delta > 0.0 { (start, start + delta) } else { (start + delta, start) };
            let mut k = lo.floor() + 1.0;
            while k < hi {
                cuts.push((k - start) / delta);
                k += 1.0;
            }
        }
        cuts.sort_by(f32::total_cmp);
        cuts.windows(2).all(|w| {
            if w[1] - w[0] < 1e-7 {
                return true;
            }
            let m = 0.5 * (w[0] + w[1]);
            self.walkable_at(ax + (bx - ax) * m, ay + (by - ay) * m)
        })
    }

    // ---- walkability --------------------------------------------------------------------------

    #[inline]
    pub fn edge_passable(&self, c: usize, r: usize, k: usize) -> bool {
        self.edge_pass[self.idx(c, r)] & (1 << k) != 0
    }

    pub fn component_at_point(&self, c: usize, r: usize) -> u32 {
        self.component[self.idx(c, r)]
    }

    /// Whether a point lies on ground connected to the main walkable region.
    pub fn in_main_region(&self, x: f32, y: f32) -> bool {
        if !self.walkable_at(x, y) {
            return false;
        }
        let (c, r) = self.nearest_point(x, y);
        self.component_at_point(c, r) == self.main_component
    }

    /// The triangles a grid edge runs along or across. An edge can be walked only if all of them
    /// can be stood on.
    fn edge_tris(&self, c: usize, r: usize, k: usize) -> [Option<Tri>; 2] {
        let (cols, rows) = (self.cols as i64, self.rows as i64);
        let tri = |c: i64, r: i64, upper: bool| -> Option<Tri> {
            if c >= 0 && r >= 0 && c < cols - 1 && r < rows - 1 {
                Some(Tri { c: c as usize, r: r as usize, upper })
            } else {
                None
            }
        };
        let (c, r) = (c as i64, r as i64);
        // Express every direction from the edge's "first" end (E, NE, N, NW and their opposites).
        let (fc, fr, dir) = match k {
            0 => (c, r, 0),         // E
            1 => (c, r, 1),         // NE
            2 => (c, r, 2),         // N
            3 => (c, r, 3),         // NW
            4 => (c - 1, r, 0),     // W  = E from the west neighbour
            5 => (c - 1, r - 1, 1), // SW = NE from the south-west neighbour
            6 => (c, r - 1, 2),     // S  = N from the south neighbour
            _ => (c + 1, r - 1, 3), // SE = NW from the south-east neighbour
        };
        match dir {
            // Along the bottom of cell (fc, fr) and the top of cell (fc, fr - 1).
            0 => [tri(fc, fr, false), tri(fc, fr - 1, true)],
            // The cell diagonal itself.
            1 => [tri(fc, fr, false), tri(fc, fr, true)],
            // Along the west side of cell (fc, fr) and the east side of cell (fc - 1, fr).
            2 => [tri(fc, fr, true), tri(fc - 1, fr, false)],
            // Across cell (fc - 1, fr), against its diagonal: crosses both its triangles.
            _ => [tri(fc - 1, fr, false), tri(fc - 1, fr, true)],
        }
    }

    fn build_walkability(&mut self) {
        let (cols, rows) = (self.cols, self.rows);
        let max = self.steepest_walkable_deg;
        let mut tw = vec![0u8; (cols - 1) * (rows - 1)];
        for r in 0..rows - 1 {
            for c in 0..cols - 1 {
                let mut bits = 0;
                if self.tri_slope_deg(Tri { c, r, upper: false }) <= max {
                    bits |= 1;
                }
                if self.tri_slope_deg(Tri { c, r, upper: true }) <= max {
                    bits |= 2;
                }
                tw[r * (cols - 1) + c] = bits;
            }
        }
        self.tri_walkable = tw;

        let mut ep = vec![0u8; cols * rows];
        for r in 0..rows {
            for c in 0..cols {
                let mut bits = 0u8;
                for (k, (dc, dr)) in NEIGHBOURS.iter().enumerate() {
                    let (nc, nr) = (c as i64 + *dc as i64, r as i64 + *dr as i64);
                    if nc < 0 || nr < 0 || nc >= cols as i64 || nr >= rows as i64 {
                        continue;
                    }
                    let tris = self.edge_tris(c, r, k);
                    let any = tris.iter().any(|t| t.is_some());
                    let all = tris.iter().flatten().all(|t| self.tri_walkable(*t));
                    if any && all {
                        bits |= 1 << k;
                    }
                }
                ep[r * cols + c] = bits;
            }
        }
        self.edge_pass = ep;

        // Connected walkable regions, by flood fill in index order.
        let mut comp = vec![u32::MAX; cols * rows];
        let mut sizes: Vec<usize> = Vec::new();
        let mut stack = Vec::new();
        for start in 0..cols * rows {
            if comp[start] != u32::MAX || self.edge_pass[start] == 0 {
                continue;
            }
            let id = sizes.len() as u32;
            let mut size = 0;
            comp[start] = id;
            stack.push(start);
            while let Some(i) = stack.pop() {
                size += 1;
                let (c, r) = (i % cols, i / cols);
                for (k, (dc, dr)) in NEIGHBOURS.iter().enumerate() {
                    if self.edge_pass[i] & (1 << k) == 0 {
                        continue;
                    }
                    let j = (r as i64 + *dr as i64) as usize * cols + (c as i64 + *dc as i64) as usize;
                    if comp[j] == u32::MAX {
                        comp[j] = id;
                        stack.push(j);
                    }
                }
            }
            sizes.push(size);
        }
        self.main_component = sizes
            .iter()
            .enumerate()
            .max_by_key(|(i, s)| (**s, std::cmp::Reverse(*i)))
            .map(|(i, _)| i as u32)
            .unwrap_or(u32::MAX);
        self.component = comp;
    }

    /// Nearest grid point in the main walkable region whose surrounding ground can be stood on,
    /// searching outward up to `max_cells`.
    pub fn nearest_main_point(&self, x: f32, y: f32, max_cells: i32) -> Option<(usize, usize)> {
        let (c0, r0) = self.nearest_point(x, y);
        let mut best: Option<((usize, usize), f32)> = None;
        for ring in 0..=max_cells {
            for dr in -ring..=ring {
                for dc in -ring..=ring {
                    if dr.abs() != ring && dc.abs() != ring {
                        continue;
                    }
                    let (c, r) = (c0 as i32 + dc, r0 as i32 + dr);
                    if c < 0 || r < 0 || c >= self.cols as i32 || r >= self.rows as i32 {
                        continue;
                    }
                    let (c, r) = (c as usize, r as usize);
                    if self.component_at_point(c, r) != self.main_component {
                        continue;
                    }
                    let (px, py) = self.point_xy(c, r);
                    if !self.walkable_at(px, py) {
                        continue;
                    }
                    let d = (px - x).powi(2) + (py - y).powi(2);
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some(((c, r), d));
                    }
                }
            }
            if best.is_some() {
                return best.map(|(p, _)| p);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plane(cols: usize, rows: usize, gx: f32, gy: f32) -> Terrain {
        let s = 10.0;
        let mut h = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                h.push(1000.0 + gx * c as f32 * s + gy * r as f32 * s);
            }
        }
        Terrain::from_heights(cols, rows, s, h, 45.0)
    }

    #[test]
    fn plane_heights_are_exact() {
        let t = plane(11, 11, 0.3, -0.2);
        for &(x, y) in &[(0.0f32, 0.0f32), (12.3, -7.7), (-49.0, 49.0), (3.3, 3.2)] {
            let expect = 1000.0 + 0.3 * (x - t.origin_x) - 0.2 * (y - t.origin_y);
            assert!((t.height(x, y) - expect).abs() < 1e-3);
        }
        assert!((t.slope_deg(1.0, 2.0) - (0.3f32.hypot(0.2)).atan().to_degrees()).abs() < 1e-3);
    }

    #[test]
    fn steep_plane_is_not_walkable() {
        let t = plane(11, 11, 1.2, 0.0); // about 50 degrees
        assert!(!t.walkable_at(0.0, 0.0));
        assert_eq!(t.main_component, u32::MAX);
        let flat = plane(11, 11, 0.1, 0.1);
        assert!(flat.walkable_at(0.0, 0.0));
        for k in 0..8 {
            assert!(flat.edge_passable(5, 5, k));
        }
    }

    #[test]
    fn upper_and_lower_triangles_meet() {
        // A single tall corner makes the two triangles of a cell differ.
        let mut h = vec![0.0; 4];
        h[3] = 10.0; // north-east corner
        let t = Terrain::from_heights(2, 2, 10.0, h, 45.0);
        // On the diagonal both formulas agree.
        assert!((t.height(t.origin_x + 5.0, t.origin_y + 5.0) - 5.0).abs() < 1e-4);
        assert!((t.height(t.origin_x + 10.0, t.origin_y + 10.0) - 10.0).abs() < 1e-4);
    }
}
