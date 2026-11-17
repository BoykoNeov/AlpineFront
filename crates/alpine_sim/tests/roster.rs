//! The locked work-roster tests (docs/scenario-tests.md, tests 22 to 29): shelters, guards, shift
//! changes, tiredness, orders against the roster, and replays. Every scenario runs twice and the
//! runs must match.
//!
//! Run with: cargo test --release -p alpine_sim --test roster

use alpine_sim::replay::{Recorder, Replay, Verdict};
use alpine_sim::roster::{self, Duty, PlaceKind, Roster};
use alpine_sim::scenario;
use alpine_sim::sim::{LoggedOrder, Order, PLAYER_SIDE, Side, Sim, SquadState, make_pool};
use alpine_sim::sky::{Calendar, DateTime};
use alpine_sim::terrain::Terrain;
use alpine_sim::tuning::Tuning;
use alpine_sim::weather::Weather;
use std::sync::Arc;
use std::time::Instant;

struct World {
    terrain: Arc<Terrain>,
    tuning: Tuning,
    tuning_toml: String,
    diary: Weather,
    diary_toml: String,
    places_toml: String,
}

type Outcome = Result<String, String>;

fn at(text: &str) -> f64 {
    DateTime::parse(text).unwrap().to_seconds()
}

fn calendar_at(w: &World, start: f64) -> Calendar {
    let mut c = scenario::step2_calendar(&w.terrain, &w.tuning).unwrap();
    c.start = start;
    c
}

