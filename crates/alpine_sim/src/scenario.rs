//! Starting situations.

use crate::sim::{Event, LoggedEvent, PatrolZone, Side, Sim};
use crate::sky::{Calendar, DateTime};
use crate::terrain::Terrain;
use crate::tuning::Tuning;
use crate::weather::Weather;
use std::sync::Arc;

/// Names of the scenarios a replay can refer to.
pub const STEP1: &str = "step1";
/// Step 1 with every squad, Austrian too, wandering on seeded random orders.
pub const STEP1_AUTOPLAY: &str = "step1-autoplay";
/// Step 2: the step-1 troops on the calendar, opening at first light on Monday 6 March 1916.
pub const STEP2: &str = "step2";
/// Step 2 with every squad, Austrian too, wandering on seeded random orders.
pub const STEP2_AUTOPLAY: &str = "step2-autoplay";

/// Whether a scenario uses the weather diary (and so needs its text, e.g. in a replay).
pub fn uses_weather(name: &str) -> bool {
    name == STEP2 || name == STEP2_AUTOPLAY
}

/// Builds a named scenario. `weather_toml` is the weather diary's text (ignored by step 1).
pub fn build(
    name: &str,
    terrain: Arc<Terrain>,
    tuning: Tuning,
    seed: u64,
    pool: Arc<rayon::ThreadPool>,
    weather_toml: &str,
) -> Result<Sim, String> {
    match name {
        STEP1 => Ok(step1(terrain, tuning, seed, pool, false)),
        STEP1_AUTOPLAY => Ok(step1(terrain, tuning, seed, pool, true)),
        STEP2 | STEP2_AUTOPLAY => {
            let weather = Weather::from_toml(weather_toml)?;
            step2(terrain, tuning, seed, pool, name == STEP2_AUTOPLAY, weather)
        }
        _ => Err(format!("unknown scenario '{name}'")),
    }
}

/// The step-2 calendar: Col di Lana, Central European Time, starting at first light (dawn) on the
/// tuning's first day.
pub fn step2_calendar(terrain: &Terrain, tuning: &Tuning) -> Result<Calendar, String> {
    let first = DateTime::parse(&format!("{} 00:00", tuning.scenario.step2_first_day))?;
    // The summit's position if the map gives it (it does), else the summit's published position.
    let (latitude, longitude) = terrain.centre_lat_lon.unwrap_or((46.50, 11.96));
    let mut cal = Calendar { latitude, longitude, utc_offset_h: 1.0, start: first.to_seconds() };
    cal.start = cal.first_light_on(cal.start).ok_or("no first light on the first day")?;
    Ok(cal)
}

/// Build step 2: the step-1 troops on the calendar. The game opens at first light on the first day,
/// which counts as that morning's dawn.
pub fn step2(
    terrain: Arc<Terrain>,
    tuning: Tuning,
    seed: u64,
    pool: Arc<rayon::ThreadPool>,
    autoplay: bool,
    weather: Weather,
) -> Result<Sim, String> {
    let cal = step2_calendar(&terrain, &tuning)?;
    let mut sim = step1(terrain, tuning, seed, pool, autoplay);
    sim.set_clock(cal, weather);
    sim.journal.push(LoggedEvent { tick: 0, event: Event::Dawn { day: 0 } });
    Ok(sim)
}

/// Picks a random point in a ring around the summit that is on the main walkable ground.
fn random_spot(sim: &mut Sim, min_r: f32, max_r: f32, max_h: f32, south_only: bool, taken: &[(f32, f32)]) -> (f32, f32) {
    let terrain = sim.terrain.clone();
    for _ in 0..10_000 {
        let rng = sim.rng();
        let a = if south_only {
            rng.range(std::f32::consts::PI * 1.1, std::f32::consts::PI * 1.9)
        } else {
            rng.range(0.0, std::f32::consts::TAU)
        };
        let r = rng.range(min_r, max_r);
        let (x, y) = (r * a.cos(), r * a.sin());
        if terrain.in_main_region(x, y)
            && terrain.height(x, y) <= max_h
            && taken.iter().all(|&(tx, ty)| (tx - x).powi(2) + (ty - y).powi(2) > 40.0f32.powi(2))
        {
            return (x, y);
        }
    }
    panic!("no room to place a squad");
}

/// Build step 1: Austro-Hungarian squads around the summit, Italian squads patrolling the southern
/// slopes below. With `autoplay`, the Austrians patrol too.
pub fn step1(terrain: Arc<Terrain>, tuning: Tuning, seed: u64, pool: Arc<rayon::ThreadPool>, autoplay: bool) -> Sim {
    let sc = tuning.scenario.clone();
    let men = tuning.squad.men_per_squad as usize;
    let mut sim = Sim::new(terrain, tuning, seed, pool);
    let mut taken = Vec::new();

    for _ in 0..sc.austrian_squads {
        let c = random_spot(&mut sim, 0.0, sc.austrian_start_radius_m, f32::INFINITY, false, &taken);
        taken.push(c);
        let places = sim.formation_around(c.0, c.1, men);
        let id = sim.add_squad(Side::Austria, &places);
        if autoplay {
            let zone = PatrolZone { min_r: 0.0, max_r: 1200.0, max_height: f32::INFINITY, south_only: false };
            let first = sim.rng().range(10.0, 600.0);
            let first = sim.tuning.seconds_to_ticks(first);
            sim.set_patrol(id, zone, first);
        }
    }

    let zone = PatrolZone {
        min_r: sc.italian_zone_min_m,
        max_r: sc.italian_zone_max_m,
        max_height: sc.italian_zone_max_height_m,
        south_only: true,
    };
    for _ in 0..sc.italian_squads {
        let c = random_spot(&mut sim, zone.min_r, zone.max_r, zone.max_height, true, &taken);
        taken.push(c);
        let places = sim.formation_around(c.0, c.1, men);
        let id = sim.add_squad(Side::Italy, &places);
        let first = sim.rng().range(10.0, 300.0);
        let first = sim.tuning.seconds_to_ticks(first);
        sim.set_patrol(id, zone, first);
    }
    sim
}
