//! The locked step-1 scenario tests (docs/scenario-tests.md), with a plain-language report.
//!
//! Every scenario is run twice and both runs must match exactly, tick by tick. Two rules are checked
//! on every tick of every run: nobody stands on ground too steep to stand on (test 6), and the men
//! of an arrived squad stand at least 1 m apart (test 11).
//!
//! Run with: cargo test --release -p alpine_sim --test scenarios

use alpine_sim::replay::{Recorder, Replay, Verdict};
use alpine_sim::scenario;
use alpine_sim::sim::{Order, Side, Sim, SquadState, make_pool};
use alpine_sim::terrain::Terrain;
use alpine_sim::tuning::Tuning;
use std::sync::Arc;
use std::time::Instant;

struct World {
    terrain: Arc<Terrain>,
    tuning: Tuning,
    tuning_toml: String,
    pool8: Arc<rayon::ThreadPool>,
    pool1: Arc<rayon::ThreadPool>,
}

/// Rules checked on every tick of every run.
#[derive(Default)]
struct Watch {
    ticks_checked: u64,
    steepest_seen: f32,
    steep_violation: Option<String>,
    closest_arrived: f32,
    spacing_violation: Option<String>,
}

impl Watch {
    fn check(&mut self, sim: &Sim, test: &str) {
        self.ticks_checked += 1;
        let limit = sim.tuning.walking.steepest_walkable_deg;
        for s in &sim.soldiers {
            let slope = sim.terrain.slope_deg(s.x, s.y);
            if slope > self.steepest_seen {
                self.steepest_seen = slope;
            }
            if slope > limit && self.steep_violation.is_none() {
                self.steep_violation = Some(format!(
                    "in test {test}, soldier {} stood on {slope:.1}° ground at ({:.0}, {:.0}), game time {}",
                    s.id,
                    s.x,
                    s.y,
                    clock(sim.game_seconds())
                ));
            }
        }
        if sim.tick.is_multiple_of(10) {
            for q in &sim.squads {
                if q.state != SquadState::Arrived {
                    continue;
                }
                for (i, &a) in q.members.iter().enumerate() {
                    for &b in &q.members[i + 1..] {
                        let (sa, sb) = (&sim.soldiers[a as usize], &sim.soldiers[b as usize]);
                        let d = ((sa.x - sb.x).powi(2) + (sa.y - sb.y).powi(2)).sqrt();
                        if d < self.closest_arrived || self.closest_arrived == 0.0 {
                            self.closest_arrived = d;
                        }
                        if d < 1.0 && self.spacing_violation.is_none() {
                            self.spacing_violation = Some(format!(
                                "in test {test}, soldiers {a} and {b} of arrived squad {} stood {d:.2} m apart",
                                q.id
                            ));
                        }
                    }
                }
            }
        }
    }
}

fn clock(seconds: f64) -> String {
    let m = (seconds / 60.0).round() as u64;
    format!("{} h {:02} min", m / 60, m % 60)
}

/// Runs a scenario twice from scratch, stepping until `done` or `max_ticks`, and checks the two runs
/// match at every tick. Returns the first run's final state.
fn run_twice(
    test: &str,
    watch: &mut Watch,
    build: &dyn Fn() -> Sim,
    done: &dyn Fn(&Sim) -> bool,
    max_ticks: u64,
    each_tick: &mut dyn FnMut(&Sim),
) -> Result<Sim, String> {
    let mut first: Vec<u64> = Vec::new();
    let mut result = None;
    for run in 0..2 {
        let mut sim = build();
        let mut sums = Vec::new();
        while !done(&sim) && sim.tick < max_ticks {
            sim.step();
            sums.push(sim.checksum());
            if run == 0 {
                watch.check(&sim, test);
                each_tick(&sim);
            }
        }
        if run == 0 {
            first = sums;
            result = Some(sim);
        } else if let Some(t) = first.iter().zip(&sums).position(|(a, b)| a != b) {
            return Err(format!("the two runs differed at tick {}", t + 1));
        } else if first.len() != sums.len() {
            return Err("the two runs lasted different lengths".into());
        }
    }
    Ok(result.unwrap())
}