fn ticks(w: &World, seconds: f64) -> u64 {
    (seconds * w.tuning.time.ticks_per_second as f64).round() as u64
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

fn places(w: &World) -> Vec<roster::Place> {
    roster::places_from_toml(&w.places_toml, &w.terrain).unwrap()
}

fn place_index(w: &World, name: &str) -> usize {
    places(w).iter().position(|p| p.name == name).unwrap_or_else(|| panic!("no place called {name}"))
}

fn step2(w: &World) -> Sim {
    scenario::step2(w.terrain.clone(), w.tuning.clone(), 1916, make_pool(4), false, w.diary.clone(), &w.places_toml).unwrap()
}

fn squad_tiredness(sim: &Sim, q: u32) -> f32 {
    let m = &sim.squads[q as usize].members;
    m.iter().map(|&i| sim.soldiers[i as usize].tiredness).sum::<f32>() / m.len() as f32
}

fn eye(sim: &Sim, x: f32, y: f32) -> [f32; 3] {
    [x, y, sim.terrain.height(x, y) + sim.tuning.soldier.eye_height_m]
}

// ---- 22. Shelter hides both ways ------------------------------------------------------------------

/// A resting squad in the great cavern and a guard squad at the West post, each with an Italian 8 m
/// away in plain sight, at a given moment. Returns (whether the sheltered squad ever saw anyone,
/// whether any Italian ever saw it, whether the guard squad saw its Italian, the sight check
/// between the cavern's men and their Italian, the distance spotted at).
fn shelter_case(w: &World, when: f64) -> Result<(bool, bool, bool, bool, u32), String> {
    let mut sim = Sim::new(w.terrain.clone(), w.tuning.clone(), 7, make_pool(2));
    sim.set_clock(calendar_at(w, when), Weather::constant(0.0));
    let men = w.tuning.squad.men_per_squad as usize;
    let a = sim.add_squad(Side::Austria, &vec![(0.0, 0.0); men]);
    let b = sim.add_squad(Side::Austria, &vec![(0.0, 0.0); men]);
    let rest = Roster { day: Duty::Rest, night: Duty::Rest };
    let guard = Roster { day: Duty::Guard, night: Duty::Guard };
    sim.start_roster(places(w), &[(a, rest), (b, guard)]);
    let cavern = sim.places[place_index(w, "Great cavern")].spots[0];
    let west = sim.places[place_index(w, "West post")].spots[0];
    if sim.squads[a as usize].post.map(|p| p.0) != Some(place_index(w, "Great cavern")) || !sim.squads[a as usize].inside {
        return Err("set-up error: the resting squad is not inside the great cavern".into());
    }
    // An Italian 8 m from each squad's spot, on ground a man can stand on and in plain sight of it.
    let near = |sim: &Sim, (x, y): (f32, f32)| -> Option<(f32, f32)> {
        (0..16).map(|k| k as f32 * std::f32::consts::TAU / 16.0).map(|ang| (x + 8.0 * ang.cos(), y + 8.0 * ang.sin())).find(|&(px, py)| {
            sim.terrain.walkable_at(px, py)
                && alpine_sim::los::visible(&sim.terrain, &sim.los, eye(sim, x, y), eye(sim, px, py))
        })
    };
    let ic = near(&sim, cavern).ok_or("no open ground beside the cavern")?;
    let iw = near(&sim, west).ok_or("no open ground beside the West post")?;
    let it = sim.add_squad(Side::Italy, &[ic, iw]);
    let it_men = sim.squads[it as usize].members.clone();
    let a_men = sim.squads[a as usize].members.clone();
    let b_men = sim.squads[b as usize].members.clone();
    // The control: the cavern's men and their Italian could see each other if they stood outside.
    let open = a_men.iter().any(|&m| sim.can_see(m, it_men[0]) && sim.can_see(it_men[0], m));
    let (mut a_saw, mut a_seen, mut b_saw) = (false, false, false);
    while sim.tick < ticks(w, 120.0) {
        sim.step();
        a_saw |= a_men.iter().any(|&m| !sim.soldiers[m as usize].sees.is_empty());
        a_seen |= it_men.iter().any(|&m| sim.soldiers[m as usize].sees.iter().any(|e| a_men.contains(e)));
        b_saw |= b_men.iter().any(|&m| sim.soldiers[m as usize].sees.contains(&it_men[1]));
    }
    Ok((a_saw, a_seen, b_saw, open, sim.spotting_range.round() as u32))
}

fn test_shelter(w: &World) -> Outcome {
    let mut lines = Vec::new();
    for (label, when) in [("noon", "1916-03-08 12:00"), ("03:00 on a clear night", "1916-03-08 03:00")] {
        let (a_saw, a_seen, b_saw, open, range) = twice(|| shelter_case(w, at(when)))??;
        if !open {
            return Err(format!("set-up error at {label}: the Italian beside the cavern is not in plain sight of its men"));
        }
        if a_saw {
            return Err(format!("at {label} the squad inside the great cavern saw an enemy"));
        }
        if a_seen {
            return Err(format!("at {label} an Italian saw the squad inside the great cavern"));
        }
        if !b_saw {
            return Err(format!("at {label} the guard squad at the West post did not see the Italian 8 m away (spotting distance {range} m)"));
        }
        lines.push(format!("at {label} (spotting distance {range} m)"));
    }
    Ok(format!(
        "{}: a squad resting in the great cavern, with an Italian 8 m away who would be in plain sight outside, saw nobody and was seen by nobody for 2 game minutes, while a guard squad at the West post saw its Italian at the same distance",
        lines.join(" and ")
    ))
}

// ---- 23. Guards watch -----------------------------------------------------------------------------

fn test_guards(w: &World) -> Outcome {
    let r = twice(|| -> Result<(String, u32, Option<u32>), String> {
        let mut sim = Sim::new(w.terrain.clone(), w.tuning.clone(), 7, make_pool(2));
        sim.set_clock(calendar_at(w, at("1916-03-08 12:00")), Weather::constant(0.0));
        let men = w.tuning.squad.men_per_squad as usize;
        let guard = Roster { day: Duty::Guard, night: Duty::Guard };
        let ps = places(w);
        let posts = ps.iter().filter(|p| p.kind == PlaceKind::Post).count();
        let mut rosters = Vec::new();
        for _ in 0..posts {
            rosters.push((sim.add_squad(Side::Austria, &vec![(0.0, 0.0); men]), guard));
        }
        sim.start_roster(ps, &rosters);
        // A point on the southern slope, 1,450 to 1,490 m from a man at the South or South-west
        // post, that a man can stand on and that he can see.
        let mut pick = None;
        'search: for name in ["South post", "South-west post"] {
            let p = place_index(w, name);
            let q = sim.squads.iter().find(|q| q.post.map(|x| x.0) == Some(p)).unwrap();
            for &m in &q.members {
                let s = &sim.soldiers[m as usize];
                for k in 0..=40 {
                    let ang = std::f32::consts::PI * (1.1 + 0.8 * k as f32 / 40.0);
                    for d in [1480.0f32, 1470.0, 1460.0, 1450.0] {
                        let (x, y) = (s.x + d * ang.cos(), s.y + d * ang.sin());
                        if sim.terrain.in_bounds(x, y)
                            && sim.terrain.walkable_at(x, y)
                            && alpine_sim::los::visible(&sim.terrain, &sim.los, eye(&sim, s.x, s.y), eye(&sim, x, y))
                        {
                            pick = Some((name, m, (x, y), d));
                            break 'search;
                        }
                    }
                }
            }
        }
        let (name, m, target, d) = pick.ok_or("no point in plain sight 1,450 to 1,490 m south of the southern posts")?;
        let it = sim.add_squad(Side::Italy, &[target]);
        let it_man = sim.squads[it as usize].members[0];
        let mut seen_at = None;
        while sim.tick < ticks(w, 5.0) && seen_at.is_none() {
            sim.step();
            if sim.soldiers[m as usize].sees.contains(&it_man) {
                seen_at = Some(sim.tick as u32);
            }
        }
        Ok((name.to_string(), d as u32, seen_at))
    })??;
    match r {
        (name, d, Some(t)) => Ok(format!(
            "at noon on a clear day a guard at the {name} spotted an Italian in plain view on the southern slope {d} m away after {:.1} game seconds",
            t as f64 / w.tuning.time.ticks_per_second as f64
        )),
        (name, d, None) => Err(format!("a guard at the {name} did not spot an Italian in plain view {d} m away in 5 game seconds")),
    }
}

