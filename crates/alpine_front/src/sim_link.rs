//! Runs the simulation at the chosen game speed and keeps what the view needs from it.

use crate::Options;
use alpine_sim::replay::{Recorder, Replay};
use alpine_sim::scenario;
use alpine_sim::sim::{Sim, make_pool};
use alpine_sim::terrain::Terrain;
use alpine_sim::tuning::Tuning;
use bevy::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub const SPEEDS: [u32; 5] = [1, 2, 4, 8, 16];

#[derive(Clone, Debug, PartialEq)]
pub enum ReplayStatus {
    /// Every checksum so far matched the recording.
    Matching,
    Diverged { tick: u64 },
    /// Reached the end of the recording, matching throughout.
    Finished,
}

pub struct Playback {
    pub file: PathBuf,
    checksums: Vec<(u64, u64)>,
    next: usize,
    pub end_tick: u64,
    pub status: ReplayStatus,
}

#[derive(Resource)]
pub struct SimLink {
    pub sim: Sim,
    pub scenario: String,
    /// Records the session so it can be saved as a replay (not while playing one back).
    pub recorder: Option<Recorder>,
    pub playback: Option<Playback>,
    pub speed: u32,
    pub paused: bool,
    accumulator: f64,
    /// Positions before the latest tick, for smooth drawing between ticks.
    pub previous: Vec<(f32, f32)>,
    /// How far between the previous and latest tick the drawing is (0..1).
    pub alpha: f32,
    /// Wall-clock milliseconds the latest ticks took (smoothed), and the worst in the last second.
    pub tick_ms: f32,
    pub tick_ms_worst: f32,
    worst_window: Vec<f32>,
    pub last_message: Option<String>,
}

impl SimLink {
    pub fn start(o: &Options) -> Result<SimLink, String> {
        let (tuning, tuning_toml) = Tuning::load_default()?;
        let pool = make_pool(tuning.threads.simulation_threads);
        let (mut sim, scenario, recorder, playback) = match &o.replay {
            Some(file) => {
                let replay = Replay::load(file)?;
                let tuning = Tuning::from_toml(&replay.tuning_toml)?;
                let terrain = Arc::new(Terrain::load_col_di_lana(tuning.walking.steepest_walkable_deg)?);
                let sim = replay.start(terrain, pool)?;
                let playback = Playback {
                    file: file.clone(),
                    checksums: replay.checksums.clone(),
                    next: 0,
                    end_tick: replay.ticks,
                    status: ReplayStatus::Matching,
                };
                (sim, replay.scenario.clone(), None, Some(playback))
            }
            None => {
                let terrain = Arc::new(Terrain::load_col_di_lana(tuning.walking.steepest_walkable_deg)?);
                let name = if o.autoplay || o.frame_check.is_some() { scenario::STEP1_AUTOPLAY } else { scenario::STEP1 };
                let sim = scenario::build(name, terrain, tuning, o.seed, pool).unwrap();
                let rec = Recorder::new(name, o.seed, &tuning_toml);
                (sim, name.to_string(), Some(rec), None)
            }
        };
        let mut recorder = recorder;
        // Fast-forward if asked (recorded like any other ticks).
        let ahead = (o.advance_minutes * 60.0 * sim.tuning.time.ticks_per_second as f32) as u64;
        for _ in 0..ahead {
            sim.step();
            if let Some(r) = recorder.as_mut() {
                r.after_step(&sim);
            }
        }
        let previous = sim.soldiers.iter().map(|s| (s.x, s.y)).collect();
        Ok(SimLink {
            sim,
            scenario,
            recorder,
            playback,
            speed: if o.frame_check.is_some() { 16 } else { 1 },
            paused: false,
            accumulator: 0.0,
            previous,
            alpha: 1.0,
            tick_ms: 0.0,
            tick_ms_worst: 0.0,
            worst_window: Vec::new(),
            last_message: None,
        })
    }

    /// Where soldier `i` should be drawn now (between the last two ticks), in simulation metres.
    pub fn drawn_position(&self, i: usize) -> (f32, f32) {
        let s = &self.sim.soldiers[i];
        let (px, py) = self.previous.get(i).copied().unwrap_or((s.x, s.y));
        (px + (s.x - px) * self.alpha, py + (s.y - py) * self.alpha)
    }

    /// Saves the session so far as a replay file under `replays/`.
    pub fn save_replay(&mut self) -> Result<PathBuf, String> {
        let rec = self.recorder.as_ref().ok_or("a replay being played back is not recorded again")?;
        let replay = rec.finish(&self.sim);
        let dir = PathBuf::from("replays");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = dir.join(format!("{}-seed{}-{stamp}.ron", self.scenario, self.sim.seed));
        replay.save(&path)?;
        Ok(std::fs::canonicalize(&path).unwrap_or(path))
    }

    pub fn max_speed(&self) -> u32 {
        self.sim.tuning.time.max_speed
    }
}

pub struct SimLinkPlugin;

impl Plugin for SimLinkPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, advance_simulation);
    }
}

/// Runs as many ticks as the elapsed time and game speed call for.
fn advance_simulation(time: Res<Time>, mut link: ResMut<SimLink>) {
    if link.paused {
        link.alpha = 1.0;
        return;
    }
    if let Some(p) = &link.playback
        && link.sim.tick >= p.end_tick
    {
        link.paused = true;
        if let Some(p) = link.playback.as_mut()
            && p.status == ReplayStatus::Matching
        {
            p.status = ReplayStatus::Finished;
        }
        return;
    }
    let dt = link.sim.tuning.dt() as f64;
    let speed = link.speed.min(link.max_speed()) as f64;
    link.accumulator += time.delta_secs_f64() * speed;
    // Never run more than twice the ticks a frame should need; if the simulation falls behind,
    // the game slows down rather than freezing.
    let max_ticks = ((speed / 30.0 / dt).ceil() as u32).max(2);
    let mut ran = 0;
    while link.accumulator >= dt && ran < max_ticks {
        let link = &mut *link;
        link.previous.clear();
        link.previous.extend(link.sim.soldiers.iter().map(|s| (s.x, s.y)));
        let start = Instant::now();
        link.sim.step();
        let ms = start.elapsed().as_secs_f32() * 1000.0;
        link.tick_ms = if link.tick_ms == 0.0 { ms } else { link.tick_ms * 0.95 + ms * 0.05 };
        link.worst_window.push(ms);
        if let Some(rec) = link.recorder.as_mut() {
            rec.after_step(&link.sim);
        }
        if let Some(p) = link.playback.as_mut() {
            while p.next < p.checksums.len() && p.checksums[p.next].0 <= link.sim.tick {
                let (t, c) = p.checksums[p.next];
                p.next += 1;
                if t == link.sim.tick && c != link.sim.checksum() && p.status == ReplayStatus::Matching {
                    p.status = ReplayStatus::Diverged { tick: t };
                }
            }
            if link.sim.tick >= p.end_tick {
                break;
            }
        }
        link.accumulator -= dt;
        ran += 1;
    }
    if ran == max_ticks {
        link.accumulator = link.accumulator.min(dt);
    }
    let window = (speed / dt) as usize;
    if link.worst_window.len() > window.max(1) {
        let excess = link.worst_window.len() - window.max(1);
        link.worst_window.drain(..excess);
    }
    link.tick_ms_worst = link.worst_window.iter().copied().fold(0.0, f32::max);
    link.alpha = (link.accumulator / dt).clamp(0.0, 1.0) as f32;
}