fn lone(world: &World, a: (Side, (f32, f32)), b: (Side, (f32, f32))) -> Sim {
    let mut sim = Sim::new(world.terrain.clone(), world.tuning.clone(), 1, world.pool8.clone());
    sim.add_squad(a.0, &[a.1]);
    sim.add_squad(b.0, &[b.1]);
    sim
}

fn squad_at(world: &World, at: (f32, f32)) -> Sim {
    let mut sim = Sim::new(world.terrain.clone(), world.tuning.clone(), 7, world.pool8.clone());
    let men = sim.tuning.squad.men_per_squad as usize;
    let places = sim.formation_around(at.0, at.1, men);
    sim.add_squad(Side::Austria, &places);
    sim
}

/// Sends a squad from one place to another; returns the game seconds taken.
fn march(world: &World, watch: &mut Watch, test: &str, from: (f32, f32), to: (f32, f32), limit_s: f64) -> Result<f64, String> {
    let limit = (limit_s * world.tuning.time.ticks_per_second as f64) as u64;
    let build = || {
        let mut sim = squad_at(world, from);
        sim.order(Order::Move { squad: 0, x: to.0, y: to.1 });
        sim
    };
    let sim = run_twice(test, watch, &build, &|s| s.squads[0].state == SquadState::Arrived, limit, &mut |_| {})?;
    let q = &sim.squads[0];
    match q.arrived_tick {
        Some(t) => Ok((t - q.order_tick) as f64 / world.tuning.time.ticks_per_second as f64),
        None => Err(format!("the squad had not arrived after {} (state: {:?})", clock(limit_s), q.state)),
    }
}

type Outcome = Result<String, String>;

fn test_replays(w: &World, watch: &mut Watch) -> Outcome {
    let minutes = 10.0;
    let ticks = (minutes * 60.0 * w.tuning.time.ticks_per_second as f64) as u64;
    let build8 = || scenario::step1(w.terrain.clone(), w.tuning.clone(), 2026, w.pool8.clone(), true);
    let sim8 = run_twice("1", watch, &build8, &|_| false, ticks, &mut |_| {})?;
    // Same again on one thread.
    let mut sim1 = scenario::step1(w.terrain.clone(), w.tuning.clone(), 2026, w.pool1.clone(), true);
    let mut sim8b = scenario::step1(w.terrain.clone(), w.tuning.clone(), 2026, w.pool8.clone(), true);
    while sim1.tick < ticks {
        sim1.step();
        sim8b.step();
        if sim1.checksum() != sim8b.checksum() {
            return Err(format!("1 thread and 8 threads differed at tick {}", sim1.tick));
        }
    }
    if sim8b.checksum() != sim8.checksum() {
        return Err("a third run did not match the first two".into());
    }
    // Record a replay with player orders, save it, load it, play it back.
    let mut sim = scenario::step1(w.terrain.clone(), w.tuning.clone(), 99, w.pool8.clone(), false);
    let mut rec = Recorder::new(scenario::STEP1, 99, w.terrain.fingerprint(), &w.tuning_toml, "", "");
    let orders = [(50u64, 0u32, (300.0, -200.0)), (900, 3, (-400.0, 460.0)), (2000, 0, (0.0, 0.0)), (2500, 7, (500.0, 500.0))];
    while sim.tick < ticks {
        for &(t, squad, (x, y)) in &orders {
            if sim.tick == t {
                sim.order(Order::Move { squad, x, y });
            }
        }
        sim.step();
        rec.after_step(&sim);
    }
    let replay = rec.finish(&sim);
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("scenario_test.replay.ron");
    replay.save(&path)?;
    let loaded = Replay::load(&path)?;
    let _ = std::fs::remove_file(&path);
    match loaded.verify(w.terrain.clone(), w.pool8.clone())? {
        Verdict::Matches { .. } => {}
        Verdict::Diverged { tick } => return Err(format!("the saved replay diverged from the original at tick {tick}")),
    }
    let moved = sim8.soldiers.iter().filter(|s| s.vx != 0.0 || s.vy != 0.0 || !s.arrived).count();
    Ok(format!(
        "{} soldiers over {minutes} game minutes matched exactly at every one of {ticks} ticks, run twice, and on 1 thread and 8 threads; \
         a saved replay with {} player orders played back identically ({} soldiers were on the move at the end)",
        sim8.soldiers.len(),
        replay.orders.len(),
        moved
    ))
}