// ---- 24, 25, 27 and 28: a day and a night on the roster ---------------------------------------------

/// What the day's run saw, compared between the two runs.
#[derive(Debug, PartialEq)]
struct DayRun {
    /// At dusk and at the next dawn: (minutes after the change by which every squad whose duty
    /// changed had set off, minutes by which every rostered squad stood at its place), or an error.
    dusk: Result<(u32, u32), String>,
    dawn: Result<(u32, u32), String>,
    /// Tiredness (in thousandths) of squad 2 (guard by day) at dusk and at the next dawn, and of
    /// squad 14 (guard around the clock) at the next dawn.
    tired_day_guard_dusk: u32,
    tired_day_guard_dawn: u32,
    tired_all_day_dawn: u32,
    /// Squad 0, sent away and then back to duty: (stayed where sent until "Back to duty", back at a
    /// post within 15 minutes after it).
    back_to_duty: (bool, bool),
    /// Squad 1, sent away and left: (stayed where sent until dusk, then took up its night duty).
    sent_until_dusk: (bool, bool),
    /// At the next dawn, after "Carry on" with no change: (rosters unchanged, every squad back at
    /// the very spot it held the morning before).
    same_as_yesterday: (bool, bool),
    dusk_time: String,
}

fn day_run(w: &World) -> DayRun {
    let mut sim = step2(w);
    let minute = ticks(w, 60.0);
    let ours: Vec<u32> = sim.squads.iter().filter(|q| q.side == PLAYER_SIDE).map(|q| q.id).collect();
    // Squad 14 guards around the clock; squads 0 and 1 are sent away after an hour, and squad 0
    // called back to duty after three.
    sim.order(Order::SetRoster { squad: 14, day: Duty::Guard, night: Duty::Guard });
    // Both are sent to open ground on the summit, 30 to 40 m from their posts (the West post and
    // the South-west post), off the line of trench.
    sim.schedule(LoggedOrder { tick: 60 * minute, order: Order::Move { squad: 0, x: -20.0, y: -5.0 } });
    sim.schedule(LoggedOrder { tick: 60 * minute + 1, order: Order::Move { squad: 1, x: -5.0, y: -20.0 } });
    sim.schedule(LoggedOrder { tick: 180 * minute, order: Order::BackToDuty { squad: 0 } });

    // Let the first order take effect, then note the morning's spots.
    for _ in 0..(20 * minute) {
        sim.step();
    }
    let morning: Vec<_> = ours.iter().map(|&q| sim.squads[q as usize].post).collect();
    let rosters: Vec<_> = ours.iter().map(|&q| sim.squads[q as usize].roster).collect();

    let mut sent0_stayed = true;
    let mut sent1_stayed = true;
    let mut back0 = false;
    while sim.tick < 180 * minute {
        sim.step();
        if sim.tick > 120 * minute {
            sent0_stayed &= sim.squads[0].state == SquadState::Arrived && sim.squads[0].detached;
        }
    }
    while sim.tick < 195 * minute {
        sim.step();
    }
    if let Some((p, _)) = sim.squads[0].post {
        back0 = sim.places[p].kind == PlaceKind::Post && sim.squads[0].state == SquadState::Arrived && !sim.squads[0].detached;
    }

    // On to dusk.
    while !sim.clock.as_ref().unwrap().night {
        sim.step();
        if sim.tick > 120 * minute && !sim.clock.as_ref().unwrap().night {
            sent1_stayed &= sim.squads[1].state == SquadState::Arrived && sim.squads[1].detached;
        }
    }
    let dusk_time = sim.now().unwrap().hhmm();
    let tired_day_guard_dusk = (squad_tiredness(&sim, 2) * 1000.0).round() as u32;
    let dusk = watch_change(&mut sim, &ours, minute);
    let sent1_night = sim.squads[1].post.is_some_and(|(p, _)| sim.places[p].kind == PlaceKind::Shelter) && sim.squads[1].inside;

    // On to the next dawn: the orders screen would open here; "Carry on" without a change.
    while sim.clock.as_ref().unwrap().night {
        sim.step();
    }
    let dawn = watch_change(&mut sim, &ours, minute);
    let tired_day_guard_dawn = (squad_tiredness(&sim, 2) * 1000.0).round() as u32;
    let tired_all_day_dawn = (squad_tiredness(&sim, 14) * 1000.0).round() as u32;
    let rosters_now: Vec<_> = ours.iter().map(|&q| sim.squads[q as usize].roster).collect();
    let spots_now: Vec<_> = ours.iter().map(|&q| sim.squads[q as usize].post).collect();
    DayRun {
        dusk,
        dawn,
        tired_day_guard_dusk,
        tired_day_guard_dawn,
        tired_all_day_dawn,
        back_to_duty: (sent0_stayed, back0),
        sent_until_dusk: (sent1_stayed, sent1_night),
        same_as_yesterday: (rosters_now == rosters, spots_now == morning),
        dusk_time,
    }
}

