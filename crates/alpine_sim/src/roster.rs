//! The work roster (decisions 39 to 43): duties, the lookout posts and shelters they are done at,
//! and how tired a man is. The places are read from `data/places/col_di_lana_1916.toml`.

use crate::terrain::Terrain;
use crate::tuning::TirednessTuning;
use serde::{Deserialize, Serialize};

/// What a squad does during one half of the day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Duty {
    /// Stand at a lookout post and watch.
    Guard,
    /// Sleep in a shelter, out of sight both ways.
    Rest,
}

impl Duty {
    pub fn word(self) -> &'static str {
        match self {
            Duty::Guard => "guard",
            Duty::Rest => "rest",
        }
    }

    /// The kind of place the duty is done at.
    pub fn place_kind(self) -> PlaceKind {
        match self {
            Duty::Guard => PlaceKind::Post,
            Duty::Rest => PlaceKind::Shelter,
        }
    }
}

/// A squad's duties: one for the day (dawn to dusk) and one for the night.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roster {
    pub day: Duty,
    pub night: Duty,
}

impl Roster {
    pub fn duty(self, night: bool) -> Duty {
        if night { self.night } else { self.day }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaceKind {
    Post,
    Shelter,
}

/// A lookout post or a shelter.
#[derive(Clone, Debug)]
pub struct Place {
    pub name: String,
    pub kind: PlaceKind,
    pub x: f32,
    pub y: f32,
    /// Where each squad the place holds stands, one spot per squad.
    pub spots: Vec<(f32, f32)>,
    /// Spare spots for squads beyond what the place holds (more squads on guard than there are
    /// posts, say), so no two squads are sent to the same ground.
    pub extra_spots: Vec<(f32, f32)>,
    pub source: String,
}

impl Place {
    /// All its spots: the regular ones, then the spare ones.
    pub fn spot_count(&self) -> usize {
        self.spots.len() + self.extra_spots.len()
    }

    /// Spot `k`, counting the regular spots first, then the spare ones.
    pub fn spot(&self, k: usize) -> (f32, f32) {
        if k < self.spots.len() { self.spots[k] } else { self.extra_spots[k - self.spots.len()] }
    }
}

#[derive(Deserialize)]
struct PlaceFile {
    place: Vec<PlaceEntry>,
}

#[derive(Deserialize)]
struct PlaceEntry {
    name: String,
    kind: PlaceKind,
    x: f32,
    y: f32,
    squads: u32,
    source: String,
}

/// Squads at the same place stand at least this far apart (a squad in formation is about 8 m across).
const SPOT_SPACING_M: f32 = 12.0;
/// Spare spots worked out around each place, where the ground allows.
const EXTRA_SPOTS_PER_PLACE: usize = 2;

/// Reads the places file and works out each squad's spot on the ground: around the place's own
/// point, on ground that can be stood on, reachable in a straight line from it, and clear of every
/// other spot. The spare spots are worked out after all the regular ones, so they never move a
/// regular spot.
pub fn places_from_toml(text: &str, terrain: &Terrain) -> Result<Vec<Place>, String> {
    let file: PlaceFile = toml::from_str(text).map_err(|e| format!("places file: {e}"))?;
    let mut offsets = Vec::new();
    for j in -4i32..=4 {
        for i in -4i32..=4 {
            offsets.push((i, j));
        }
    }
    offsets.sort_by_key(|&(i, j)| (i * i + j * j, j, i));
    let mut taken: Vec<(f32, f32)> = Vec::new();
    let mut places = Vec::new();
    for e in file.place {
        if e.squads == 0 {
            return Err(format!("places file: {} holds no squads", e.name));
        }
        if !terrain.in_main_region(e.x, e.y) {
            return Err(format!("places file: {} at ({}, {}) is not on walkable ground", e.name, e.x, e.y));
        }
        let spots = find_spots(terrain, &offsets, &mut taken, (e.x, e.y), e.squads as usize);
        if spots.len() < e.squads as usize {
            return Err(format!("places file: no room around {} for {} squads", e.name, e.squads));
        }
        places.push(Place { name: e.name, kind: e.kind, x: e.x, y: e.y, spots, extra_spots: Vec::new(), source: e.source });
    }
    for p in &mut places {
        p.extra_spots = find_spots(terrain, &offsets, &mut taken, (p.x, p.y), EXTRA_SPOTS_PER_PLACE);
    }
    Ok(places)
}

/// Up to `n` spots around `centre`, nearest first, clear of every spot already `taken` (which they
/// join).
fn find_spots(terrain: &Terrain, offsets: &[(i32, i32)], taken: &mut Vec<(f32, f32)>, centre: (f32, f32), n: usize) -> Vec<(f32, f32)> {
    let mut spots = Vec::new();
    for &(i, j) in offsets {
        if spots.len() == n {
            break;
        }
        let (x, y) = (centre.0 + i as f32 * SPOT_SPACING_M, centre.1 + j as f32 * SPOT_SPACING_M);
        let clear = taken.iter().all(|&(tx, ty)| (tx - x).powi(2) + (ty - y).powi(2) >= (SPOT_SPACING_M - 0.01).powi(2));
        if clear && terrain.in_main_region(x, y) && terrain.segment_walkable(centre.0, centre.1, x, y) {
            spots.push((x, y));
            taken.push((x, y));
        }
    }
    spots
}

/// `data/places/col_di_lana_1916.toml`'s text.
pub fn load_col_di_lana_1916_text() -> Result<String, String> {
    let path = crate::data_dir().join("places").join("col_di_lana_1916.toml");
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// How tired a man is, in words: below a quarter of the way to exhausted he is fresh.
pub fn tiredness_word(t: f32) -> &'static str {
    if t >= EXHAUSTED {
        "exhausted"
    } else if t >= 0.25 {
        "tired"
    } else {
        "fresh"
    }
}

/// Tiredness counts as exhausted from here (1.0 is the most, reached after the tuning's hours
/// awake; the margin absorbs rounding from adding it up minute by minute).
pub const EXHAUSTED: f32 = 0.999;

impl TirednessTuning {
    /// The share of the light's spotting distance a man with this tiredness sees to.
    pub fn spotting_factor(&self, t: f32) -> f32 {
        1.0 - (1.0 - self.exhausted_spotting_factor) * t
    }

    /// The share of his walking speed a man with this tiredness keeps.
    pub fn walking_factor(&self, t: f32) -> f32 {
        1.0 - (1.0 - self.exhausted_walking_factor) * t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario;
    use crate::sim::{Order, SquadState, make_pool};
    use crate::tuning::Tuning;
    use crate::weather::Weather;
    use std::sync::Arc;

    /// More squads on guard than there are posts: each extra squad gets a spare spot of its own and
    /// gets there (sharing one spot left men jostling for ever).
    #[test]
    fn more_guards_than_posts_all_reach_their_spots() {
        let (tuning, _) = Tuning::load_default().unwrap();
        let terrain = Arc::new(Terrain::load_col_di_lana(tuning.walking.steepest_walkable_deg).unwrap());
        let (diary, _) = Weather::load_march_1916().unwrap();
        let text = load_col_di_lana_1916_text().unwrap();
        let mut sim = scenario::step2(terrain, tuning, 1916, make_pool(4), false, diary, &text).unwrap();
        let ours: Vec<u32> = sim.squads.iter().filter(|q| q.roster.is_some()).map(|q| q.id).collect();
        for &q in &ours {
            sim.order(Order::SetRoster { squad: q, day: Duty::Guard, night: Duty::Rest });
        }
        let end = sim.tuning.seconds_to_ticks(30.0 * 60.0);
        while sim.tick < end {
            sim.step();
        }
        let mut spots: Vec<(usize, usize)> = Vec::new();
        for &q in &ours {
            let s = &sim.squads[q as usize];
            assert!(matches!(s.state, SquadState::Arrived | SquadState::Idle), "squad {q} is still {:?}", s.state);
            let post = s.post.expect("on duty");
            assert_eq!(sim.places[post.0].kind, PlaceKind::Post);
            assert!(!spots.contains(&post), "two squads share spot {post:?}");
            spots.push(post);
        }
    }
}
