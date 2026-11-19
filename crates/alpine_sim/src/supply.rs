//! Carrying parties, cold and hunger (decisions 45 to 51): the summit store of food and fuel, the
//! dump on Monte Sief the carrying parties fetch from, and the numbers for cold and hunger. Read from
//! `data/supply.toml`. Without that file (the locked tests 1 to 29) none of this happens.

use crate::roster::{self, Place, PlaceKind};
use crate::terrain::Terrain;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct SupplyFile {
    #[serde(default)]
    pub switches: SupplySwitches,
    pub dump: PlaceEntry,
    pub store: PlaceEntry,
    pub stocks: StockTuning,
    pub carrying: CarryTuning,
    pub cold: ColdTuning,
    pub hunger: HungerTuning,
}

fn yes() -> bool {
    true
}

/// Carrying, cold and hunger, each on or off (decision 65); off, the game behaves as if that rule
/// had never been built.
#[derive(Clone, Debug, Deserialize)]
pub struct SupplySwitches {
    /// Squads can be set to carry. Off: a squad set to carry rests instead.
    #[serde(default = "yes")]
    pub carrying: bool,
    /// Men grow cold outside and warm in heated shelters; fuel burns. Off: nobody grows cold and no
    /// fuel is needed.
    #[serde(default = "yes")]
    pub cold: bool,
    /// Men grow hungry without meals; food is eaten. Off: nobody grows hungry and no food is needed.
    #[serde(default = "yes")]
    pub hunger: bool,
}

impl Default for SupplySwitches {
    fn default() -> Self {
        SupplySwitches { carrying: true, cold: true, hunger: true }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct PlaceEntry {
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub squads: u32,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct StockTuning {
    pub food_kg_per_man_day: f32,
    pub fuel_kg_per_man_day: f32,
    pub store_days: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CarryTuning {
    pub load_kg: f32,
    pub loading_minutes: f32,
    pub unloading_minutes: f32,
    pub laden_walking_factor: f32,
    pub night_walking_factor: f32,
    pub tiring_factor: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ColdTuning {
    pub per_degree_hour: f32,
    pub moving_factor: f32,
    pub warming_per_hour: f32,
    pub cold_from: f32,
    pub very_cold_from: f32,
    pub worst_walking_factor: f32,
    pub worst_tiring_factor: f32,
    pub frostbite_from: f32,
    pub frostbite_chance_per_hour_from: f32,
    pub frostbite_chance_per_hour_worst: f32,
    pub death_after_hours_at_worst: f32,
    pub death_chance_per_hour: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HungerTuning {
    pub hungry_after_hours: f32,
    pub starving_after_hours: f32,
    pub tiring_factor: f32,
    pub sleep_factor: f32,
    pub chilling_factor: f32,
    pub warming_factor: f32,
}

/// The two stocks the summit keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stock {
    Food,
    Fuel,
}

impl Stock {
    pub fn word(self) -> &'static str {
        match self {
            Stock::Food => "food",
            Stock::Fuel => "fuel",
        }
    }
}

/// Why a man left the company.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lost {
    /// Sent down the mountain with frostbite, out for the week.
    Frostbite,
    /// Died of exposure.
    Died,
}

/// A man lost, with the update after which it happened.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LostMan {
    pub tick: u64,
    pub soldier: u32,
    pub squad: u32,
    pub why: Lost,
}

/// Where a carrying squad is in its round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CarryLeg {
    /// Walking to the dump, empty.
    ToDump,
    /// Loading at the dump until this tick.
    Loading { until: u64 },
    /// Walking to the store with a load of this stock.
    ToStore { stock: Stock },
    /// Unloading at the store until this tick.
    Unloading { until: u64, stock: Stock },
    /// The store is full: resting in a shelter until the shift ends.
    Done,
}

/// The summit's stocks and everything else carrying, cold and hunger keep track of.
#[derive(Clone, Debug)]
pub struct Supply {
    pub tuning: SupplyFile,
    pub food_kg: f32,
    pub fuel_kg: f32,
    /// What the store holds at most, of each (for the men on the summit at the start).
    pub food_cap_kg: f32,
    pub fuel_cap_kg: f32,
    /// What the company uses in a day, of each, for the men on the summit at the start.
    pub food_per_day_kg: f32,
    pub fuel_per_day_kg: f32,
    /// The dump and the store, as indices into `Sim::places`.
    pub dump: usize,
    pub store: usize,
    /// Every man lost so far, in order.
    pub lost: Vec<LostMan>,
    /// Kilograms brought up since the latest dawn, and in the 24 hours before it, of each.
    pub carried_today: [f32; 2],
    pub carried_yesterday: Option<[f32; 2]>,
    /// Meals since the latest dawn at which some men went without, and in the 24 hours before it.
    pub missed_meals_today: u32,
    pub missed_meals_yesterday: Option<u32>,
}

impl Supply {
    /// Days of a stock left at the company's starting size.
    pub fn days_left(&self, stock: Stock) -> f32 {
        match stock {
            Stock::Food => self.food_kg / self.food_per_day_kg.max(1e-6),
            Stock::Fuel => self.fuel_kg / self.fuel_per_day_kg.max(1e-6),
        }
    }

    /// The stock with fewer days left (food when equal).
    pub fn lower(&self) -> Stock {
        if self.days_left(Stock::Fuel) < self.days_left(Stock::Food) { Stock::Fuel } else { Stock::Food }
    }