/// Runs on from a dusk or dawn, just noted, for 15 game minutes and reports when every squad whose
/// place changed had set off and when every rostered squad stood at its place.
fn watch_change(sim: &mut Sim, ours: &[u32], minute: u64) -> Result<(u32, u32), String> {
    let start = sim.tick;
    let moving: Vec<u32> =
        ours.iter().copied().filter(|&q| matches!(sim.squads[q as usize].state, SquadState::Planning { .. } | SquadState::Moving)).collect();
    if moving.is_empty() {
        return Err("no squad set off".into());
    }
    let (mut set_off, mut all_there) = (None, None);
    while sim.tick < start + 15 * minute + 1 {
        sim.step();
        let elapsed = ((sim.tick - start) as f64 / minute as f64).ceil() as u32;
        if set_off.is_none() && moving.iter().all(|&q| !matches!(sim.squads[q as usize].state, SquadState::Planning { .. })) {
            set_off = Some(elapsed);
        }
        let there = ours.iter().all(|&q| {
            let s = &sim.squads[q as usize];
            let Some((p, _)) = s.post else { return false };
            matches!(s.state, SquadState::Arrived | SquadState::Idle) && (sim.places[p].kind == PlaceKind::Post || s.inside)
        });
        if there && all_there.is_none() {
            all_there = Some(elapsed);
        }
    }
    match (set_off, all_there) {
        (Some(a), Some(b)) => Ok((a, b)),
        (None, _) => Err("some squads had not set off after 15 minutes".into()),
        (_, None) => {
            let late: Vec<u32> = ours
                .iter()
                .copied()
                .filter(|&q| {
                    let s = &sim.squads[q as usize];
                    !(matches!(s.state, SquadState::Arrived | SquadState::Idle) && s.post.is_some())
                })
                .collect();
            Err(format!("squads {late:?} were not at their places 15 minutes later"))
        }
    }
}

