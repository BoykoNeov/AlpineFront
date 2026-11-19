//! Replays: a seed, a scenario, the tuning in force and the order log, plus checksums of the state
//! along the way so a played-back replay can prove it matches the original.
//!
//! A replay holds on the same machine and game version only (see docs/decisions.md, 7b).

use crate::scenario;
use crate::sim::{LoggedOrder, Sim};
use crate::terrain::Terrain;
use crate::tuning::Tuning;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

/// 2: the replay records which ground it was played on (`terrain`).
/// 3: step 2 has the work roster, and the replay keeps the places file (`places_toml`).
/// 4: step 2 has carrying, cold and hunger, and the replay keeps the supply file (`supply_toml`).
/// 5: step 2 has falling snow, and the replay keeps the snow file (`snow_toml`).
/// 6: step 2 has the night raid, and the replay keeps the raid file (`raid_toml`).
/// Older step-1 replays still play (step 1 did not change); older step-2 replays are refused.
pub const FORMAT: u32 = 6;
/// A checksum is recorded every this many ticks.
pub const CHECK_EVERY: u64 = 10;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Replay {
    pub format: u32,
    pub game_version: String,
    pub scenario: String,
    pub seed: u64,
    /// Fingerprint of the ground it was recorded on (`Terrain::fingerprint`).
    /// Missing in format-1 files, which are then refused by the format check with a clear message.
    #[serde(default)]
    pub terrain: u64,
    /// The tuning file's text at the time of recording, so later edits don't break the replay.
    pub tuning_toml: String,
    /// The weather diary's text at the time of recording (step 2 on; empty for step 1).
    #[serde(default)]
    pub weather_toml: String,
    /// The places file's text at the time of recording (step 2 on, from format 3; empty otherwise).
    #[serde(default)]
    pub places_toml: String,
    /// The supply file's text at the time of recording (the step-2 game, from format 4; empty
    /// otherwise, which means no carrying, cold or hunger).
    #[serde(default)]
    pub supply_toml: String,
    /// The snow file's text at the time of recording (the step-2 game, from format 5; empty
    /// otherwise, which means no snow).
    #[serde(default)]
    pub snow_toml: String,
    /// The raid file's text at the time of recording (the step-2 game, from format 6; empty
    /// otherwise, which means no raid).
    #[serde(default)]
    pub raid_toml: String,
    /// How many ticks were recorded.
    pub ticks: u64,
    pub orders: Vec<LoggedOrder>,
    /// (tick, checksum) pairs, taken after the update that brought the simulation to that tick.
    pub checksums: Vec<(u64, u64)>,
}

/// Records a replay alongside a running simulation.
pub struct Recorder {
    pub replay: Replay,
}

impl Recorder {
    pub fn new(scenario: &str, seed: u64, terrain: u64, tuning_toml: &str, weather_toml: &str, places_toml: &str) -> Self {
        Recorder {
            replay: Replay {
                format: FORMAT,
                game_version: env!("CARGO_PKG_VERSION").to_string(),
                scenario: scenario.to_string(),
                seed,
                terrain,
                tuning_toml: tuning_toml.to_string(),
                weather_toml: weather_toml.to_string(),
                places_toml: places_toml.to_string(),
                supply_toml: String::new(),
                snow_toml: String::new(),
                raid_toml: String::new(),
                ticks: 0,
                orders: Vec::new(),
                checksums: Vec::new(),
            },
        }
    }

    /// Records the supply file the game was started with (the step-2 game).
    pub fn with_supply(mut self, supply_toml: &str) -> Self {
        self.replay.supply_toml = supply_toml.to_string();
        self
    }

    /// Records the snow file the game was started with (the step-2 game).
    pub fn with_snow(mut self, snow_toml: &str) -> Self {
        self.replay.snow_toml = snow_toml.to_string();
        self
    }

    /// Records the raid file the game was started with (the step-2 game).
    pub fn with_raid(mut self, raid_toml: &str) -> Self {
        self.replay.raid_toml = raid_toml.to_string();
        self
    }

    /// Call after every `Sim::step`.
    pub fn after_step(&mut self, sim: &Sim) {
        self.replay.ticks = sim.tick;
        if sim.tick.is_multiple_of(CHECK_EVERY) {
            self.replay.checksums.push((sim.tick, sim.checksum()));
        }
    }

    /// The replay so far, with the simulation's order log.
    pub fn finish(&self, sim: &Sim) -> Replay {
        let mut r = self.replay.clone();
        r.orders = sim.order_log.clone();
        r
    }
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    /// Every checksum matched.
    Matches { ticks: u64 },
    /// First tick at which the state differed.
    Diverged { tick: u64 },
}

impl Replay {
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Replay, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let r: Replay = ron::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let old_step1 = (2..FORMAT).contains(&r.format) && (r.scenario == scenario::STEP1 || r.scenario == scenario::STEP1_AUTOPLAY);
        if r.format != FORMAT && !old_step1 {
            return Err(format!(
                "replay format {} not supported (expected {FORMAT}); replays saved before the map or game changed cannot be played \
                 (step-2 replays from before the work roster, carrying, cold and hunger, falling snow or the night raid, included)",
                r.format
            ));
        }
        Ok(r)
    }

    /// A fresh simulation set up as at the start of the recording, with every order scheduled.
    pub fn start(&self, terrain: Arc<Terrain>, pool: Arc<rayon::ThreadPool>) -> Result<Sim, String> {
        if terrain.fingerprint() != self.terrain {
            return Err("this replay was recorded on different ground (the map has changed since), so it cannot be played back".to_string());
        }
        let tuning = Tuning::from_toml(&self.tuning_toml)?;
        let mut sim = scenario::build(
            &self.scenario,
            terrain,
            tuning,
            self.seed,
            pool,
            &self.weather_toml,
            &self.places_toml,
            &self.supply_toml,
            &self.snow_toml,
            &self.raid_toml,
        )?;
        for o in &self.orders {
            sim.schedule(o.clone());
        }
        Ok(sim)
    }

    /// Plays the replay back headless and compares every recorded checksum.
    pub fn verify(&self, terrain: Arc<Terrain>, pool: Arc<rayon::ThreadPool>) -> Result<Verdict, String> {
        let mut sim = self.start(terrain, pool)?;
        let mut checks = self.checksums.iter().peekable();
        while sim.tick < self.ticks {
            sim.step();
            while let Some(&&(t, c)) = checks.peek() {
                if t > sim.tick {
                    break;
                }
                checks.next();
                if t == sim.tick && c != sim.checksum() {
                    return Ok(Verdict::Diverged { tick: t });
                }
            }
        }
        Ok(Verdict::Matches { ticks: sim.tick })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_different_ground() {
        let flat = Terrain::from_heights(4, 4, 10.0, vec![0.0; 16], 45.0);
        let mut bumped = vec![0.0; 16];
        bumped[5] = 1.0;
        let other = Arc::new(Terrain::from_heights(4, 4, 10.0, bumped, 45.0));
        assert_ne!(flat.fingerprint(), other.fingerprint());
        let rec = Recorder::new(scenario::STEP1, 1, flat.fingerprint(), "", "", "");
        let pool = Arc::new(rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap());
        let err = rec.replay.start(other, pool).err().expect("should refuse");
        assert!(err.contains("different ground"), "{err}");
    }
}
