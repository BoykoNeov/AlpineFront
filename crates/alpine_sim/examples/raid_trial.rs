//! A raid played headless, for checking the raid by hand while it is built and tuned (decision 56:
//! numbers are checked by hand, not by lasting tests). Prints what happened, minute by minute.
//!
//! Usage:
//!   cargo run --release -p alpine_sim --example raid_trial -- [--seed N] [--at "1916-03-10 02:00"]
//!       [--post NAME] [--instruction flare|shout|quiet] [--seeds K] [--week]
//!
//! By default the game is set at the given moment of a night (Thursday's heavy snow) with every
//! squad on its usual duty, and the raiders set out at once against the post named (the officer's
//! choice is skipped). With `--week` the whole week is played from Monday's first light and the
//! officer picks the night himself (slow: a week is millions of updates). `--seeds K` repeats for K
//! seeds and sums up.

use alpine_sim::raid::{Instruction, RaidFile, Stage};
use alpine_sim::roster;
use alpine_sim::scenario;
use alpine_sim::sim::{Event, Order, Sim, make_pool};
use alpine_sim::sky::DateTime;
use alpine_sim::terrain::Terrain;
use alpine_sim::tuning::Tuning;
use alpine_sim::weather::Weather;
use std::sync::Arc;
use std::time::Instant;

struct Args {
    seed: u64,
    seeds: u64,
    at: String,
    post: String,
    instruction: Option<Instruction>,
    week: bool,
    quiet: bool,
}

fn args() -> Args {
    let mut a = Args { seed: 1916, seeds: 1, at: "1916-03-10 02:00".into(), post: "South post".into(), instruction: None, week: false, quiet: false };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--seed" => a.seed = it.next().unwrap().parse().unwrap(),
            "--seeds" => a.seeds = it.next().unwrap().parse().unwrap(),
            "--at" => a.at = it.next().unwrap(),
            "--post" => a.post = it.next().unwrap(),
            "--instruction" => {
                a.instruction = Some(match it.next().unwrap().as_str() {
                    "flare" => Instruction::Flare,
                    "shout" => Instruction::Shout,
                    _ => Instruction::Quiet,
                })
            }
            "--week" => a.week = true,
            "--quiet" => a.quiet = true,
            o => panic!("unknown option {o}"),
        }
    }
    a
}

fn build(seed: u64, tuning: &Tuning, terrain: &Arc<Terrain>) -> Sim {
    let (_, weather_toml) = Weather::load_march_1916().unwrap();
    let places = roster::load_col_di_lana_1916_text().unwrap();
    let supply = alpine_sim::supply::load_text().unwrap();
    let snow = alpine_sim::snow::load_text().unwrap();
    let raid = alpine_sim::raid::load_text().unwrap();
    scenario::build(scenario::STEP2, terrain.clone(), tuning.clone(), seed, make_pool(8), &weather_toml, &places, &supply, &snow, &raid).unwrap()
}