fn test_shifts(r: &DayRun) -> Outcome {
    let (dusk_off, dusk_there) = r.dusk.clone().map_err(|e| format!("at dusk: {e}"))?;
    let (dawn_off, dawn_there) = r.dawn.clone().map_err(|e| format!("at dawn: {e}"))?;
    if dusk_off > 1 || dawn_off > 1 {
        return Err(format!("squads set off {dusk_off} minutes after dusk and {dawn_off} after dawn; the limit is 1"));
    }
    if dusk_there > 15 || dawn_there > 15 {
        return Err(format!("the last squad reached its place {dusk_there} minutes after dusk and {dawn_there} after dawn; the limit is 15"));
    }
    Ok(format!(
        "at dusk on Monday ({}) every squad whose duty changed set off within {dusk_off} game minute and all stood at their places after {dusk_there} minutes; at Tuesday's first light, within {dawn_off} minute and after {dawn_there} minutes",
        r.dusk_time
    ))
}

fn test_tiredness(w: &World, r: &DayRun) -> Outcome {
    let word = |t: u32| roster::tiredness_word(t as f32 / 1000.0);
    let pct = |t: u32| format!("{:.0}%", t as f64 / 10.0);
    let a = word(r.tired_day_guard_dusk);
    let b = word(r.tired_all_day_dawn);
    let c = word(r.tired_day_guard_dawn);
    let msg = format!(
        "a squad guarding from dawn to dusk was {a} at dusk ({} of the way to exhausted); one guarding around the clock was {b} at Tuesday's dawn ({}); the dawn-to-dusk guards, after the night in the great cavern, were {c} ({}). Tiredness: {} h awake to exhausted, {} h asleep to fresh",
        pct(r.tired_day_guard_dusk),
        pct(r.tired_all_day_dawn),
        pct(r.tired_day_guard_dawn),
        w.tuning.tiredness.hours_awake_to_exhausted,
        w.tuning.tiredness.hours_asleep_to_fresh
    );
    if (a, b, c) == ("tired", "exhausted", "fresh") { Ok(msg) } else { Err(msg + "; expected tired, exhausted, fresh") }
}

fn test_orders(r: &DayRun) -> Outcome {
    let (stayed0, back0) = r.back_to_duty;
    let (stayed1, night1) = r.sent_until_dusk;
    if !stayed0 {
        return Err("squad 0, sent away, did not stay where it was sent until called back".into());
    }
    if !back0 {
        return Err("squad 0 was not back at a lookout post 15 minutes after \"Back to duty\"".into());
    }
    if !stayed1 {
        return Err("squad 1, sent away, did not stay where it was sent until dusk".into());
    }
    if !night1 {
        return Err("squad 1 did not take up its night duty (rest in a shelter) after dusk".into());
    }
    Ok("squad 0, a day guard sent 30 m away across the summit, left its post and stood where sent until \"Back to duty\", then was at a lookout post again within 15 minutes; squad 1, sent away and left there, stayed until dusk and then went to rest in a shelter as its roster said".into())
}

fn test_same(r: &DayRun) -> Outcome {
    match r.same_as_yesterday {
        (true, true) => Ok("after \"Carry on\" at Tuesday's first light with no change, every squad had the same duties as on Monday and stood at the very post or shelter spot it held on Monday morning".into()),
        (false, _) => Err("the roster changed overnight without an order".into()),
        (true, false) => Err("the duties were the same but some squads ended up at a different post or shelter than on Monday".into()),
    }
}

// ---- 26. Tired men see and walk less --------------------------------------------------------------

/// A direction from the summit in which a man at each of the given distances would stand on
/// walkable ground in plain sight.
fn open_direction(sim: &Sim, distances: &[f32]) -> Option<(f32, f32)> {
    let t = &sim.terrain;
    (0..64).map(|k| k as f32 * std::f32::consts::TAU / 64.0).map(|a| (a.cos(), a.sin())).find(|&(dx, dy)| {
        distances.iter().all(|&d| {
            let (x, y) = (dx * d, dy * d);
            t.walkable_at(x, y) && alpine_sim::los::visible(t, &sim.los, eye(sim, 0.0, 0.0), eye(sim, x, y))
        })
    })
}

