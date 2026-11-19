//! The locked tests for carrying parties, cold and hunger (docs/scenario-tests.md, 30, 31 and 34 to
//! 37; 32 and 33 withdrawn). They test the lasting rules, not this week's numbers (decision 56):
//! where chance comes in, a test raises the chance to a certainty in its own copy of the supply
//! file, so it checks that the thing can happen, not how likely it is. Every scenario runs twice and
//! the runs must match.
//!
//! Run with: cargo test --release -p alpine_sim --test supply

use alpine_sim::report;
use alpine_sim::roster::{self, Duty, PlaceKind, Roster};
use alpine_sim::scenario;
use alpine_sim::sim::{Order, Side, Sim, make_pool};
use alpine_sim::sky::{Calendar, DateTime};
use alpine_sim::supply::{CarryLeg, Lost, SupplyFile};
use alpine_sim::terrain::Terrain;
use alpine_sim::tuning::Tuning;
use alpine_sim::weather::Weather;
use std::sync::Arc;
use std::time::Instant;

struct World {
    terrain: Arc<Terrain>,
    tuning: Tuning,
    diary_toml: String,
    places_toml: String,
    supply_toml: String,
}

type Outcome = Result<String, String>;

fn at(text: &str) -> f64 {
    DateTime::parse(text).unwrap().to_seconds()
}

fn ticks(w: &World, seconds: f64) -> u64 {
    (seconds * w.tuning.time.ticks_per_second as f64).round() as u64
}

fn minutes(w: &World, tick: u64) -> f64 {
    tick as f64 / w.tuning.time.ticks_per_second as f64 / 60.0
}

