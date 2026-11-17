//! Starting situations.

use crate::sim::{PatrolZone, Side, Sim};
use crate::terrain::Terrain;
use crate::tuning::Tuning;
use std::sync::Arc;

/// Names of the scenarios a replay can refer to.
pub const STEP1: &str = "step1";
/// Step 1 with every squad, Austrian too, wandering on seeded random orders.
pub const STEP1_AUTOPLAY: &str = "step1-autoplay";

pub fn build(name: &str, terrain: Arc<Terrain>, tuning: Tuning, seed: u64, pool: Arc<rayon::ThreadPool>) -> Option<Sim> {
    match name {
        STEP1 => Some(step1(terrain, tuning, seed, pool, false)),
        STEP1_AUTOPLAY => Some(step1(terrain, tuning, seed, pool, true)),
        _ => None,
    }
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