fn sight_test(
    w: &World,
    watch: &mut Watch,
    test: &str,
    a: (f32, f32),
    b: (f32, f32),
    expect_sight: bool,
    expect_spotted: bool,
) -> Result<(f32, Option<f64>), String> {
    let build = || lone(w, (Side::Austria, a), (Side::Italy, b));
    let mut first_spot: Option<u64> = None;
    let ticks = 3 * w.tuning.time.ticks_per_second as u64;
    let sim = run_twice(test, watch, &build, &|_| false, ticks, &mut |s| {
        if first_spot.is_none() && s.soldiers[0].sees.contains(&1) && s.soldiers[1].sees.contains(&0) {
            first_spot = Some(s.tick);
        }
    })?;
    let ab = sim.can_see(0, 1);
    let ba = sim.can_see(1, 0);
    if ab != ba {
        return Err(format!("sight was one-sided: A sees B = {ab}, B sees A = {ba}"));
    }
    if ab != expect_sight {
        return Err(format!("line of sight was {} but should be {}", yes(ab), yes(expect_sight)));
    }
    let spotted_both = sim.soldiers[0].sees.contains(&1) && sim.soldiers[1].sees.contains(&0);
    let spotted_any = !sim.soldiers[0].sees.is_empty() || !sim.soldiers[1].sees.is_empty();
    if expect_spotted && !spotted_both {
        return Err("they had not spotted each other after 3 game seconds".into());
    }
    if !expect_spotted && spotted_any {
        return Err("one of them was spotted, but should not have been".into());
    }
    let ga = sim.ground(0);
    let gb = sim.ground(1);
    let eye = sim.tuning.soldier.eye_height_m;
    let head = sim.tuning.soldier.head_height_m;
    let clear = alpine_sim::los::clearance(&sim.terrain, &sim.los, [ga[0], ga[1], ga[2] + eye], [gb[0], gb[1], gb[2] + head], false);
    Ok((clear, first_spot.map(|t| t as f64 / w.tuning.time.ticks_per_second as f64)))
}

