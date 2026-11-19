//! The locked tests for falling snow and the rule switches (docs/scenario-tests.md, 38 to 44). They
//! test the lasting rules, not this week's numbers (decision 56): that fresh snow slows, that a
//! track is quicker, that snowshoes help, that snow cuts sight, that routes use tracks, that
//! forecasts can be wrong, and that every switch, turned off, gives the game without its rule
//! (decision 65). Every scenario runs twice and the runs must match.
//!
//! Run with: cargo test --release -p alpine_sim --test snow

use alpine_sim::flow::FlowField;
use alpine_sim::report;
use alpine_sim::roster::{self, Duty, PlaceKind, Roster};
use alpine_sim::scenario;
use alpine_sim::sim::{Order, Side, Sim, SquadState, make_pool};
use alpine_sim::sky::DateTime;
use alpine_sim::snow::{RouteSnow, SnowAmount, SnowFile};
use alpine_sim::supply::SupplyFile;
use alpine_sim::terrain::Terrain;
use alpine_sim::tuning::Tuning;
use alpine_sim::weather::Weather;
use std::sync::Arc;
use std::time::Instant;

struct World {
    terrain: Arc<Terrain>,
    tuning: Tuning,
    diary: Weather,
    places_toml: String,
    supply_toml: String,
    snow_toml: String,
}

type Outcome = Result<String, String>;

fn at(text: &str) -> f64 {
    DateTime::parse(text).unwrap().to_seconds()
}

fn minutes(w: &World, ticks: u64) -> f64 {
    ticks as f64 / w.tuning.time.ticks_per_second as f64 / 60.0
}

/// Runs `f` twice, side by side, and checks both runs give the same answer.
fn twice<T: PartialEq + std::fmt::Debug + Send>(f: impl Fn() -> T + Sync) -> Result<T, String> {
    let (a, b) = std::thread::scope(|s| {
        let ha = s.spawn(&f);
        let hb = s.spawn(&f);
        (ha.join().unwrap(), hb.join().unwrap())
    });
    if a != b {
        return Err(format!("the two runs differed: {a:?} against {b:?}"));
    }
    Ok(a)
}

/// The two ends of the test walk, on the ridge path from the summit toward Monte Sief, snapped to
/// walkable ground.
fn ends(w: &World) -> ((f32, f32), (f32, f32)) {
    let t = &w.terrain;
    let a = t.nearest_main_point(20.0, 62.0, 30).expect("start on walkable ground");
    let b = t.nearest_main_point(-160.0, 240.0, 30).expect("end on walkable ground");
    (t.point_xy(a.0, a.1), t.point_xy(b.0, b.1))
}

/// A small game at `start` with weather `weather`: one squad of ten of ours at the start of the test
/// walk, and the snow file (as changed by `tune`) in force unless `snow` is false; then `cm` of
/// loose fresh snow lying everywhere.
fn walker(w: &World, start: f64, weather: Weather, snow: bool, cm: f32, tune: impl Fn(&mut SnowFile)) -> Sim {
    let mut sim = Sim::new(w.terrain.clone(), w.tuning.clone(), 7, make_pool(2));
    let mut cal = scenario::step2_calendar(&w.terrain, &w.tuning).unwrap();
    cal.start = start;
    sim.set_clock(cal, weather);
    let (a, _) = ends(w);
    let slots = sim.formation_around(a.0, a.1, 10);
    sim.add_squad(Side::Austria, &slots);
    if snow {
        let mut file = SnowFile::from_toml(&w.snow_toml).unwrap();
        file.snowshoes.squads.clear();
        tune(&mut file);
        sim.start_snow(file).unwrap();
        if let Some(sn) = sim.snow.as_mut() {
            sn.loose.fill(cm);
            sn.untrodden = cm;
            sn.for_routes = Arc::new(sn.loose.clone());
        }
    }
    sim
}

