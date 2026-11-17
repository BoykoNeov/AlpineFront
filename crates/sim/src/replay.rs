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

pub const FORMAT: u32 = 1;
/// A checksum is recorded every this many ticks.
pub const CHECK_EVERY: u64 = 10;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Replay {
    pub format: u32,
    pub game_version: String,
    pub scenario: String,
    pub seed: u64,
    /// The tuning file's text at the time of recording, so later edits don't break the replay.
    pub tuning_toml: String,
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
    pub fn new(scenario: &str, seed: u64, tuning_toml: &str) -> Self {
        Recorder {
            replay: Replay {
                format: FORMAT,
                game_version: env!("CARGO_PKG_VERSION").to_string(),
                scenario: scenario.to_string(),
                seed,
                tuning_toml: tuning_toml.to_string(),
                ticks: 0,
                orders: Vec::new(),
                checksums: Vec::new(),
            },
        }
    }

    /// Call after every `Sim::step`.
    pub fn after_step(&mut self, sim: &Sim) {
        self.replay.ticks = sim.tick;
        if sim.tick % CHECK_EVERY == 0 {
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
        if r.format != FORMAT {
            return Err(format!("replay format {} not supported (expected {FORMAT})", r.format));
        }
        Ok(r)
    }

    /// A fresh simulation set up as at the start of the recording, with every order scheduled.
    pub fn start(&self, terrain: Arc<Terrain>, pool: Arc<rayon::ThreadPool>) -> Result<Sim, String> {
        let tuning = Tuning::from_toml(&self.tuning_toml)?;
        let mut sim = scenario::build(&self.scenario, terrain, tuning, self.seed, pool)
            .ok_or_else(|| format!("unknown scenario '{}'", self.scenario))?;
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