fn yes(b: bool) -> &'static str {
    if b { "clear" } else { "blocked" }
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn main() {
    let started = Instant::now();
    let (tuning, tuning_toml) = Tuning::load_default().expect("tuning file");
    let terrain = Arc::new(Terrain::load_col_di_lana(tuning.walking.steepest_walkable_deg).expect("terrain"));
    let world = World { terrain, tuning, tuning_toml, pool8: make_pool(8), pool1: make_pool(1) };
    let w = &world;
    let mut watch = Watch::default();
    let mut results: Vec<(&str, &str, Outcome)> = Vec::new();

    let summit = (0.0, 0.0);
    let south = (0.0, -250.0);
    let north = (0.0, 250.0);
    let sief = (-400.0, 460.0);
    let meadow = (0.0, 900.0);

    results.push(("1", "Exact replays", test_replays(w, &mut watch)));

    results.push(("2", "Rounded summit", sight_test(w, &mut watch, "2", summit, south, false, false).map(|(c, _)| {
        format!("a man on the summit and a man 250 m down the south face cannot see each other, either way; the rounded top hides him by {:.1} m", -c)
    })));

    results.push(("3", "Ridge", sight_test(w, &mut watch, "3", south, north, false, false).map(|(c, _)| {
        format!("men 250 m down the south and north faces cannot see each other; the summit is {:.0} m in the way", -c)
    })));

    results.push(("4", "Summit and Monte Sief", sight_test(w, &mut watch, "4", summit, sief, true, true).and_then(|(c, t)| {
        let limit = 2.0;
        match t {
            Some(t) if t <= limit => Ok(format!(
                "the summit and Monte Sief's top ({:.0} m apart) see each other with {c:.1} m to spare, and each man spotted the other after {t:.1} game seconds (limit {limit} s)",
                dist(summit, sief)
            )),
            Some(t) => Err(format!("spotting took {t:.1} game seconds, more than {limit}")),
            None => Err("they never spotted each other".into()),
        }
    })));

    let detour = march(w, &mut watch, "7", meadow, summit, 2.5 * 3600.0).and_then(|s| {
        if s <= 2.0 * 3600.0 {
            Ok(format!(
                "the squad went around the cliff from the meadow north of the summit and arrived after {} (limit 2 h 00 min); the straight line is {:.0} m",
                clock(s),
                dist(meadow, summit)
            ))
        } else {
            Err(format!("the squad took {}, more than 2 hours", clock(s)))
        }
    });
    results.push(("7", "Cliff detour", detour));

    // 10. Nobody gets stuck.
    let stuck = {
        let hours = 2.0;
        let ticks = (hours * 3600.0 * w.tuning.time.ticks_per_second as f64) as u64;
        let limit = (120.0 * w.tuning.time.ticks_per_second as f64) as u32;
        let mut worst: (u32, u32, f32, f32) = (0, 0, 0.0, 0.0);
        let mut arrivals = 0u32;
        let mut prev_arrived = vec![false; 0];
        let build = || scenario::step1(w.terrain.clone(), w.tuning.clone(), 31, w.pool8.clone(), true);
        let r = run_twice("10", &mut watch, &build, &|_| false, ticks, &mut |s| {
            if prev_arrived.is_empty() {
                prev_arrived = vec![false; s.squads.len()];
            }
            for (i, q) in s.squads.iter().enumerate() {
                let a = q.state == SquadState::Arrived;
                if a && !prev_arrived[i] {
                    arrivals += 1;
                }
                prev_arrived[i] = a;
            }
            for sd in &s.soldiers {
                if sd.still_ticks > worst.0 {
                    worst = (sd.still_ticks, sd.id, sd.x, sd.y);
                }
            }
        });
        r.and_then(|_| {
            let secs = worst.0 as f64 / w.tuning.time.ticks_per_second as f64;
            if worst.0 > limit {
                Err(format!(
                    "soldier {} stood still for {:.0} game seconds without arriving, at ({:.0}, {:.0}); limit is 120",
                    worst.1, secs, worst.2, worst.3
                ))
            } else {
                Ok(format!(
                    "in 2 game hours with all 20 squads on random orders ({arrivals} arrivals), the longest anyone stood still on the move was {secs:.1} game seconds (limit 120)"
                ))
            }
        })
    };
    results.push(("10", "Nobody gets stuck", stuck));

    let steep_limit = w.tuning.walking.steepest_walkable_deg;
    results.push((
        "6",
        "No impossible ground",
        match &watch.steep_violation {
            Some(v) => Err(v.clone()),
            None => Ok(format!(
                "over {} checked ticks in all the tests above, the steepest ground anyone stood on was {:.1}° (the tuning file's limit, now {steep_limit:.0}°)",
                watch.ticks_checked, watch.steepest_seen
            )),
        },
    ));
    results.push((
        "11",
        "Spacing",
        match &watch.spacing_violation {
            Some(v) => Err(v.clone()),
            None => Ok(format!(
                "in every arrived squad, in every test, the closest two men stood {:.2} m apart (at least 1 m expected)",
                watch.closest_arrived
            )),
        },
    ));

    results.sort_by_key(|r| r.0.parse::<u32>().unwrap());
    let failed = results.iter().filter(|r| r.2.is_err()).count();
    println!();
    println!("Alpine Front: step-1 scenario tests (locked, docs/scenario-tests.md; 5, 8 and 9 withdrawn, decision 56)");
    println!("=================================================================================================");
    for (n, title, r) in &results {
        match r {
            Ok(d) => println!("PASS {n:>2}. {title}: {d}."),
            Err(d) => println!("FAIL {n:>2}. {title}: {d}."),
        }
    }
    println!();
    println!(
        "Every scenario was run twice; any difference between the runs would show above as a failure. {} of {} passed. Took {:.0} s.",
        results.len() - failed,
        results.len(),
        started.elapsed().as_secs_f64()
    );
    if failed > 0 {
        std::process::exit(1);
    }
}