/// Sends squad 0 to `to` and returns the game ticks until all its men are there.
fn trip(sim: &mut Sim, to: (f32, f32)) -> Result<u64, String> {
    sim.order(Order::Move { squad: 0, x: to.0, y: to.1 });
    let t0 = sim.tick;
    sim.step();
    while sim.tick < t0 + 3 * 3600 * 10 {
        sim.step();
        match sim.squads[0].state {
            SquadState::Arrived => return Ok(sim.tick - t0),
            SquadState::Unreachable => return Err("the squad could not get there".into()),
            _ => {}
        }
    }
    Err("the squad did not arrive within 3 game hours".into())
}

const NOON: &str = "1916-03-08 12:00";

// ---- 38. Fresh snow slows ------------------------------------------------------------------------

fn test_slows(w: &World) -> Outcome {
    let (_, b) = ends(w);
    let (bare, deep) = twice(|| {
        let bare = trip(&mut walker(w, at(NOON), Weather::constant(0.0), false, 0.0, |_| {}), b);
        let deep = trip(&mut walker(w, at(NOON), Weather::constant(0.0), true, 30.0, |_| {}), b);
        (bare, deep)
    })?;
    let (bare, deep) = (bare?, deep?);
    let msg = format!(
        "a squad on foot took {:.1} min for the walk from the summit toward the Sief without fresh snow, {:.1} min through 30 cm",
        minutes(w, bare),
        minutes(w, deep)
    );
    if deep > bare { Ok(msg) } else { Err(msg + "; expected longer in the snow") }
}

// ---- 39. A packed track is quicker ---------------------------------------------------------------

fn test_track(w: &World) -> Outcome {
    let (a, b) = ends(w);
    let (first, again) = twice(|| -> Result<(u64, u64), String> {
        let mut sim = walker(w, at(NOON), Weather::constant(0.0), true, 30.0, |_| {});
        let first = trip(&mut sim, b)?;
        trip(&mut sim, a)?;
        let again = trip(&mut sim, b)?;
        Ok((first, again))
    })??;
    let msg = format!(
        "through 30 cm of fresh snow the squad broke the trail in {:.1} min; walking it again on its own track took {:.1} min",
        minutes(w, first),
        minutes(w, again)
    );
    if again < first { Ok(msg) } else { Err(msg + "; expected the track to be quicker") }
}

// ---- 40. Snowshoes help --------------------------------------------------------------------------

fn test_snowshoes(w: &World) -> Outcome {
    let (_, b) = ends(w);
    let (foot, shoes) = twice(|| {
        let foot = trip(&mut walker(w, at(NOON), Weather::constant(0.0), true, 30.0, |_| {}), b);
        let shoes = trip(&mut walker(w, at(NOON), Weather::constant(0.0), true, 30.0, |f| f.snowshoes.squads = vec![0]), b);
        (foot, shoes)
    })?;
    let (foot, shoes) = (foot?, shoes?);
    let msg = format!("through 30 cm the walk took {:.1} min on foot and {:.1} min on snowshoes", minutes(w, foot), minutes(w, shoes));
    if shoes < foot { Ok(msg) } else { Err(msg + "; expected snowshoes to be quicker") }
}

// ---- 41. Falling snow cuts sight -----------------------------------------------------------------

fn sight(w: &World, when: &str, snow: bool, tune: impl Fn(&mut SnowFile)) -> f32 {
    let mut sim = walker(w, at(when), Weather::constant(0.0).with_snowfall(4.0), snow, 0.0, tune);
    sim.step();
    sim.spotting_range
}

fn test_sight(w: &World) -> Outcome {
    let r = twice(|| [sight(w, NOON, false, |_| {}), sight(w, NOON, true, |_| {}), sight(w, "1916-03-08 22:00", false, |_| {}), sight(w, "1916-03-08 22:00", true, |_| {})])?;
    let msg = format!(
        "a man was spotted at {:.0} m at noon without falling snow and at {:.0} m in heavy snow; at 22:00 at {:.0} m and {:.0} m",
        r[0], r[1], r[2], r[3]
    );
    if r[1] < r[0] && r[3] < r[2] { Ok(msg) } else { Err(msg + "; expected falling snow to shorten both") }
}

// ---- 42. Routes use tracks -----------------------------------------------------------------------

