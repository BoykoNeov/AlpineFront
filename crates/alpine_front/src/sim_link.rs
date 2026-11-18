//! Runs the simulation at the chosen game speed and keeps what the view needs from it.

use crate::Options;
use alpine_sim::replay::{Recorder, Replay};
use alpine_sim::scenario;
use alpine_sim::sim::{Event, Sim, make_pool};
use alpine_sim::terrain::Terrain;
use alpine_sim::tuning::Tuning;
use alpine_sim::weather::Weather;
use bevy::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub const SPEEDS: [u32; 7] = [1, 2, 4, 8, 16, 32, 64];
/// Real time the skip-ahead may spend running the simulation in each frame, so the screens stay
/// responsive while it runs.
const SKIP_BUDGET_MS: f64 = 12.0;

/// What a new contact or an arrival does to normal play: the player's choice (decision 38).
/// The skip-ahead always stops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OnEvent {
    /// Only show a message.
    #[default]
    Message,
    /// Slow to 8x (if faster).
    Slow8,
    /// Slow to 1x.
    Slow1,
}

impl OnEvent {
    /// The fastest speed allowed right after an event.
    fn speed_cap(self) -> u32 {
        match self {
            OnEvent::Message => u32::MAX,
            OnEvent::Slow8 => 8,
            OnEvent::Slow1 => 1,
        }
    }
}

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
    /// Share of each frame's time spent running the simulation (smoothed), and the worst share in
    /// the last 60 frames. The speed check's limit is half (decision 30).
    pub frame_share: f32,
    pub frame_share_worst: f32,
    share_window: Vec<f32>,
    pub last_message: Option<String>,
    /// The skip-ahead is running: as fast as the computer can until dawn or until something happens.
    pub skipping: bool,
    /// The dawn orders screen is open (the game waits for "Carry on").
    pub dawn_screen: bool,
    /// Automatic runs (autoplay, the frame check) carry on at dawn by themselves.
    pub auto_carry_on: bool,
    /// What events do to normal play.
    pub on_event: OnEvent,
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
                // The frame check plays the game as the player does, on the work roster (decision
                // 44); only --autoplay sets every squad wandering on random orders.
                let name = match (o.step1, o.autoplay) {
                    (true, true) => scenario::STEP1_AUTOPLAY,
                    (true, false) => scenario::STEP1,
                    (false, true) => scenario::STEP2_AUTOPLAY,
                    (false, false) => scenario::STEP2,
                };
                let (weather_toml, places_toml) = if scenario::uses_weather(name) {
                    (Weather::load_march_1916()?.1, alpine_sim::roster::load_col_di_lana_1916_text()?)
                } else {
                    (String::new(), String::new())
                };
                let rec = Recorder::new(name, o.seed, terrain.fingerprint(), &tuning_toml, &weather_toml, &places_toml);
                let sim = scenario::build(name, terrain, tuning, o.seed, pool, &weather_toml, &places_toml)?;
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
        let top_speed = sim.tuning.time.max_speed;
        let auto_carry_on = o.autoplay || o.frame_check.is_some();
        // A game on the calendar opens at dawn, with the orders screen.
        let dawn_screen = sim.clock.is_some() && ahead == 0 && !auto_carry_on;
        Ok(SimLink {
            sim,
            scenario,
            recorder,
            playback,
            speed: if o.frame_check.is_some() { top_speed } else { 1 },
            paused: false,
            accumulator: 0.0,
            previous,
            alpha: 1.0,
            tick_ms: 0.0,
            tick_ms_worst: 0.0,
            worst_window: Vec::new(),
            frame_share: 0.0,
            frame_share_worst: 0.0,
            share_window: Vec::new(),
            last_message: None,
            skipping: false,
            dawn_screen,
            auto_carry_on,
            on_event: OnEvent::default(),
        })
    }

    /// Closes the dawn orders screen and lets time run again.
    pub fn carry_on(&mut self) {
        self.dawn_screen = false;
        self.paused = false;
    }

    /// Starts or stops the skip-ahead.
    pub fn toggle_skip(&mut self) {
        if self.sim.clock.is_none() {
            self.last_message = Some("The skip-ahead needs the calendar (step 2).".into());
            return;
        }
        if self.dawn_screen {
            return;
        }
        self.skipping = !self.skipping;
        if self.skipping {
            self.paused = false;
            self.last_message = Some("Skipping ahead to dawn, or until something happens.".into());
        }
    }

    /// Reacts to the events of the latest update (decisions 36 and 38): dawn opens the orders
    /// screen; a new contact or an arrival stops the skip-ahead, shows a message, and slows normal
    /// play if the player chose so. Returns whether the game's pace changed (so the frame's run of
    /// updates should end here).
    fn react(&mut self, events: &[Event]) -> bool {
        let mut stop = false;
        for e in events {
            if !e.stops_time() {
                continue;
            }
            let when = self.sim.now().map(|t| t.hhmm()).unwrap_or_default();
            let what = match e {
                Event::Dawn { .. } => {
                    if self.auto_carry_on {
                        continue;
                    }
                    self.dawn_screen = true;
                    self.paused = true;
                    self.skipping = false;
                    stop = true;
                    continue;
                }
                Event::Contact { enemy_squad, seen_by_squad, distance_m } => {
                    format!("{when}: Italian squad {enemy_squad} came into sight of our squad {seen_by_squad}, {distance_m:.0} m away.")
                }
                Event::Arrived { squad } => format!("{when}: squad {squad} has arrived."),
                Event::Unreachable { squad } => format!("{when}: squad {squad} cannot get where it was sent."),
                Event::Dusk => continue,
            };
            if self.auto_carry_on {
                continue;
            }
            let was_skipping = self.skipping;
            self.skipping = false;
            let cap = self.on_event.speed_cap();
            let slowed = self.speed > cap;
            if slowed {
                self.speed = cap;
            }
            self.last_message = Some(match (was_skipping, slowed) {
                (_, true) => format!("{what} Slowed to {cap}x."),
                (true, false) => format!("{what} Skip-ahead stopped."),
                (false, false) => what,
            });
            stop |= was_skipping || slowed;
        }
        stop
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

/// Records the latest update for the replay, or checks it against the replay being played back.
fn after_update(sim: &Sim, recorder: &mut Option<Recorder>, playback: &mut Option<Playback>) {
    if let Some(rec) = recorder.as_mut() {
        rec.after_step(sim);
    }
    if let Some(p) = playback.as_mut() {
        while p.next < p.checksums.len() && p.checksums[p.next].0 <= sim.tick {
            let (t, c) = p.checksums[p.next];
            p.next += 1;
            if t == sim.tick && c != sim.checksum() && p.status == ReplayStatus::Matching {
                p.status = ReplayStatus::Diverged { tick: t };
            }
        }
    }
}

/// The skip-ahead: runs the simulation flat out for up to `SKIP_BUDGET_MS` of real time this frame.
fn skip_ahead(link: &mut SimLink) {
    let start = Instant::now();
    let end_tick = link.playback.as_ref().map(|p| p.end_tick).unwrap_or(u64::MAX);
    let SimLink { sim, recorder, playback, .. } = link;
    let stop = sim.skip(
        |s| s.tick < end_tick && start.elapsed().as_secs_f64() * 1000.0 < SKIP_BUDGET_MS,
        |s| after_update(s, recorder, playback),
    );
    link.previous.clear();
    link.previous.extend(link.sim.soldiers.iter().map(|s| (s.x, s.y)));
    link.alpha = 1.0;
    link.accumulator = 0.0;
    if let Some(events) = stop {
        link.react(&events);
    }
}

/// Runs as many ticks as the elapsed time and game speed call for.
fn advance_simulation(time: Res<Time>, mut link: ResMut<SimLink>) {
    if link.dawn_screen {
        link.paused = true;
    }
    if link.paused {
        link.alpha = 1.0;
        return;
    }
    if link.skipping {
        skip_ahead(&mut link);
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
    let mut frame_sim_ms = 0.0;
    while link.accumulator >= dt && ran < max_ticks {
        let link = &mut *link;
        link.previous.clear();
        link.previous.extend(link.sim.soldiers.iter().map(|s| (s.x, s.y)));
        let start = Instant::now();
        link.sim.step();
        let ms = start.elapsed().as_secs_f32() * 1000.0;
        frame_sim_ms += ms;
        link.tick_ms = if link.tick_ms == 0.0 { ms } else { link.tick_ms * 0.95 + ms * 0.05 };
        link.worst_window.push(ms);
        after_update(&link.sim, &mut link.recorder, &mut link.playback);
        link.accumulator -= dt;
        ran += 1;
        if link.playback.as_ref().is_some_and(|p| link.sim.tick >= p.end_tick) {
            break;
        }
        // Dawn, a new contact or an arrival: stop on this exact update.
        if !link.sim.events.is_empty() {
            let events = link.sim.events.clone();
            if link.react(&events) {
                link.accumulator = 0.0;
                break;
            }
        }
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
    let frame_ms = time.delta_secs() * 1000.0;
    if frame_ms > 0.0 {
        let share = frame_sim_ms / frame_ms;
        link.frame_share = link.frame_share * 0.95 + share * 0.05;
        link.share_window.push(share);
        if link.share_window.len() > 60 {
            link.share_window.remove(0);
        }
        link.frame_share_worst = link.share_window.iter().copied().fold(0.0, f32::max);
    }
    link.alpha = (link.accumulator / dt).clamp(0.0, 1.0) as f32;
}
