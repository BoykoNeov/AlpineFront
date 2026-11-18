//! The locked step-2 tests (docs/scenario-tests.md, tests 12 to 20; 14 to 16 withdrawn): the
//! calendar and dawn, the moon, twilight and the skip-ahead. Every scenario runs twice and the runs must match.
//!
//! Run with: cargo test --release -p alpine_sim --test step2

use alpine_sim::scenario;
use alpine_sim::sim::{Event, LoggedOrder, Order, PLAYER_SIDE, Sim, SquadState, make_pool};
use alpine_sim::sky::{self, Calendar, DateTime, DAWN_ALTITUDE_DEG, MOONRISE_ALTITUDE_DEG};
use alpine_sim::terrain::Terrain;
use alpine_sim::tuning::Tuning;
use alpine_sim::weather::Weather;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

struct World {
    terrain: Arc<Terrain>,
    tuning: Tuning,
    diary: Weather,
    diary_toml: String,
    places_toml: String,
}

type Outcome = Result<String, String>;

const DAY_S: f64 = 86400.0;

fn at(text: &str) -> f64 {
    DateTime::parse(text).unwrap().to_seconds()
}

fn hhmm(t: f64) -> String {
    DateTime::from_seconds(t).hhmm()
}

/// The step-2 calendar, starting at the given local time instead of Monday's first light.
fn calendar_at(w: &World, start: f64) -> Calendar {
    let mut c = scenario::step2_calendar(&w.terrain, &w.tuning).unwrap();
    c.start = start;
    c
}

/// Runs `f` twice and checks both runs give the same answer.
fn twice<T: PartialEq + std::fmt::Debug>(f: impl Fn() -> T) -> Result<T, String> {
    let a = f();
    let b = f();
    if a != b {
        return Err(format!("the two runs differed: {a:?} against {b:?}"));
    }
    Ok(a)
}

