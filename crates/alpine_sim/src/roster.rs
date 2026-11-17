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
    pub source: String,
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

/// Reads the places file and works out each squad's spot on the ground: around the place's own
/// point, on ground that can be stood on, reachable in a straight line from it, and clear of every
/// other spot.
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
        let mut spots = Vec::new();
        for &(i, j) in &offsets {
            if spots.len() == e.squads as usize {
                break;
            }
            let (x, y) = (e.x + i as f32 * SPOT_SPACING_M, e.y + j as f32 * SPOT_SPACING_M);
            let clear = taken.iter().all(|&(tx, ty)| (tx - x).powi(2) + (ty - y).powi(2) >= (SPOT_SPACING_M - 0.01).powi(2));
            if clear && terrain.in_main_region(x, y) && terrain.segment_walkable(e.x, e.y, x, y) {
                spots.push((x, y));
                taken.push((x, y));
            }
        }
        if spots.len() < e.squads as usize {
            return Err(format!("places file: no room around {} for {} squads", e.name, e.squads));
        }
        places.push(Place { name: e.name, kind: e.kind, x: e.x, y: e.y, spots, source: e.source });
    }
    Ok(places)
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
