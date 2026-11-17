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