    /// Whether the store is full of both.
    pub fn full(&self) -> bool {
        self.food_kg >= self.food_cap_kg - 1e-3 && self.fuel_kg >= self.fuel_cap_kg - 1e-3
    }

    /// Adds a load to the store, never past what it holds.
    pub fn unload(&mut self, stock: Stock, kg: f32) {
        let (have, cap) = match stock {
            Stock::Food => (&mut self.food_kg, self.food_cap_kg),
            Stock::Fuel => (&mut self.fuel_kg, self.fuel_cap_kg),
        };
        let added = kg.min((cap - *have).max(0.0));
        *have += added;
        self.carried_today[stock as usize] += added;
    }
}

impl SupplyFile {
    pub fn from_toml(text: &str) -> Result<SupplyFile, String> {
        toml::from_str(text).map_err(|e| format!("supply file: {e}"))
    }
}

impl ColdTuning {
    /// "warm", "cold" or "very cold".
    pub fn word(&self, chill: f32) -> &'static str {
        if chill >= self.very_cold_from {
            "very cold"
        } else if chill >= self.cold_from {
            "cold"
        } else {
            "warm"
        }
    }

    /// How many times as fast as usual a man this cold tires.
    pub fn tiring_factor(&self, chill: f32) -> f32 {
        1.0 + (self.worst_tiring_factor - 1.0) * chill
    }

    /// The share of his walking speed a man this cold keeps.
    pub fn walking_factor(&self, chill: f32) -> f32 {
        1.0 - (1.0 - self.worst_walking_factor) * chill
    }

    /// His chance of frostbite in one hour outside at this coldness.
    pub fn frostbite_per_hour(&self, chill: f32) -> f32 {
        if chill < self.frostbite_from {
            return 0.0;
        }
        let k = ((chill - self.frostbite_from) / (1.0 - self.frostbite_from).max(1e-6)).clamp(0.0, 1.0);
        self.frostbite_chance_per_hour_from + (self.frostbite_chance_per_hour_worst - self.frostbite_chance_per_hour_from) * k
    }
}

impl HungerTuning {
    /// How hungry a man is from the minutes since his last meal: 0 fed (or only just hungry), 1
    /// starving.
    pub fn hunger(&self, minutes_since_meal: u32) -> f32 {
        let h = minutes_since_meal as f32 / 60.0;
        ((h - self.hungry_after_hours) / (self.starving_after_hours - self.hungry_after_hours).max(1e-6)).clamp(0.0, 1.0)
    }

    /// "fed", "hungry" or "starving".
    pub fn word(&self, minutes_since_meal: u32) -> &'static str {
        let h = self.hunger(minutes_since_meal);
        if h >= 1.0 {
            "starving"
        } else if minutes_since_meal as f32 / 60.0 > self.hungry_after_hours {
            "hungry"
        } else {
            "fed"
        }
    }

    /// A factor that is 1 for a fed man and `full` for a starving one, in proportion between.
    pub fn scale(&self, hunger: f32, full: f32) -> f32 {
        1.0 + (full - 1.0) * hunger
    }
}

/// The dump and the store as places, with their spots worked out clear of every existing place's.
pub fn places(file: &SupplyFile, terrain: &Terrain, existing: &[Place]) -> Result<(Place, Place), String> {
    let mut taken: Vec<(f32, f32)> =
        existing.iter().flat_map(|p| p.spots.iter().chain(p.extra_spots.iter()).copied()).collect();
    let mut make = |e: &PlaceEntry, kind: PlaceKind| -> Result<Place, String> {
        if !terrain.in_main_region(e.x, e.y) {
            return Err(format!("supply file: {} at ({}, {}) is not on walkable ground", e.name, e.x, e.y));
        }
        let spots = roster::find_spots_around(terrain, &mut taken, (e.x, e.y), e.squads.max(1) as usize);
        if spots.len() < e.squads.max(1) as usize {
            return Err(format!("supply file: no room around {} for {} squads", e.name, e.squads));
        }
        Ok(Place { name: e.name.clone(), kind, x: e.x, y: e.y, spots, extra_spots: Vec::new(), source: e.source.clone() })
    };
    let mut dump = make(&file.dump, PlaceKind::Dump)?;
    let mut store = make(&file.store, PlaceKind::Store)?;
    dump.extra_spots = roster::find_spots_around(terrain, &mut taken, (dump.x, dump.y), roster::EXTRA_SPOTS_PER_PLACE);
    store.extra_spots = roster::find_spots_around(terrain, &mut taken, (store.x, store.y), roster::EXTRA_SPOTS_PER_PLACE);
    Ok((dump, store))
}

/// `data/supply.toml`'s text.
pub fn load_text() -> Result<String, String> {
    let path = crate::data_dir().join("supply.toml");
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::Tuning;

    #[test]
    fn the_supply_file_loads_and_its_places_fit() {
        let (tuning, _) = Tuning::load_default().unwrap();
        let terrain = Terrain::load_col_di_lana(tuning.walking.steepest_walkable_deg).unwrap();
        let file = SupplyFile::from_toml(&load_text().unwrap()).unwrap();
        let existing = roster::places_from_toml(&roster::load_col_di_lana_1916_text().unwrap(), &terrain).unwrap();
        let (dump, store) = places(&file, &terrain, &existing).unwrap();
        assert_eq!(dump.spots.len(), file.dump.squads as usize);
        assert_eq!(store.spots.len(), file.store.squads as usize);
    }
}