/// A fresh and an exhausted man on the summit; Italians at 48% and 52% of the light's spotting
/// distance in an open direction. Returns (the distance, what each saw in 5 game seconds, as
/// [fresh near, fresh far, exhausted near, exhausted far]).
fn sight_case(w: &World, when: f64) -> Result<(u32, [bool; 4]), String> {
    let mut sim = Sim::new(w.terrain.clone(), w.tuning.clone(), 7, make_pool(2));
    sim.set_clock(calendar_at(w, when), Weather::constant(0.0));
    let range = sim.spotting_range;
    let dir = open_direction(&sim, &[range * 0.48, range * 0.52]).ok_or("no open direction from the summit")?;
    let fresh = sim.add_squad(Side::Austria, &[(0.0, 0.0)]);
    let tired = sim.add_squad(Side::Austria, &[(0.0, 0.0)]);
    let (f, t) = (sim.squads[fresh as usize].members[0], sim.squads[tired as usize].members[0]);
    sim.soldiers[t as usize].tiredness = 1.0;
    let near = (dir.0 * range * 0.48, dir.1 * range * 0.48);
    let far = (dir.0 * range * 0.52, dir.1 * range * 0.52);
    let it = sim.add_squad(Side::Italy, &[near, far]);
    let (n, fa) = (sim.squads[it as usize].members[0], sim.squads[it as usize].members[1]);
    let mut seen = [false; 4];
    while sim.tick < ticks(w, 5.0) {
        sim.step();
        let s = |i: u32, e: u32| sim.soldiers[i as usize].sees.contains(&e);
        let now = [s(f, n), s(f, fa), s(t, n), s(t, fa)];
        for k in 0..4 {
            seen[k] |= now[k];
        }
    }
    Ok((range.round() as u32, seen))
}

/// Game seconds for a squad, fresh or exhausted, to climb from the valley floor to the summit
/// (test 8's climb).
fn climb(w: &World, tiredness: f32) -> Option<u64> {
    let mut sim = Sim::new(w.terrain.clone(), w.tuning.clone(), 3, make_pool(2));
    let men = w.tuning.squad.men_per_squad as usize;
    let start = sim.formation_around(-1200.0, -1600.0, men);
    let q = sim.add_squad(Side::Austria, &start);
    for &m in &sim.squads[q as usize].members.clone() {
        sim.soldiers[m as usize].tiredness = tiredness;
    }
    sim.order(Order::Move { squad: q, x: 0.0, y: 0.0 });
    let limit = ticks(w, 10.0 * 3600.0);
    while sim.tick < limit {
        sim.step();
        if sim.squads[q as usize].state == SquadState::Arrived {
            return Some(sim.tick / w.tuning.time.ticks_per_second as u64);
        }
    }
    None
}

fn test_tired_effects(w: &World) -> Outcome {
    let mut lines = Vec::new();
    for (label, when) in [("noon", "1916-03-08 12:00"), ("03:00 on a clear night", "1916-03-08 03:00")] {
        let (range, seen) = twice(|| sight_case(w, at(when)))??;
        if seen != [true, true, true, false] {
            return Err(format!(
                "at {label} (spotting distance {range} m), with Italians at 48% and 52% of it: the fresh man saw near {}, far {}; the exhausted man saw near {}, far {}. Expected the fresh man to see both and the exhausted man only the near one",
                seen[0], seen[1], seen[2], seen[3]
            ));
        }
        lines.push(format!("at {label} an exhausted man spotted an Italian at 48% of the {range} m spotting distance but not one at 52%, while a fresh man saw both"));
    }
    let (fresh, tired) = twice(|| std::thread::scope(|s| {
        let a = s.spawn(|| climb(w, 0.0));
        let b = s.spawn(|| climb(w, 1.0));
        (a.join().unwrap(), b.join().unwrap())
    }))?;
    let (fresh, tired) = (fresh.ok_or("the fresh squad did not reach the summit")?, tired.ok_or("the exhausted squad did not reach the summit")?);
    let ratio = tired as f64 / fresh as f64;
    let hm = |s: u64| format!("{} h {:02} min", s / 3600, (s / 60) % 60);
    let msg = format!(
        "{}; on the long climb to the summit a fresh squad took {} and an exhausted one {}, {ratio:.2} times as long (expected about 1.4)",
        lines.join("; "),
        hm(fresh),
        hm(tired)
    );
    if (1.3..=1.5).contains(&ratio) { Ok(msg) } else { Err(msg) }
}

// ---- 29. Replays and old tests --------------------------------------------------------------------