fn main() {
    let a = args();
    let (tuning, _) = Tuning::load_default().unwrap();
    let terrain = Arc::new(Terrain::load_col_di_lana(tuning.walking.steepest_walkable_deg).unwrap());
    RaidFile::from_toml(&alpine_sim::raid::load_text().unwrap()).unwrap();
    let mut totals = [0u32; 6];
    for k in 0..a.seeds {
        let seed = a.seed + k;
        let started = Instant::now();
        let mut sim = build(seed, &tuning, &terrain);
        if let Some(i) = a.instruction {
            sim.order(Order::SetInstruction { instruction: i });
        }
        if !a.week {
            // Put the game at the chosen moment: the clock jumps there and every squad takes up its
            // duty for that half of the day.
            let c = sim.clock.as_mut().unwrap();
            let target = DateTime::parse(&a.at).unwrap().to_seconds();
            let mut cal = c.calendar;
            cal.start = target;
            let weather = c.weather.clone();
            sim.set_clock(cal, weather);
            if let Some(sn) = sim.snow.as_mut() {
                // About the Tuesday fall, settled, lying where nobody walked.
                sn.loose.fill(10.0);
                sn.untrodden = 10.0;
            }
            let ours: Vec<u32> = sim.squads.iter().filter(|q| q.roster.is_some()).map(|q| q.id).collect();
            for q in ours {
                let r = sim.squads[q as usize].roster.unwrap();
                sim.order(Order::SetRoster { squad: q, day: r.day, night: r.night });
            }
            // Ten minutes for the night guards to reach their posts.
            let settle = sim.tuning.seconds_to_ticks(600.0);
            for _ in 0..settle {
                sim.step();
            }
            let post = sim.places.iter().position(|p| p.name == a.post).expect("no such post");
            sim.force_raid(post);
        }
        let dt = sim.tuning.dt() as f64;
        let start_tick = sim.tick;
        let limit = if a.week { u64::MAX } else { start_tick + sim.tuning.seconds_to_ticks(3.0 * 3600.0) };
        let mut last_minute = None;
        let mut first_seen = false;
        loop {
            sim.step();
            let now = sim.now().unwrap();
            // Who first saw a raider, and how (sight, hearing at arm's reach, or a flare's light).
            if !first_seen && !a.quiet {
                let r = sim.raid.as_ref().unwrap();
                for s in sim.soldiers.iter().filter(|s| s.side == alpine_sim::sim::Side::Austria) {
                    if let Some(&e) = s.sees.iter().find(|&&e| r.is_raider(e)) {
                        first_seen = true;
                        let en = &sim.soldiers[e as usize];
                        let d = ((s.x - en.x).powi(2) + (s.y - en.y).powi(2)).sqrt();
                        let q = &sim.squads[s.squad as usize];
                        let at = q.post.map(|(p, _)| sim.places[p].name.clone()).unwrap_or_else(|| "no post".into());
                        let lit = r.flares.iter().any(|f| ((en.x - f.x).powi(2) + (en.y - f.y).powi(2)).sqrt() <= r.tuning.flares.radius_m);
                        let moving = (en.vx * en.vx + en.vy * en.vy).sqrt();
                        println!(
                            "  {} first seen: man {} of squad {} ({at}) saw raider {e} at {d:.1} m; his sight {:.1} m, hearing {:.1} m, raider moving {moving:.2} m/s, lit by a flare: {lit}, tiredness {:.2}",
                            now.hhmm(),
                            s.id,
                            s.squad,
                            sim.spotting_range_of(s.id),
                            r.tuning.hearing.within_m * sim.tuning.tiredness.spotting_factor(s.tiredness),
                            s.tiredness
                        );
                        break;
                    }
                }
            }
            for e in sim.events.clone() {
                if let Event::Raid(n) = e
                    && !a.quiet
                {
                    println!("  {} {:?}", now.hhmm(), n);
                }
            }
            let r = sim.raid.as_ref().unwrap();
            if !a.quiet && r.out() {
                let minute = sim.tick / sim.tuning.seconds_to_ticks(60.0);
                if last_minute != Some(minute) && minute.is_multiple_of(2) {
                    last_minute = Some(minute);
                    let post = r.record.post.unwrap();
                    let pxy = (sim.places[post].x, sim.places[post].y);
                    let mut near = f32::INFINITY;
                    let mut out = 0;
                    for &i in &r.raiders {
                        let s = &sim.soldiers[i as usize];
                        if s.gone.is_none() && !s.hidden {
                            out += 1;
                            near = near.min(((s.x - pxy.0).powi(2) + (s.y - pxy.1).powi(2)).sqrt());
                        }
                    }
                    println!(
                        "  {} out {out:2}, nearest {near:5.1} m, seeing {:.1} m, alert {:?}, struggles {}, prisoners {}, shots {}, flares {}, hit ours {} theirs {}",
                        now.hhmm(),
                        sim.spotting_range,
                        r.alert[post],
                        r.struggles.len(),
                        r.prisoners.len(),
                        r.record.shots,
                        r.record.flares,
                        r.record.hit_ours.len() + r.record.killed.len(),
                        r.record.raiders_hit
                    );
                }
            }
            if matches!(r.stage, Stage::Over { .. }) && r.record.set_out.is_some() || sim.tick >= limit {
                break;
            }
            if a.week && sim.clock.as_ref().unwrap().day >= 7 {
                break;
            }
        }
        let r = sim.raid.as_ref().unwrap();
        let rec = &r.record;
        let took = |t: Option<u64>| t.map(|t| sim.time_at(t).unwrap().hhmm()).unwrap_or("-".into());
        println!(
            "seed {seed}: night {:?}, set out {}, post {}, word {:?} at {}, taken {}, killed {}, hit ours {}, freed {}, raiders hit {}, shots {}, flares {}, struggles {}, over {} ({:.0} s real, {:.0} game minutes)",
            rec.night,
            took(rec.set_out),
            rec.post.map(|p| sim.places[p].name.clone()).unwrap_or_default(),
            rec.word.map(|w| w.1),
            took(rec.word.map(|w| w.0)),
            rec.taken.len(),
            rec.killed.len(),
            rec.hit_ours.len(),
            rec.freed,
            rec.raiders_hit,
            rec.shots,
            rec.flares,
            rec.struggles,
            took(rec.over_at),
            started.elapsed().as_secs_f64(),
            (sim.tick - start_tick) as f64 * dt / 60.0
        );
        totals[0] += rec.taken.len() as u32;
        totals[1] += rec.killed.len() as u32;
        totals[2] += rec.hit_ours.len() as u32;
        totals[3] += rec.raiders_hit;
        totals[4] += rec.word.is_some() as u32;
        totals[5] += (!rec.taken.is_empty()) as u32;
    }
    if a.seeds > 1 {
        println!(
            "over {} raids: {} taken ({} raids took someone), {} killed, {} hit ours, {} raiders hit, word reached the command post in {}",
            a.seeds, totals[0], totals[5], totals[1], totals[2], totals[3], totals[4]
        );
    }
}