fn test_routes(w: &World) -> Outcome {
    let t = &w.terrain;
    let file = SnowFile::from_toml(&w.snow_toml).unwrap();
    let (a, b) = ends(w);
    let r = twice(|| -> Result<(f32, f32, usize), String> {
        let dest = t.nearest_point(b.0, b.1);
        let from: Vec<(usize, usize)> = vec![t.nearest_point(a.0, a.1)];
        // A detour: the ground's own route from the start to a point well off the straight way,
        // then from there to the end, packed into a track through 60 cm of loose snow.
        let ground = FlowField::compute_bounded(t, &w.tuning.walking, dest, &from, None);
        let straight = ground.route_from(t, &w.tuning.walking, a.0, a.1, 200);
        let near = |pts: &[(f32, f32)], p: (f32, f32)| pts.iter().map(|q| (q.0 - p.0).hypot(q.1 - p.1)).fold(f32::INFINITY, f32::min);
        // The bend: walkable ground beside the halfway point, at least 50 m from the ground's route
        // (the narrow ridge leaves little room, so try both sides at several distances).
        let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt();
        let side = [60.0f32, -60.0, 90.0, -90.0, 120.0, -120.0, 150.0, -150.0]
            .iter()
            .filter_map(|&off| t.nearest_main_point(mid.0 - dy / len * off, mid.1 + dx / len * off, 2))
            .map(|g| t.point_xy(g.0, g.1))
            .find(|&p| near(&straight, p) >= 50.0)
            .ok_or("no walkable ground 50 m beside the way")?;
        let to_side = FlowField::compute_bounded(t, &w.tuning.walking, t.nearest_point(side.0, side.1), &from, None);
        let leg1 = to_side.route_from(t, &w.tuning.walking, a.0, a.1, 200);
        let leg2 = ground.route_from(t, &w.tuning.walking, side.0, side.1, 200);
        let mut loose = vec![60.0f32; t.cols * t.rows];
        for leg in [&leg1, &leg2] {
            for p in leg.windows(2) {
                let steps = ((p[1].0 - p[0].0).hypot(p[1].1 - p[0].1) / 2.0).ceil().max(1.0) as usize;
                for k in 0..=steps {
                    let f = k as f32 / steps as f32;
                    let (c, r) = t.nearest_point(p[0].0 + (p[1].0 - p[0].0) * f, p[0].1 + (p[1].1 - p[0].1) * f);
                    loose[t.idx(c, r)] = 0.0;
                }
            }
        }
        let snow = RouteSnow { loose: Arc::new(loose), table: file.walking.on_foot.clone() };
        let tracked = FlowField::compute_bounded(t, &w.tuning.walking, dest, &from, Some(snow));
        let route = tracked.route_from(t, &w.tuning.walking, a.0, a.1, 200);
        Ok((near(&straight, side), near(&route, side), route.len()))
    })??;
    let msg = format!(
        "with a packed track bending off the ground's own route, through 60 cm of loose snow elsewhere, the route came within {:.0} m of the track's bend; the ground's own route passes {:.0} m from it",
        r.1, r.0
    );
    if r.1 < 15.0 && r.0 > 30.0 { Ok(msg) } else { Err(msg + "; expected the route to follow the track") }
}

// ---- 43. Forecasts can be wrong ------------------------------------------------------------------

/// The forecast at each dawn of the week, as said and as the diary brings it.
fn forecasts(w: &World, tune: impl Fn(&mut SnowFile)) -> Vec<([SnowAmount; 2], [SnowAmount; 2])> {
    let cal0 = scenario::step2_calendar(&w.terrain, &w.tuning).unwrap();
    (0..7)
        .map(|d| {
            let mut sim = Sim::new(w.terrain.clone(), w.tuning.clone(), 100 + d, make_pool(1));
            let mut cal = cal0.clone();
            cal.start = cal.first_light_on(cal0.start + d as f64 * 86_400.0).unwrap();
            sim.set_clock(cal, w.diary.clone());
            let mut file = SnowFile::from_toml(&w.snow_toml).unwrap();
            file.snowshoes.squads.clear();
            tune(&mut file);
            sim.start_snow(file).unwrap();
            let f = sim.snow.as_ref().unwrap().forecast.expect("a forecast");
            (f.said, f.truth)
        })
        .collect()
}