fn hm(min: f64) -> String {
    format!("{} h {:02} min", (min / 60.0) as u32, (min % 60.0) as u32)
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

/// The step-2 game as played, with the supply file.
fn game(w: &World) -> Sim {
    scenario::build(scenario::STEP2, w.terrain.clone(), w.tuning.clone(), 1916, make_pool(4), &w.diary_toml, &w.places_toml, &w.supply_toml, "", "")
        .unwrap()
}

/// A small game: `squads` squads of `men` each on the given rosters, at a moment, in a steady
/// temperature with a clear sky, with the supply file (as changed by `tune`) in force.
fn small(w: &World, start: f64, celsius: f64, rosters: &[Roster], men: usize, tune: impl Fn(&mut SupplyFile)) -> Sim {
    let mut sim = Sim::new(w.terrain.clone(), w.tuning.clone(), 11, make_pool(2));
    let mut cal: Calendar = scenario::step2_calendar(&w.terrain, &w.tuning).unwrap();
    cal.start = start;
    sim.set_clock(cal, Weather::constant(0.0).with_temperature(celsius));
    let list: Vec<(u32, Roster)> = rosters.iter().map(|&r| (sim.add_squad(Side::Austria, &vec![(0.0, 0.0); men]), r)).collect();
    sim.start_roster(roster::places_from_toml(&w.places_toml, &w.terrain).unwrap(), &list);
    let mut file = SupplyFile::from_toml(&w.supply_toml).unwrap();
    tune(&mut file);
    sim.start_supply(file).unwrap();
    sim
}

/// The first resting squad of the game, set to carry by day, with the store at `days` of each.
fn carrying_game(w: &World, days: f32) -> (Sim, u32) {
    let mut sim = game(w);
    let q = sim.squads.iter().find(|q| q.inside && q.roster == Some(Roster { day: Duty::Rest, night: Duty::Rest })).unwrap().id;
    let sup = sim.supply.as_mut().unwrap();
    sup.food_kg = sup.food_per_day_kg * days;
    sup.fuel_kg = sup.fuel_per_day_kg * days;
    sim.order(Order::SetRoster { squad: q, day: Duty::Carry, night: Duty::Rest });
    (sim, q)
}

// ---- 30. Carrying delivers ------------------------------------------------------------------------

fn test_delivers(w: &World) -> Outcome {
    let r = twice(|| -> Result<(u64, u32, u32), String> {
        let (mut sim, q) = carrying_game(w, 1.0);
        let men = sim.squads[q as usize].members.len() as u32;
        // A generous limit, only so a broken rule cannot hang the test: the rule is that a
        // delivery arrives, not how soon (decision 56).
        while sim.tick < ticks(w, 8.0 * 3600.0) {
            sim.step();
            let c = sim.supply.as_ref().unwrap().carried_today;
            if c[0] + c[1] > 0.0 {
                return Ok((sim.tick, (c[0] + c[1]).round() as u32, men));
            }
        }
        Err("no load reached the store in 8 game hours".into())
    })??;
    let (tick, kg, men) = r;
    let load = SupplyFile::from_toml(&w.supply_toml).unwrap().carrying.load_kg;
    let expected = (men as f32 * load).round() as u32;
    if kg != expected {
        return Err(format!("the first delivery brought {kg} kg; its {men} men's loads are {expected} kg"));
    }
    Ok(format!(
        "a resting squad set to carry by day, with the store at 1 day of each, made its first delivery from the Sief dump {} after setting off: {kg} kg, its {men} men's loads",
        hm(minutes(w, tick))
    ))
}

// ---- 31. The store has a limit --------------------------------------------------------------------

fn test_limit(w: &World) -> Outcome {
    let r = twice(|| -> Result<(bool, bool, f64), String> {
        // Just short of full: the first load more than fills it.
        let (mut sim, q) = carrying_game(w, 2.95);
        let (food_cap, fuel_cap) = {
            let s = sim.supply.as_ref().unwrap();
            (s.food_cap_kg, s.fuel_cap_kg)
        };
        let mut over = false;
        let mut rested = None;
        while sim.tick < ticks(w, 10.0 * 3600.0) && rested.is_none() {
            sim.step();
            let s = sim.supply.as_ref().unwrap();
            over |= s.food_kg > food_cap + 1e-3 || s.fuel_kg > fuel_cap + 1e-3;
            let sq = &sim.squads[q as usize];
            if sq.carry == Some(CarryLeg::Done) && sq.inside {
                rested = Some(sim.tick);
            }
        }
        let full = sim.supply.as_ref().unwrap().full();
        Ok((over, full, rested.map_or(-1.0, |t| minutes(w, t))))
    })??;
    match r {
        (true, _, _) => Err("the store held more than it can".into()),
        (_, _, m) if m < 0.0 => Err("the carrying squad did not go back to rest in a shelter once the store was full".into()),
        (_, full, m) => Ok(format!(
            "with the store just short of full, the carrying squad's load filled it without going past what it holds{}; the squad then went back to its shelter to rest, inside {} after setting off",
            if full { "" } else { " (some went unused)" },
            hm(m)
        )),
    }
}

// ---- 34. Neglect costs men, and 36. The lost leave ------------------------------------------------

/// Three squads on night guard with no fuel, from noon (they rest the afternoon), in a steady
/// −16 °C. Returns, per man lost, (game minutes, why), and the squads' sizes at the end, plus the
/// morning report's company line.
fn neglect(w: &World, starving: bool, tune: impl Fn(&mut SupplyFile) + Copy) -> (Vec<(u32, String)>, Vec<usize>, String) {
    let night = Roster { day: Duty::Rest, night: Duty::Guard };
    let mut sim = small(w, at("1916-03-08 12:00"), -16.0, &[night; 3], 10, tune);
    let sup = sim.supply.as_mut().unwrap();
    sup.fuel_kg = 0.0;
    if starving {
        sup.food_kg = 0.0;
        for s in sim.soldiers.iter_mut() {
            s.minutes_since_meal = 72 * 60;
        }
    }
    // Four nights at most.
    while sim.tick < ticks(w, 4.0 * 24.0 * 3600.0) {
        sim.step();
        if starving {
            // No food reaches them.
            sim.supply.as_mut().unwrap().food_kg = 0.0;
        }
        sim.supply.as_mut().unwrap().fuel_kg = 0.0;
    }
    let lost = sim.supply.as_ref().unwrap().lost.iter().map(|l| (minutes(w, l.tick) as u32, format!("{:?}", l.why))).collect();
    let sizes = sim.squads.iter().map(|q| q.members.len()).collect();
    let line = report::morning_report(&sim).map(|r| r.events[0].clone()).unwrap_or_default();
    (lost, sizes, line)
}

fn test_neglect(w: &World) -> Outcome {
    // Frostbite: certain once a man is cold enough, so the test checks that it happens.
    let frost = |f: &mut SupplyFile| {
        f.cold.frostbite_chance_per_hour_from = 60.0;
        f.cold.frostbite_chance_per_hour_worst = 60.0;
    };
    let (lost, _, _) = twice(|| neglect(w, false, frost))?;
    let first_frost = lost.iter().find(|l| l.1 == "Frostbite").map(|l| l.0);
    let Some(ff) = first_frost else { return Err("with no fuel, three squads on night guard had no frostbite in four nights".into()) };
    if ff as f64 > 36.0 * 60.0 {
        return Err(format!("the first frostbite came only after {}, beyond the second night", hm(ff as f64)));
    }
    // Death: certain once a man has been at the worst cold for the set hours; no frostbite, so men
    // stay out long enough. Fed against starving.
    let die = |f: &mut SupplyFile| {
        f.cold.frostbite_chance_per_hour_from = 0.0;
        f.cold.frostbite_chance_per_hour_worst = 0.0;
        f.cold.death_chance_per_hour = 60.0;
    };
    let (fed, _, _) = twice(|| neglect(w, false, die))?;
    let (starving, _, _) = twice(|| neglect(w, true, die))?;
    let first = |l: &[(u32, String)]| l.iter().find(|x| x.1 == "Died").map(|x| x.0 as f64);
    let (f, s) = (first(&fed), first(&starving));
    match (f, s) {
        (_, None) => Err("starving, with no fuel, on night guard, nobody died in four nights".into()),
        (Some(f), Some(s)) if s >= f => Err(format!("the first death came {} in when starving, no sooner than {} when fed", hm(s), hm(f))),
        (f, Some(s)) => Ok(format!(
            "three squads guarding by night with no fuel in a steady −16 °C (chances of frostbite and death set to certain, to test the rule): the first frostbite came {} after noon on the first day, within two nights; starving, the first death came {} in, {}",
            hm(ff as f64),
            hm(s),
            match f {
                Some(f) => format!("sooner than when fed ({})", hm(f)),
                None => "while fed, nobody died in four nights".to_string(),
            }
        )),
    }
}

fn test_lost_leave(w: &World) -> Outcome {
    // Once with frostbite certain, once with death certain (and no frostbite), so both kinds of
    // loss are checked.
    let frost = |f: &mut SupplyFile| {
        f.cold.frostbite_chance_per_hour_from = 60.0;
        f.cold.frostbite_chance_per_hour_worst = 60.0;
    };
    let die = |f: &mut SupplyFile| {
        f.cold.frostbite_chance_per_hour_from = 0.0;
        f.cold.frostbite_chance_per_hour_worst = 0.0;
        f.cold.death_chance_per_hour = 60.0;
    };
    let mut lines = Vec::new();
    for (label, case) in [("frostbite", twice(|| neglect(w, false, frost))?), ("death", twice(|| neglect(w, true, die))?)] {
        let (lost, sizes, line) = case;
        let total: usize = sizes.iter().sum();
        let frost_n = lost.iter().filter(|l| l.1 == format!("{:?}", Lost::Frostbite)).count();
        let dead_n = lost.iter().filter(|l| l.1 == format!("{:?}", Lost::Died)).count();
        let wanted = if label == "frostbite" { frost_n } else { dead_n };
        if wanted == 0 {
            return Err(format!("with {label} made certain, nobody was lost to it, so this could not be checked"));
        }
        if total + lost.len() != 30 {
            return Err(format!("{} men were lost but the squads hold {total} of 30", lost.len()));
        }
        if !line.contains(&format!("{frost_n} sent down with frostbite, {dead_n} dead")) {
            return Err(format!("the morning report does not count the {frost_n} frostbitten and {dead_n} dead: \"{line}\""));
        }
        lines.push(format!("with {label} made certain, {} men were lost and left their squads ({total} of 30 remain); the morning report says: \"{line}\"", lost.len()));
    }
    Ok(lines.join("; "))
}

// ---- 35. Hunger tires -----------------------------------------------------------------------------

fn test_hunger(w: &World) -> Outcome {
    let r = twice(|| -> (u32, u32) {
        let day = Roster { day: Duty::Guard, night: Duty::Rest };
        // From just after Wednesday's first light to before dusk: no meal falls in between.
        let mut sim = small(w, at("1916-03-08 06:30"), -8.0, &[day, day], 10, |_| {});
        let (fed, hungry) = (sim.squads[0].members.clone(), sim.squads[1].members.clone());
        for &m in &hungry {
            sim.soldiers[m as usize].minutes_since_meal = 72 * 60;
        }
        while sim.tick < ticks(w, 11.0 * 3600.0) {
            sim.step();
        }
        let avg = |ms: &[u32]| (ms.iter().map(|&m| sim.soldiers[m as usize].tiredness).sum::<f32>() / ms.len() as f32 * 1000.0) as u32;
        (avg(&fed), avg(&hungry))
    })?;
    let (fed, starving) = r;
    let pct = |t: u32| format!("{:.0}%", t as f64 / 10.0);
    let msg = format!(
        "two squads guarding from 06:30 to 17:30 in −8 °C: the fed one ended {} of the way to exhausted, the starving one {}",
        pct(fed),
        pct(starving)
    );
    if starving > fed { Ok(msg) } else { Err(msg + "; expected the starving squad more tired") }
}

// ---- 37. Night carrying is slower -----------------------------------------------------------------

/// Minutes for a squad carrying around the clock, at the West post's squad's spot, to reach the dump
/// from the store's spot, starting at a given moment.
fn trip(w: &World, start: f64) -> Result<f64, String> {
    let carry = Roster { day: Duty::Carry, night: Duty::Carry };
    let mut sim = small(w, start, -10.0, &[carry], 10, |_| {});
    // Stand the squad at the store, with room in the store, and send it off from there.
    {
        let store = sim.supply.as_ref().unwrap().store;
        let (x, y) = sim.places[store].spots[0];
        let slots = sim.formation_around(x, y, 10);
        for (k, &m) in sim.squads[0].members.clone().iter().enumerate() {
            let s = &mut sim.soldiers[m as usize];
            (s.x, s.y) = slots[k];
            s.slot = slots[k];
        }
        let sup = sim.supply.as_mut().unwrap();
        sup.food_kg = sup.food_cap_kg * 0.5;
        sup.fuel_kg = sup.fuel_cap_kg * 0.5;
    }
    sim.order(Order::SetRoster { squad: 0, day: Duty::Rest, night: Duty::Rest });
    sim.step();
    sim.order(Order::SetRoster { squad: 0, day: Duty::Carry, night: Duty::Carry });
    let t0 = sim.tick;
    while sim.tick < t0 + ticks(w, 4.0 * 3600.0) {
        sim.step();
        if matches!(sim.squads[0].carry, Some(CarryLeg::Loading { .. })) {
            return Ok(minutes(w, sim.tick - t0));
        }
    }
    let at_dump = sim.squads[0].post.map(|(p, _)| sim.places[p].kind) == Some(PlaceKind::Dump);
    Err(format!("the squad did not reach the dump in 4 game hours (heading there: {at_dump})"))
}

fn test_night(w: &World) -> Outcome {
    let (day, night) = twice(|| (trip(w, at("1916-03-08 10:00")), trip(w, at("1916-03-08 22:00"))))?;
    let (day, night) = (day.map_err(|e| format!("by day: {e}"))?, night.map_err(|e| format!("by night: {e}"))?);
    let msg = format!("the walk from the summit store to the Sief dump took {} by day and {} by night", hm(day), hm(night));
    if night > day { Ok(msg) } else { Err(msg + "; expected longer by night") }
}

fn main() {
    let started = Instant::now();
    let (tuning, _) = Tuning::load_default().expect("tuning file");
    let terrain = Arc::new(Terrain::load_col_di_lana(tuning.walking.steepest_walkable_deg).expect("terrain"));
    let (_, diary_toml) = Weather::load_march_1916().expect("weather diary");
    let places_toml = roster::load_col_di_lana_1916_text().expect("places file");
    let supply_toml = alpine_sim::supply::load_text().expect("supply file");
    let world = World { terrain, tuning, diary_toml, places_toml, supply_toml };
    let w = &world;

    let mut results: Vec<(&str, &str, Outcome)> = Vec::new();
    let mut run = |n: &'static str, title: &'static str, f: &dyn Fn() -> Outcome| {
        let t = Instant::now();
        let r = f();
        eprintln!("  test {n} done in {:.0} s", t.elapsed().as_secs_f64());
        results.push((n, title, r));
    };
    run("30", "Carrying delivers", &|| test_delivers(w));
    run("31", "The store has a limit", &|| test_limit(w));
    run("34", "Neglect costs men", &|| test_neglect(w));
    run("35", "Hunger tires", &|| test_hunger(w));
    run("36", "The lost leave", &|| test_lost_leave(w));
    run("37", "Night carrying is slower", &|| test_night(w));

    let failed = results.iter().filter(|r| r.2.is_err()).count();
    println!();
    println!("Alpine Front: carrying, cold and hunger tests (locked, docs/scenario-tests.md; 32 and 33 withdrawn)");
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
