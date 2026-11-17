//! Tuning numbers, read from `data/tuning.toml`.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tuning {
    pub time: TimeTuning,
    pub threads: ThreadTuning,
    pub walking: WalkingTuning,
    pub squad: SquadTuning,
    pub soldier: SoldierTuning,
    pub spotting: SpottingTuning,
    pub line_of_sight: LosTuning,
    pub scenario: ScenarioTuning,
    /// Optional in the file so replays saved before the light model still load.
    #[serde(default)]
    pub light: LightTuning,
    #[serde(default)]
    pub events: EventTuning,
    #[serde(default)]
    pub tiredness: TirednessTuning,
}

/// How men tire and recover (decisions 39 and 43). Only the player's men tire, and only in games
/// with a work roster.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TirednessTuning {
    /// Hours awake that take a fresh man to exhausted.
    pub hours_awake_to_exhausted: f32,
    /// Hours asleep in a shelter that take an exhausted man to fresh.
    pub hours_asleep_to_fresh: f32,
    /// An exhausted man spots at this share of the light's spotting distance, and walks at this
    /// share of his speed; a less tired man in proportion.
    pub exhausted_spotting_factor: f32,
    pub exhausted_walking_factor: f32,
}

impl Default for TirednessTuning {
    fn default() -> Self {
        TirednessTuning {
            hours_awake_to_exhausted: 18.0,
            hours_asleep_to_fresh: 8.0,
            exhausted_spotting_factor: 0.5,
            exhausted_walking_factor: 0.7,
        }
    }
}

/// Light and how far a man can be seen in it (decisions 33 to 35).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LightTuning {
    /// Pairs of (light on the ground in lux, distance in metres at which a man in a dark coat is
    /// spotted against snow). Read as straight lines on log-log paper between the points.
    pub spotting_by_light: Vec<[f64; 2]>,
    /// Pairs of (sun's height above the horizon in degrees, light in lux under a clear sky), from
    /// high to low.
    pub sun_lux_by_altitude: Vec<[f64; 2]>,
    pub full_moon_lux: f64,
    pub starlight_lux: f64,
    /// Overcast lets this share of the light through; partial cloud lets through this raised to
    /// the cloud cover.
    pub overcast_light_factor: f64,
}

impl Default for LightTuning {
    fn default() -> Self {
        LightTuning {
            spotting_by_light: vec![
                [100.0, 1500.0],
                [10.0, 1000.0],
                [1.0, 500.0],
                [0.25, 300.0],
                [0.01, 120.0],
                [0.002, 50.0],
                [0.0001, 15.0],
            ],
            sun_lux_by_altitude: vec![[0.0, 400.0], [-6.0, 3.0], [-12.0, 0.008], [-18.0, 0.0006]],
            full_moon_lux: 0.25,
            starlight_lux: 0.002,
            overcast_light_factor: 0.05,
        }
    }
}

/// What counts as an event worth stopping the skip-ahead for (decision 36).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventTuning {
    /// An enemy squad coming into view counts as a new contact if none of our men had seen it for
    /// this long.
    pub new_contact_after_unseen_minutes: f32,
}

impl Default for EventTuning {
    fn default() -> Self {
        EventTuning { new_contact_after_unseen_minutes: 30.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeTuning {
    pub ticks_per_second: u32,
    pub max_speed: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThreadTuning {
    pub simulation_threads: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalkingTuning {
    pub tobler_peak_kmh: f32,
    pub tobler_steepness: f32,
    pub tobler_offset: f32,
    pub load_factor: f32,
    pub steepest_walkable_deg: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SquadTuning {
    pub men_per_squad: u32,
    pub formation_spacing_m: f32,
    pub arrival_radius_m: f32,
    pub order_delay_s: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SoldierTuning {
    pub personal_space_m: f32,
    pub eye_height_m: f32,
    pub head_height_m: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpottingTuning {
    pub spotting_range_m: f32,
    pub look_interval_s: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LosTuning {
    pub refraction_coefficient: f32,
    pub earth_radius_m: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScenarioTuning {
    pub austrian_squads: u32,
    pub italian_squads: u32,
    pub austrian_start_radius_m: f32,
    pub italian_zone_min_m: f32,
    pub italian_zone_max_m: f32,
    pub italian_zone_max_height_m: f32,
    pub patrol_pause_min_minutes: f32,
    pub patrol_pause_max_minutes: f32,
    /// The step-2 game opens at sunrise on this day (decision 31).
    #[serde(default = "default_step2_first_day")]
    pub step2_first_day: String,
}

fn default_step2_first_day() -> String {
    "1916-03-06".into()
}

impl Tuning {
    pub fn from_toml(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| format!("tuning file: {e}"))
    }

    pub fn load(path: &Path) -> Result<(Self, String), String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok((Self::from_toml(&text)?, text))
    }

    /// Loads `data/tuning.toml` from the repository.
    pub fn load_default() -> Result<(Self, String), String> {
        Self::load(&crate::data_dir().join("tuning.toml"))
    }

    pub fn dt(&self) -> f32 {
        1.0 / self.time.ticks_per_second as f32
    }

    pub fn seconds_to_ticks(&self, s: f32) -> u64 {
        (s * self.time.ticks_per_second as f32).round().max(1.0) as u64
    }
}

impl WalkingTuning {
    /// Horizontal walking speed in m/s on ground with the given grade (rise over run).
    pub fn speed_mps(&self, grade: f32) -> f32 {
        self.tobler_peak_kmh / 3.6
            * (-self.tobler_steepness * (grade + self.tobler_offset).abs()).exp()
            * self.load_factor
    }
}