fn test_forecasts(w: &World) -> Outcome {
    let (right, wrong) = twice(|| (forecasts(w, |f| f.switches.wrong_forecasts = false), forecasts(w, |f| f.forecast.wrong_chance = 1.0)))?;
    let snowy = right.iter().filter(|f| f.1.iter().any(|&a| a != SnowAmount::None)).count();
    if snowy == 0 {
        return Err("no dawn of the week forecast any snow, though the diary has snow".into());
    }
    if let Some(d) = right.iter().position(|f| f.0 != f.1) {
        return Err(format!("with wrong forecasts switched off, day {d}'s forecast still differed from the diary"));
    }
    if let Some(d) = wrong.iter().position(|f| f.0[0] == f.1[0] || f.0[1] == f.1[1]) {
        return Err(format!("with the wrong-forecast chance made certain, day {d}'s forecast was still right"));
    }
    Ok(format!(
        "with wrong forecasts switched off, all 7 dawn forecasts matched the diary ({snowy} of them with snow coming); with the chance of a wrong forecast made certain, every one was wrong"
    ))
}

// ---- 44. Every switch, turned off, gives the game without its rule ------------------------------

/// Checksums of a game every 10 updates for `n` updates.
fn sums(sim: &mut Sim, n: u64) -> Vec<u64> {
    (0..n)
        .filter_map(|k| {
            sim.step();
            (k % 10 == 0).then(|| sim.checksum())
        })
        .collect()
}

/// A small roster game: two squads of ten of ours at a moment in a steady temperature, with the
/// supply file (as changed by `tune`) in force.
fn roster_game(w: &World, start: f64, rosters: &[Roster], tune: impl Fn(&mut SupplyFile)) -> Sim {
    let mut sim = Sim::new(w.terrain.clone(), w.tuning.clone(), 11, make_pool(2));
    let mut cal = scenario::step2_calendar(&w.terrain, &w.tuning).unwrap();
    cal.start = start;
    sim.set_clock(cal, Weather::constant(0.0).with_temperature(-15.0));
    let list: Vec<(u32, Roster)> = rosters.iter().map(|&r| (sim.add_squad(Side::Austria, &[(0.0, 0.0); 10]), r)).collect();
    sim.start_roster(roster::places_from_toml(&w.places_toml, &w.terrain).unwrap(), &list);
    let mut file = SupplyFile::from_toml(&w.supply_toml).unwrap();
    tune(&mut file);
    sim.start_supply(file).unwrap();
    sim
}