/// The same, with the two runs side by side on their own threads (each run is single-minded and
/// gives the same result on any number of threads, test 1).
fn twice_parallel<T: PartialEq + std::fmt::Debug + Send>(f: impl Fn() -> T + Sync) -> Result<T, String> {
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

// ---- 12. Calendar and dawn ------------------------------------------------------------------------

fn test_dawn(w: &World) -> Outcome {
    // The game opens at first light on Monday (decision 37).
    let open = twice(|| {
        let sim = scenario::step2(w.terrain.clone(), w.tuning.clone(), 1916, make_pool(2), false, w.diary.clone(), &w.places_toml).unwrap();
        let first_dawn = sim.journal.iter().any(|e| e.tick == 0 && e.event == Event::Dawn { day: 0 });
        (sim.now().unwrap().long_date(), sim.now().unwrap().seconds.to_bits(), first_dawn)
    })?;
    if open.0 != "Monday 6 March 1916" || !open.2 {
        return Err(format!("the game opened on {} (dawn noted: {})", open.0, open.2));
    }
    let open_s = f64::from_bits(open.1);
    if (open_s - (6.0 * 3600.0 + 12.0 * 60.0)).abs() > 120.0 {
        return Err(format!("the game opened at {}, not within 2 minutes of 06:12", hhmm(open_s)));
    }
    // Each day of the week, a clock-only simulation from midnight notes dawn once, at first light.
    let dawns = twice_parallel(|| {
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..7)
                .map(|d| {
                    s.spawn(move || {
                        let start = at("1916-03-06 00:00") + d as f64 * DAY_S;
                        let mut sim = Sim::new(w.terrain.clone(), w.tuning.clone(), 1, make_pool(1));
                        sim.set_clock(calendar_at(w, start), w.diary.clone());
                        let end = (DAY_S * w.tuning.time.ticks_per_second as f64) as u64;
                        let mut found = Vec::new();
                        while sim.tick < end {
                            sim.step();
                            if sim.events.iter().any(|e| matches!(e, Event::Dawn { .. })) {
                                found.push(sim.now().unwrap().to_seconds().to_bits());
                            }
                        }
                        found
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect::<Vec<_>>()
        })
    })?;
    let mut times = Vec::new();
    for (d, found) in dawns.iter().enumerate() {
        if found.len() != 1 {
            return Err(format!("day {} had {} dawns", d + 1, found.len()));
        }
        times.push(f64::from_bits(found[0]));
    }
    let cal = calendar_at(w, 0.0);
    for (d, &t) in times.iter().enumerate() {
        let first_light = cal.first_light_on(t).unwrap();
        if (t - first_light).abs() > 1.0 {
            return Err(format!("day {} dawn at {} but first light is at {}", d + 1, hhmm(t), hhmm(first_light)));
        }
        if d > 0 && t - DAY_S >= times[d - 1] {
            return Err("dawn did not come earlier each morning".into());
        }
    }
    let minutes = |t: f64| (t / 60.0).rem_euclid(1440.0);
    let mon = minutes(times[0]) - (6.0 * 60.0 + 12.0);
    let sun = minutes(times[6]) - (6.0 * 60.0 + 1.0);
    if mon.abs() > 2.0 || sun.abs() > 2.0 {
        return Err(format!("Monday dawn {} and Sunday dawn {}, expected 06:12 and 06:01 within 2 minutes", hhmm(times[0]), hhmm(times[6])));
    }
    Ok(format!(
        "the game opens at {} on Monday 6 March 1916; dawn then came once each morning at first light: {} (expected 06:12 and 06:01 within 2 minutes at the ends)",
        hhmm(open_s),
        times.iter().map(|&t| hhmm(t)).collect::<Vec<_>>().join(", ")
    ))
}

// ---- 13. Moon -------------------------------------------------------------------------------------

fn test_moon(w: &World) -> Outcome {
    let (set, lits) = twice(|| {
        let cal = calendar_at(w, 0.0);
        let set = cal.next_set(at("1916-03-10 18:00"), 12.0, MOONRISE_ALTITUDE_DEG, |t| cal.moon(t).0);
        let lits: Vec<u64> = (0..=24).map(|h| cal.moon(at("1916-03-11 00:00") + h as f64 * 3600.0).1.to_bits()).collect();
        (set.map(f64::to_bits), lits)
    })?;
    let set = f64::from_bits(set.ok_or("the moon did not set that night")?);
    let expected = at("1916-03-11 01:42");
    let off = (set - expected) / 60.0;
    let lits: Vec<f64> = lits.into_iter().map(f64::from_bits).collect();
    let (lo, hi) = (lits.iter().copied().fold(1.0, f64::min), lits.iter().copied().fold(0.0, f64::max));
    if off.abs() > 15.0 {
        return Err(format!("the moon set at {}, {off:.0} minutes from 01:42", hhmm(set)));
    }
    if lo < 0.40 || hi > 0.60 {
        return Err(format!("on 11 March the moon was {:.0} to {:.0}% lit, expected 40 to 60%", lo * 100.0, hi * 100.0));
    }
    Ok(format!(
        "the moon set at {} on the night of 10 to 11 March ({:+.0} minutes from the Telve table's 01:42); on 11 March it was {:.0} to {:.0}% lit",
        hhmm(set),
        off,
        lo * 100.0,
        hi * 100.0
    ))
}

// ---- 17. Twilight is gradual ----------------------------------------------------------------------

fn test_twilight(w: &World) -> Outcome {
    let worst = twice(|| {
        let cal = calendar_at(w, 0.0);
        let mut worst = (0.0f64, 0.0f64, 0.0f32, 0.0f32);
        for d in 0..7 {
            let noon = at("1916-03-06 12:00") + d as f64 * DAY_S;
            let sunset = cal.sunset_on(noon).unwrap();
            let night = cal.next_set(sunset, 6.0, -18.0, |t| cal.sun_altitude(t)).unwrap();
            let range = |t: f64| {
                let sky = cal.sky(t, w.diary.cloud_at(t), &w.tuning.light);
                sky::spotting_distance(sky.lux, &w.tuning.light, w.tuning.spotting.spotting_range_m)
            };
            let mut t = sunset;
            let mut prev = range(t);
            while t < night + 60.0 {
                t += 60.0;
                let r = range(t);
                let change = ((r - prev) / prev).abs() as f64;
                if change > worst.0 {
                    worst = (change, t, prev, r);
                }
                prev = r;
            }
        }
        (worst.0.to_bits(), worst.1.to_bits(), worst.2.to_bits(), worst.3.to_bits())
    })?;
    let (change, t, a, b) = (f64::from_bits(worst.0), f64::from_bits(worst.1), f32::from_bits(worst.2), f32::from_bits(worst.3));
    let msg = format!(
        "on every evening of the week, from sunset to full night, the biggest change in one game minute was {:.1}% ({a:.0} m to {b:.0} m at {} on {})",
        change * 100.0,
        hhmm(t),
        DateTime::from_seconds(t).short_date()
    );
    if change <= 0.10 { Ok(msg) } else { Err(msg + "; the limit is 10%") }
}

// ---- 18. Skip stops at dawn -----------------------------------------------------------------------

fn test_skip_night(w: &World) -> Outcome {
    let cal = calendar_at(w, 0.0);
    let nights: Vec<(f64, f64)> = (0..7)
        .map(|d| {
            let noon = at("1916-03-06 12:00") + d as f64 * DAY_S;
            let dusk = cal.next_set(noon, 12.0, sky::DUSK_ALTITUDE_DEG, |t| cal.sun_altitude(t)).unwrap();
            let first_light = cal.first_light_on(noon + DAY_S).unwrap();
            (dusk, first_light)
        })
        .collect();
    // Each night: (stop time, events at the stop, sun before and at the stop, contacts seen), run twice.
    let results = twice_parallel(|| {
        std::thread::scope(|s| {
            let handles: Vec<_> = nights
                .iter()
                .map(|&(dusk, _)| {
                    s.spawn(move || {
                        let mut sim = scenario::step1(w.terrain.clone(), w.tuning.clone(), 1916, make_pool(1), false);
                        sim.set_clock(calendar_at(w, dusk), w.diary.clone());
                        let limit = (14.0 * 3600.0 * w.tuning.time.ticks_per_second as f64) as u64;
                        let mut sun_before = sim.clock.as_ref().unwrap().sky.sun_altitude_deg;
                        let mut sun_prev = sun_before;
                        // Other stops on the way (an Italian patrol coming into sight) are skipped
                        // past: the rule is that the skip-ahead stops at dawn (decision 56).
                        let events = loop {
                            let stop = sim.skip(
                                |s| s.tick < limit,
                                |s| {
                                    sun_before = sun_prev;
                                    sun_prev = s.clock.as_ref().unwrap().sky.sun_altitude_deg;
                                },
                            );
                            match stop {
                                Some(e) if !e.iter().any(|e| matches!(e, alpine_sim::sim::Event::Dawn { .. })) => continue,
                                Some(e) => break format!("{e:?}"),
                                None => break "none".to_string(),
                            }
                        };
                        (sim.now().unwrap().to_seconds().to_bits(), events, sun_before.to_bits(), sun_prev.to_bits())
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect::<Vec<_>>()
        })
    })?;
    let mut lines = Vec::new();
    for ((dusk, first_light), (t, events, before, now)) in nights.iter().zip(&results) {
        let (t, before, now) = (f64::from_bits(*t), f64::from_bits(*before), f64::from_bits(*now));
        let day = DateTime::from_seconds(*dusk).short_date();
        if !events.contains("Dawn") {
            return Err(format!("the night of {day} stopped at {} on {events}, not at dawn", hhmm(t)));
        }
        if !(before < DAWN_ALTITUDE_DEG && now >= DAWN_ALTITUDE_DEG) {
            return Err(format!("the night of {day} stopped on an update where first light had not just come (sun {before:.3}° to {now:.3}°)"));
        }
        if (t - first_light).abs() > 1.0 {
            return Err(format!("the night of {day} stopped at {}, first light was {}", hhmm(t), hhmm(*first_light)));
        }
        lines.push(format!("{} {}-{}", &day[..3], hhmm(*dusk), hhmm(t)));
    }
    Ok(format!(
        "started at dusk on each of the 7 nights, the skip-ahead stopped on the exact update of first light ({})",
        lines.join(", ")
    ))
}

// ---- 19. Skip stops on events ---------------------------------------------------------------------

/// Enemy squads in sight of the player's men right now.
fn seen_now(sim: &Sim) -> Vec<u32> {
    let mut v: Vec<u32> = sim
        .soldiers
        .iter()
        .filter(|s| s.side == PLAYER_SIDE)
        .flat_map(|s| s.sees.iter().map(|&e| sim.soldiers[e as usize].squad))
        .collect();
    v.sort();
    v.dedup();
    v
}

fn test_skip_events(w: &World) -> Outcome {
    let gap = w.tuning.seconds_to_ticks(w.tuning.events.new_contact_after_unseen_minutes * 60.0);
    // Part 1: daytime contacts. Five stops in a row from Monday's dawn, each checked against a
    // separate record of who was in sight.
    let contacts = twice(|| -> Result<Vec<(u64, String)>, String> {
        let mut sim = scenario::step2(w.terrain.clone(), w.tuning.clone(), 1916, make_pool(8), false, w.diary.clone(), &w.places_toml).unwrap();
        let mut last: BTreeMap<u32, u64> = BTreeMap::new();
        let mut stops = Vec::new();
        let mut missed: Option<String> = None;
        let limit = (6.0 * 3600.0 * w.tuning.time.ticks_per_second as f64) as u64;
        while stops.len() < 5 {
            let mut due = false;
            let stop = sim.skip(
                |s| s.tick < limit,
                |s| {
                    due = false;
                    for q in seen_now(s) {
                        let tick = s.tick - 1;
                        if last.get(&q).is_none_or(|&t| tick - t > gap) {
                            due = true;
                        }
                        last.insert(q, tick);
                    }
                    if due && !s.events.iter().any(Event::stops_time) && missed.is_none() {
                        missed = Some(format!("a new contact at {} did not stop the skip-ahead", s.now().unwrap().hhmm()));
                    }
                },
            );
            if let Some(m) = missed.take() {
                return Err(m);
            }
            let Some(events) = stop else { return Err("no contact in 6 game hours of daylight".into()) };
            if !due {
                return Err(format!("the skip-ahead stopped at {} on {events:?} with no new contact due", sim.now().unwrap().hhmm()));
            }
            stops.push((sim.tick, sim.now().unwrap().hhmm()));
        }
        Ok(stops)
    })??;

    // Part 2: our own squads. One sent 150 m away, one sent into the cliffs below Settsass, where no
    // walkable ground lies within 300 m. (A point off the map would not do: the game refuses such a
    // click, and the simulation sends the squad to the nearest edge instead.)
    let moves = twice(|| -> Result<(String, String), String> {
        let mut sim = scenario::step2(w.terrain.clone(), w.tuning.clone(), 1916, make_pool(8), false, w.diary.clone(), &w.places_toml).unwrap();
        let lead = &sim.soldiers[sim.squads[0].members[0] as usize];
        let (x, y) = (lead.x + 150.0, lead.y);
        sim.order(Order::Move { squad: 0, x, y });
        sim.order(Order::Move { squad: 1, x: -150.0, y: 2800.0 });
        let limit = (3.0 * 3600.0 * w.tuning.time.ticks_per_second as f64) as u64;
        let (mut arrived, mut unreachable) = (None, None);
        while arrived.is_none() || unreachable.is_none() {
            let Some(events) = sim.skip(|s| s.tick < limit, |_| {}) else { break };
            for e in events {
                match e {
                    Event::Arrived { squad: 0 } => {
                        let q = &sim.squads[0];
                        if q.state != SquadState::Arrived || q.arrived_tick.map(|t| t + 1) != Some(sim.tick) {
                            return Err("the skip-ahead did not stop on the update squad 0 arrived".into());
                        }
                        arrived = Some(sim.now().unwrap().hhmm());
                    }
                    Event::Unreachable { squad: 1 } => {
                        if sim.squads[1].state != SquadState::Unreachable {
                            return Err("the skip-ahead stopped for squad 1, which was not stuck".into());
                        }
                        unreachable = Some(sim.now().unwrap().hhmm());
                    }
                    _ => {}
                }
            }
        }
        Ok((
            arrived.ok_or("squad 0 never arrived, or the skip-ahead ran past it")?,
            unreachable.ok_or("squad 1's impossible order never stopped the skip-ahead")?,
        ))
    })??;
    Ok(format!(
        "from Monday's dawn the skip-ahead stopped for new Italian contacts at {}, each on the update a squad unseen for 30 minutes came into sight, and missed none; it stopped when squad 0 arrived 150 m away ({}) and when squad 1 was sent into the cliffs below Settsass ({})",
        contacts.iter().map(|c| c.1.clone()).collect::<Vec<_>>().join(", "),
        moves.0,
        moves.1
    ))
}

// ---- 20. Skipping changes nothing -----------------------------------------------------------------

fn test_skip_same(w: &World) -> Outcome {
    let tps = w.tuning.time.ticks_per_second as u64;
    let day = 24 * 3600 * tps;
    let orders = [(600u64, 0u32, (150.0f32, -100.0f32)), (20_000, 5, (-300.0, 200.0)), (200_000, 0, (0.0, 0.0)), (500_000, 9, (300.0, 300.0))];
    let build = || {
        let mut sim = scenario::step2(w.terrain.clone(), w.tuning.clone(), 1916, make_pool(4), false, w.diary.clone(), &w.places_toml).unwrap();
        for &(tick, squad, (x, y)) in &orders {
            sim.schedule(LoggedOrder { tick, order: Order::Move { squad, x, y } });
        }
        sim
    };
    let (normal, skipped) = std::thread::scope(|s| {
        let a = s.spawn(|| {
            let mut sim = build();
            let mut sums = Vec::with_capacity(day as usize);
            while sim.tick < day {
                sim.step();
                sums.push(sim.checksum());
            }
            sums
        });
        let b = s.spawn(|| {
            let mut sim = build();
            let mut sums = Vec::with_capacity(day as usize);
            let mut stops = 0u32;
            while sim.skip(|s| s.tick < day, |s| sums.push(s.checksum())).is_some() {
                stops += 1;
            }
            (sums, stops)
        });
        (a.join().unwrap(), b.join().unwrap())
    });
    let (skipped, stops) = skipped;
    if normal.len() != skipped.len() {
        return Err(format!("the runs lasted {} and {} updates", normal.len(), skipped.len()));
    }
    if let Some(t) = normal.iter().zip(&skipped).position(|(a, b)| a != b) {
        return Err(format!("the runs differed from update {}", t + 1));
    }
    Ok(format!(
        "24 game hours from Monday's dawn with 4 orders, played update by update and crossed with the skip-ahead (which stopped {stops} times), matched at every one of {} updates",
        normal.len()
    ))
}

fn main() {
    let started = Instant::now();
    let (tuning, _) = Tuning::load_default().expect("tuning file");
    let terrain = Arc::new(Terrain::load_col_di_lana(tuning.walking.steepest_walkable_deg).expect("terrain"));
    let (diary, diary_toml) = Weather::load_march_1916().expect("weather diary");
    let places_toml = alpine_sim::roster::load_col_di_lana_1916_text().expect("places file");
    let world = World { terrain, tuning, diary, diary_toml, places_toml };
    let w = &world;
    let _ = &w.diary_toml;

    let mut results: Vec<(&str, &str, Outcome)> = Vec::new();
    let mut run = |n: &'static str, title: &'static str, f: &dyn Fn() -> Outcome| {
        let t = Instant::now();
        let r = f();
        eprintln!("  test {n} done in {:.0} s", t.elapsed().as_secs_f64());
        results.push((n, title, r));
    };
    run("12", "Calendar and dawn", &|| test_dawn(w));
    run("13", "Moon", &|| test_moon(w));
    run("17", "Twilight is gradual", &|| test_twilight(w));
    run("18", "Skip stops at dawn", &|| test_skip_night(w));
    run("19", "Skip stops on events", &|| test_skip_events(w));
    run("20", "Skipping changes nothing", &|| test_skip_same(w));

    let failed = results.iter().filter(|r| r.2.is_err()).count();
    println!();
    println!("Alpine Front: step-2 tests (locked, docs/scenario-tests.md; 14 to 16 and 21 withdrawn, decision 56)");
    println!("=============================================================================================");
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