fn test_replays(w: &World) -> Outcome {
    let dir = std::env::temp_dir().join(format!("alpine_roster_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("roster.ron");
    let minute = ticks(w, 60.0);
    let (orders, sums) = {
        let mut sim = step2(w);
        let mut rec = Recorder::new(scenario::STEP2, 1916, w.terrain.fingerprint(), &w.tuning_toml, &w.diary_toml, &w.places_toml);
        let plan = [
            (5 * minute, Order::SetRoster { squad: 12, day: Duty::Guard, night: Duty::Rest }),
            (30 * minute, Order::Move { squad: 2, x: -150.0, y: -60.0 }),
            (60 * minute, Order::SetRoster { squad: 3, day: Duty::Rest, night: Duty::Guard }),
            (90 * minute, Order::BackToDuty { squad: 2 }),
        ];
        for (tick, order) in plan {
            sim.schedule(LoggedOrder { tick, order });
        }
        while sim.tick < 120 * minute {
            sim.step();
            rec.after_step(&sim);
        }
        let replay = rec.finish(&sim);
        replay.save(&path)?;
        (replay.orders.len(), replay.checksums.len())
    };
    let replay = Replay::load(&path)?;
    let verdict = twice(|| replay.verify(w.terrain.clone(), make_pool(4)).map(|v| format!("{v:?}")))??;
    let _ = std::fs::remove_dir_all(&dir);
    if verdict != format!("{:?}", Verdict::Matches { ticks: replay.ticks }) {
        return Err(format!("the saved step-2 replay with roster orders did not match: {verdict}"));
    }
    // A step-2 replay from before the roster is refused with a clear message.
    let mut old = replay.clone();
    old.format = 2;
    let old_path = std::env::temp_dir().join(format!("alpine_roster_old_{}.ron", std::process::id()));
    old.save(&old_path)?;
    let refused = Replay::load(&old_path).err();
    let _ = std::fs::remove_file(&old_path);
    let Some(why) = refused else { return Err("a step-2 replay saved before the roster was not refused".into()) };
    if !why.contains("roster") {
        return Err(format!("the refusal of an old step-2 replay does not say why: {why}"));
    }
    Ok(format!(
        "a 2-hour step-2 game with {orders} roster orders (setting duties, sending a squad away, calling it back) was saved, loaded and played back matching at all {sums} checks, twice; a step-2 replay from before the roster is refused, saying so. Tests 1 to 11 are run by the step-1 suite and 12 to 21 by the step-2 suite"
    ))
}

fn main() {
    let started = Instant::now();
    let (tuning, tuning_toml) = Tuning::load_default().expect("tuning file");
    let terrain = Arc::new(Terrain::load_col_di_lana(tuning.walking.steepest_walkable_deg).expect("terrain"));
    let (diary, diary_toml) = Weather::load_march_1916().expect("weather diary");
    let places_toml = roster::load_col_di_lana_1916_text().expect("places file");
    let world = World { terrain, tuning, tuning_toml, diary, diary_toml, places_toml };
    let w = &world;

    let mut results: Vec<(&str, &str, Outcome)> = Vec::new();
    let mut run = |n: &'static str, title: &'static str, f: &dyn Fn() -> Outcome| {
        let t = Instant::now();
        let r = f();
        eprintln!("  test {n} done in {:.0} s", t.elapsed().as_secs_f64());
        results.push((n, title, r));
    };
    run("22", "Shelter hides both ways", &|| test_shelter(w));
    run("23", "Guards watch", &|| test_guards(w));
    let t = Instant::now();
    let day = twice(|| day_run(w));
    eprintln!("  the day on the roster (tests 24, 25, 27, 28) done in {:.0} s", t.elapsed().as_secs_f64());
    let with_day = |f: &dyn Fn(&DayRun) -> Outcome| match &day {
        Ok(r) => f(r),
        Err(e) => Err(e.clone()),
    };
    run("24", "Shifts change themselves", &|| with_day(&test_shifts));
    run("25", "Tiredness builds and recovers", &|| with_day(&|r| test_tiredness(w, r)));
    run("26", "Tired men see and walk less", &|| test_tired_effects(w));
    run("27", "Orders beat the roster", &|| with_day(&test_orders));
    run("28", "Same as yesterday", &|| with_day(&test_same));
    run("29", "Replays and old tests", &|| test_replays(w));
    results.sort_by_key(|r| r.0);

    let failed = results.iter().filter(|r| r.2.is_err()).count();
    println!();
    println!("Alpine Front: work-roster tests (locked, docs/scenario-tests.md)");
    println!("================================================================");
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