fn test_switches(w: &World) -> Outcome {
    let (_, b) = ends(w);
    let snowing = || Weather::constant(1.0).with_snowfall(3.0);
    let checks = twice(|| -> Vec<(&'static str, Result<(), String>)> {
        let mut out: Vec<(&'static str, Result<(), String>)> = Vec::new();
        let walk = |sim: &mut Sim| {
            sim.order(Order::Move { squad: 0, x: b.0, y: b.1 });
        };

        // Snowfall off: the game with no snow file at all, checksum for checksum, while it snows.
        let mut off = walker(w, at(NOON), snowing(), true, 0.0, |f| f.switches.snowfall = false);
        let mut none = walker(w, at(NOON), snowing(), false, 0.0, |_| {});
        walk(&mut off);
        walk(&mut none);
        out.push(("snowfall off plays exactly as no snow file", if sums(&mut off, 6000) == sums(&mut none, 6000) { Ok(()) } else { Err("the games differed".into()) }));

        // Snowshoes off: the game in which nobody has snowshoes.
        let mut off = walker(w, at(NOON), snowing(), true, 30.0, |f| {
            f.snowshoes.squads = vec![0];
            f.switches.snowshoes = false;
        });
        let mut none = walker(w, at(NOON), snowing(), true, 30.0, |_| {});
        walk(&mut off);
        walk(&mut none);
        out.push(("snowshoes off plays exactly as nobody having them", if sums(&mut off, 6000) == sums(&mut none, 6000) { Ok(()) } else { Err("the games differed".into()) }));

        // Trail breaking off: walking leaves the snow untouched.
        let mut sim = walker(w, at(NOON), Weather::constant(0.0), true, 30.0, |f| f.switches.trail_breaking = false);
        let walked = trip(&mut sim, b);
        let sn = sim.snow.as_ref().unwrap();
        let (lo, hi) = sn.loose.iter().fold((f32::INFINITY, 0.0f32), |(l, h), &v| (l.min(v), h.max(v)));
        out.push(("trail breaking off: nobody packs the snow", match walked {
            Err(e) => Err(e),
            Ok(_) if hi - lo < 1e-3 && sn.packed.iter().all(|&p| p == 0.0) => Ok(()),
            Ok(_) => Err(format!("fresh snow ran from {lo:.2} to {hi:.2} cm after the walk")),
        }));

        // Slowing off: the walk takes exactly as long as with no snow at all.
        let slow_off = trip(&mut walker(w, at(NOON), Weather::constant(0.0), true, 30.0, |f| f.switches.slows_walking = false), b);
        let bare = trip(&mut walker(w, at(NOON), Weather::constant(0.0), false, 0.0, |_| {}), b);
        out.push(("slowing off: snow does not slow the walk", match (slow_off, bare) {
            (Ok(a), Ok(c)) if a == c => Ok(()),
            (a, c) => Err(format!("{a:?} against {c:?} without snow")),
        }));

        // Routes off: the route is worked out from the ground alone.
        let mut sim = walker(w, at(NOON), Weather::constant(0.0), true, 30.0, |f| f.switches.routes_use_tracks = false);
        let r = trip(&mut sim, b).and_then(|_| if sim.squads[0].field.as_ref().is_some_and(|f| f.snow.is_none()) { Ok(()) } else { Err("the route saw the snow".into()) });
        out.push(("routes off: routes from the ground alone", r));

        // Sight off: falling snow leaves the spotting distance as without snow.
        let (s_off, s_none) = (sight(w, NOON, true, |f| f.switches.cuts_sight = false), sight(w, NOON, false, |_| {}));
        out.push(("sight off: snow does not shorten sight", if s_off == s_none { Ok(()) } else { Err(format!("{s_off} m against {s_none} m")) }));

        // Forecast off: no forecast, and none in the morning report.
        let mut sim = walker(w, at(NOON), Weather::constant(0.0), true, 0.0, |f| f.switches.forecast = false);
        sim.journal.push(alpine_sim::sim::LoggedEvent { tick: 0, event: alpine_sim::sim::Event::Dawn { day: 0 } });
        let said = report::morning_report(&sim).map(|r| r.weather.iter().any(|l| l.starts_with("Forecast"))).unwrap_or(false);
        out.push(("forecast off: no forecast", if sim.snow.as_ref().unwrap().forecast.is_none() && !said { Ok(()) } else { Err("a forecast was given".into()) }));

        // Tiredness off: men on guard never tire.
        let guard = Roster { day: Duty::Guard, night: Duty::Guard };
        let mut tuning = w.tuning.clone();
        tuning.tiredness.enabled = false;
        let mut sim = Sim::new(w.terrain.clone(), tuning, 11, make_pool(2));
        let mut cal = scenario::step2_calendar(&w.terrain, &w.tuning).unwrap();
        cal.start = at(NOON);
        sim.set_clock(cal, Weather::constant(0.0));
        let q = sim.add_squad(Side::Austria, &[(0.0, 0.0); 10]);
        sim.start_roster(roster::places_from_toml(&w.places_toml, &w.terrain).unwrap(), &[(q, guard)]);
        for _ in 0..2 * 3600 * 10 {
            sim.step();
        }
        out.push(("tiredness off: nobody tires", if sim.soldiers.iter().all(|s| s.tiredness == 0.0) { Ok(()) } else { Err("men tired".into()) }));

        // Cold off: guards outside at -15 C never grow cold, and no fuel burns.
        let mut sim = roster_game(w, at(NOON), &[guard], |f| f.switches.cold = false);
        let fuel = sim.supply.as_ref().unwrap().fuel_kg;
        for _ in 0..2 * 3600 * 10 {
            sim.step();
        }
        let cold_ok = sim.soldiers.iter().all(|s| s.chill == 0.0) && sim.supply.as_ref().unwrap().fuel_kg == fuel;
        out.push(("cold off: nobody grows cold, no fuel burns", if cold_ok { Ok(()) } else { Err("men grew cold or fuel burned".into()) }));

        // Hunger off: across the evening meal no food is eaten and nobody grows hungrier.
        let mut sim = roster_game(w, at("1916-03-08 17:00"), &[guard], |f| f.switches.hunger = false);
        let food = sim.supply.as_ref().unwrap().food_kg;
        for _ in 0..3 * 3600 * 10 {
            sim.step();
        }
        let fed = sim.soldiers.iter().all(|s| s.minutes_since_meal == 0) && sim.supply.as_ref().unwrap().food_kg == food;
        out.push(("hunger off: nobody grows hungry, no food is eaten", if fed { Ok(()) } else { Err("food was eaten or men grew hungry".into()) }));

        // Carrying off: a squad set to carry rests in a shelter instead.
        let mut sim = roster_game(w, at(NOON), &[Roster { day: Duty::Rest, night: Duty::Rest }], |f| f.switches.carrying = false);
        sim.order(Order::SetRoster { squad: 0, day: Duty::Carry, night: Duty::Carry });
        for _ in 0..30 * 60 * 10 {
            sim.step();
        }
        let q = &sim.squads[0];
        let rests = q.carry.is_none() && q.post.map(|(p, _)| sim.places[p].kind) == Some(PlaceKind::Shelter);
        out.push(("carrying off: a squad set to carry rests", if rests { Ok(()) } else { Err("the squad went carrying".into()) }));
        out
    })?;
    let failed: Vec<String> = checks.iter().filter_map(|(n, r)| r.as_ref().err().map(|e| format!("{n}: {e}"))).collect();
    if failed.is_empty() {
        Ok(format!("{} switches checked: {}", checks.len(), checks.iter().map(|c| c.0).collect::<Vec<_>>().join("; ")))
    } else {
        Err(failed.join("; "))
    }
}

fn main() {
    let started = Instant::now();
    let (tuning, _) = Tuning::load_default().expect("tuning file");
    let terrain = Arc::new(Terrain::load_col_di_lana(tuning.walking.steepest_walkable_deg).expect("terrain"));
    let (diary, _) = Weather::load_march_1916().expect("weather diary");
    let places_toml = roster::load_col_di_lana_1916_text().expect("places file");
    let supply_toml = alpine_sim::supply::load_text().expect("supply file");
    let snow_toml = alpine_sim::snow::load_text().expect("snow file");
    let world = World { terrain, tuning, diary, places_toml, supply_toml, snow_toml };
    let w = &world;

    let mut results: Vec<(&str, &str, Outcome)> = Vec::new();
    let mut run = |n: &'static str, title: &'static str, f: &dyn Fn() -> Outcome| {
        let t = Instant::now();
        let r = f();
        eprintln!("  test {n} done in {:.0} s", t.elapsed().as_secs_f64());
        results.push((n, title, r));
    };
    run("38", "Fresh snow slows", &|| test_slows(w));
    run("39", "A packed track is quicker", &|| test_track(w));
    run("40", "Snowshoes help", &|| test_snowshoes(w));
    run("41", "Falling snow cuts sight", &|| test_sight(w));
    run("42", "Routes use tracks", &|| test_routes(w));
    run("43", "Forecasts can be wrong", &|| test_forecasts(w));
    run("44", "Every switch, turned off, gives the game without its rule", &|| test_switches(w));

    let failed = results.iter().filter(|r| r.2.is_err()).count();
    println!();
    println!("Alpine Front: falling snow and rule switch tests (locked, docs/scenario-tests.md)");
    println!("=================================================================================");
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
