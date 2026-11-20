//! The tests for the night raid (docs/scenario-tests.md, 45 to 49), drafted by Claude while building
//! decision 75 and awaiting the director's approval. They test the lasting rules, not this week's
//! numbers (decision 56): that every raid switch, turned off, gives the game without its rule
//! (decision 65); that a prisoner who reaches the Italian trench is lost to the company; that the
//! player learns nothing of the raid until word reaches the command post; that equally tired men of
//! both sides see each other alike; and that a game with a raid replays exactly. Every scenario runs
//! twice and the runs must match.
//!
//! Run with: cargo test --release -p alpine_sim --test raid

use alpine_sim::raid::{Instruction, RaidFile, Stage};
use alpine_sim::replay::{Recorder, Replay, Verdict};
use alpine_sim::roster;
use alpine_sim::scenario;
use alpine_sim::sim::{Event, Order, Side, Sim, SquadState, make_pool};
use alpine_sim::sky::DateTime;
use alpine_sim::supply::Lost;
use alpine_sim::terrain::Terrain;
use alpine_sim::tuning::Tuning;
use alpine_sim::weather::Weather;
use std::sync::Arc;
use std::time::Instant;

struct World {
    terrain: Arc<Terrain>,
    tuning: Tuning,
    tuning_toml: String,
    weather_toml: String,
    places_toml: String,
    supply_toml: String,
    snow_toml: String,
    raid_toml: String,
}

type Outcome = Result<String, String>;

/// Thursday night's heavy snow (sight about 1 m), and the clear night before it.
const SNOWY: &str = "1916-03-10 02:00";
const CLEAR: &str = "1916-03-09 02:30";
/// The post the raiders go for in these tests.
const POST: &str = "South post";

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

fn ticks(w: &World, seconds: f32) -> u64 {
    w.tuning.seconds_to_ticks(seconds)
}

/// The step-2 game as played, with the raid file (as changed by `tune`) in force unless `raid` is
/// false, set at the moment `at` of a night: the clock jumps there, 10 cm of fresh snow lies where
/// nobody walked, every squad takes up its night duty, and ten minutes pass for the guards to reach
/// their posts. The raid has not started.
fn night_game(w: &World, at: &str, raid: bool, instruction: Option<Instruction>, tune: impl Fn(&mut RaidFile)) -> Sim {
    let mut sim = scenario::build(
        scenario::STEP2,
        w.terrain.clone(),
        w.tuning.clone(),
        1916,
        make_pool(4),
        &w.weather_toml,
        &w.places_toml,
        &w.supply_toml,
        &w.snow_toml,
        "",
    )
    .unwrap();
    if raid {
        let mut file = RaidFile::from_toml(&w.raid_toml).unwrap();
        tune(&mut file);
        sim.start_raid(file).unwrap();
    }
    if let Some(instruction) = instruction {
        sim.order(Order::SetInstruction { instruction });
    }
    let c = sim.clock.as_ref().unwrap();
    let mut cal = c.calendar;
    cal.start = DateTime::parse(at).unwrap().to_seconds();
    let weather = c.weather.clone();
    sim.set_clock(cal, weather);
    if let Some(sn) = sim.snow.as_mut() {
        sn.loose.fill(10.0);
        sn.untrodden = 10.0;
    }
    let ours: Vec<u32> = sim.squads.iter().filter(|q| q.roster.is_some()).map(|q| q.id).collect();
    for q in ours {
        let r = sim.squads[q as usize].roster.unwrap();
        sim.order(Order::SetRoster { squad: q, day: r.day, night: r.night });
    }
    for _ in 0..ticks(w, 600.0) {
        sim.step();
    }
    sim
}

fn post_of(sim: &Sim) -> usize {
    sim.places.iter().position(|p| p.name == POST).expect("the South post")
}

/// Sends the raiders out against the South post at once and plays until the raid is over (at most
/// an hour), calling `each` after every update.
fn play_raid(w: &World, sim: &mut Sim, mut each: impl FnMut(&Sim)) {
    let post = post_of(sim);
    sim.force_raid(post);
    let limit = sim.tick + ticks(w, 3600.0);
    while sim.tick < limit {
        sim.step();
        each(sim);
        if sim.raid.as_ref().is_some_and(|r| matches!(r.stage, Stage::Over { .. })) {
            break;
        }
    }
}

fn sums(sim: &mut Sim, n: u64) -> Vec<u64> {
    (0..n)
        .filter_map(|k| {
            sim.step();
            (k % 10 == 0).then(|| sim.checksum())
        })
        .collect()
}

fn ok_if(cond: bool, why: impl Into<String>) -> Result<(), String> {
    if cond { Ok(()) } else { Err(why.into()) }
}

// ---- 45. Every switch, turned off, gives the game without its rule ------------------------------

fn test_switches(w: &World) -> Outcome {
    let checks = twice(|| -> Vec<(&'static str, Result<(), String>)> {
        let mut out: Vec<(&'static str, Result<(), String>)> = Vec::new();

        // The raid off: the game with no raid file at all, checksum for checksum.
        let mut off = night_game(w, SNOWY, true, None, |f| f.switches.raid = false);
        let mut none = night_game(w, SNOWY, false, None, |_| {});
        off.force_raid(post_of(&off));
        let same = sums(&mut off, ticks(w, 1200.0)) == sums(&mut none, ticks(w, 1200.0));
        out.push(("raid off plays exactly as no raid file", ok_if(same, "the games differed")));

        // The officer's choice off: the raid comes on the night and at the time the file sets.
        let mut sim = night_game(w, "1916-03-09 18:00", true, None, |f| {
            f.switches.officer_picks_night = false;
            f.officer.fixed_night = 0;
            f.officer.fixed_time = "02:30".into();
        });
        let limit = sim.tick + ticks(w, 10.0 * 3600.0);
        while sim.tick < limit && !sim.raid.as_ref().unwrap().out() {
            sim.step();
        }
        let r = sim.raid.as_ref().unwrap();
        let when = r.record.set_out.map(|_| sim.now().unwrap().hhmm());
        out.push((
            "officer's choice off: the raid comes on the night and at the time set",
            ok_if(r.record.night == Some(0) && when.as_deref() == Some("02:30"), format!("night {:?}, set out {when:?}", r.record.night)),
        ));

        // Each of the others in one raid on the snowy night, with that switch off.
        let raid_with = |tune: &dyn Fn(&mut RaidFile), each: &mut dyn FnMut(&Sim)| -> Sim {
            let mut sim = night_game(w, SNOWY, true, None, tune);
            play_raid(w, &mut sim, each);
            sim
        };

        // Italians tire off: no Italian tires (with it on, some do in the same raid).
        let italians_tired = |sim: &Sim| sim.soldiers.iter().any(|s| s.side == Side::Italy && s.tiredness > 0.0);
        let on = raid_with(&|_| {}, &mut |_| {});
        let off = raid_with(&|f| f.switches.italians_tire = false, &mut |_| {});
        out.push((
            "Italians tiring off: no Italian tires",
            ok_if(italians_tired(&on) && !italians_tired(&off), "an Italian tired with the rule off, or none tired with it on"),
        ));

        // Whispered orders off: every raider hears the order to advance as he sets out.
        let mut all_heard = None;
        raid_with(&|f| f.switches.whispered_orders = false, &mut |sim| {
            let r = sim.raid.as_ref().unwrap();
            if all_heard.is_none() && r.out() {
                all_heard = Some(r.raiders.iter().all(|&i| r.heard[i as usize].is_some()));
            }
        });
        out.push(("whispered orders off: every raider hears every order at once", ok_if(all_heard == Some(true), "some raiders had not heard")));

        // Shouts muffled off: a shout carries as far in heavy snow as in clear air.
        let mut file = RaidFile::from_toml(&w.raid_toml).unwrap();
        file.switches.snow_muffles_shouts = false;
        out.push((
            "muffled shouts off: a shout carries as far in any weather",
            ok_if(file.shout_m(10.0) == file.shout_m(0.0), format!("{} m in heavy snow against {} m", file.shout_m(10.0), file.shout_m(0.0))),
        ));

        // Hearing off: until the first flare, nobody sees a raider farther than his eyes reach.
        let mut beyond = None;
        let mut seen_before_flare = 0u32;
        raid_with(&|f| f.switches.hearing = false, &mut |sim| {
            let r = sim.raid.as_ref().unwrap();
            if !r.flares.is_empty() || r.record.flares > 0 || beyond.is_some() {
                return;
            }
            for s in sim.soldiers.iter().filter(|s| s.side == Side::Austria) {
                let range = sim.spotting_range_of(s.id);
                for &e in s.sees.iter().filter(|&&e| r.is_raider(e)) {
                    seen_before_flare += 1;
                    let en = &sim.soldiers[e as usize];
                    let d = ((s.x - en.x).powi(2) + (s.y - en.y).powi(2)).sqrt();
                    if d > range + 0.01 {
                        beyond = Some(format!("a man saw a raider {d:.1} m away with sight {range:.1} m"));
                    }
                }
            }
        });
        let heard = match beyond {
            Some(e) => Err(e),
            None if seen_before_flare == 0 => Err("no raider was seen before the first flare, so nothing was checked".into()),
            None => Ok(()),
        };
        out.push(("hearing off: nobody notices a man beyond his sight", heard));

        // Flares off: no flare goes up.
        let sim = raid_with(&|f| f.switches.flares = false, &mut |_| {});
        let r = &sim.raid.as_ref().unwrap().record;
        out.push(("flares off: no flare goes up", ok_if(r.flares == 0, format!("{} flares", r.flares))));

        // Shooting off: nobody fires and none of ours is shot (raiders can still be hurt in a
        // struggle, at most one for each struggle).
        let sim = raid_with(&|f| f.switches.shooting = false, &mut |_| {});
        let r = &sim.raid.as_ref().unwrap().record;
        out.push((
            "shooting off: nobody fires",
            ok_if(
                r.shots == 0 && r.hit_ours.is_empty() && r.raiders_hit <= r.struggles,
                format!("{} shots, {} of ours hit, {} raiders hurt in {} struggles", r.shots, r.hit_ours.len(), r.raiders_hit, r.struggles),
            ),
        ));

        // Cover off: a guard in his post is as easy to hit as in the open. Noon, so the raiders in
        // their trench can see the post.
        let chance = |cover: bool| -> Option<f32> {
            let mut sim = night_game(w, "1916-03-09 12:00", true, None, |f| f.switches.cover_at_posts = cover);
            // Until a day guard stands at a lookout post.
            let at_post = |sim: &Sim| {
                sim.squads
                    .iter()
                    .find(|q| {
                        q.side == Side::Austria
                            && q.post.is_some_and(|(p, _)| sim.places[p].kind == roster::PlaceKind::Post)
                            && matches!(q.state, SquadState::Arrived | SquadState::Idle)
                            && !q.inside
                            && !q.members.is_empty()
                    })
                    .map(|q| q.id)
            };
            let limit = sim.tick + ticks(w, 3600.0);
            while at_post(&sim).is_none() && sim.tick < limit {
                sim.step();
            }
            let squad = at_post(&sim)?;
            let r = sim.raid.as_mut().unwrap();
            let raider = r.raiders[0];
            r.task[raider as usize] = alpine_sim::raid::Task::Advance;
            let guard = sim.squads[squad as usize].members[0];
            sim.chance_to_hit(raider, guard)
        };
        let (with, without) = (chance(true), chance(false));
        let factor = RaidFile::from_toml(&w.raid_toml).unwrap().shooting.post_cover_factor;
        out.push((
            "cover off: a man in a post is as easy to hit as in the open",
            match (with, without) {
                (Some(a), Some(b)) if b > 0.0 && (a - b * factor).abs() < 1e-6 => Ok(()),
                (a, b) => Err(format!("chance {a:?} with cover, {b:?} without")),
            },
        ));

        // The standing instruction off: none is shown, and an order to change it changes nothing.
        let mut sim = night_game(w, SNOWY, true, Some(Instruction::Quiet), |f| f.switches.standing_instruction = false);
        sim.step();
        out.push((
            "standing instruction off: every post shouts first",
            ok_if(sim.instruction_shown().is_none() && sim.raid.as_ref().unwrap().instruction == Instruction::Shout, "an instruction other than shouting was in force"),
        ));

        // The command-post rule off: the player sees a raider the moment one of our men does.
        let mut first = None;
        raid_with(&|f| f.switches.word_must_reach_command_post = false, &mut |sim| {
            if first.is_some() {
                return;
            }
            let r = sim.raid.as_ref().unwrap();
            let drawn = sim.drawn_men(false);
            for s in sim.soldiers.iter().filter(|s| s.side == Side::Austria) {
                if let Some(&e) = s.sees.iter().find(|&&e| r.is_raider(e)) {
                    first = Some(drawn[e as usize]);
                }
            }
        });
        out.push(("command-post rule off: a raider shows when first seen", ok_if(first == Some(true), format!("first sighting shown: {first:?}"))));

        // The struggle off: no struggle ever starts.
        let mut any = false;
        let sim = raid_with(&|f| f.switches.struggle = false, &mut |sim| any |= !sim.raid.as_ref().unwrap().struggles.is_empty());
        let r = &sim.raid.as_ref().unwrap().record;
        out.push(("struggle off: no struggle starts", ok_if(!any && r.struggles == 0, format!("{} struggles", r.struggles))));

        // Freeing off: no prisoner goes free.
        let sim = raid_with(&|f| f.switches.escort_can_be_freed = false, &mut |_| {});
        let r = &sim.raid.as_ref().unwrap().record;
        out.push(("freeing off: no prisoner goes free", ok_if(r.freed == 0, format!("{} freed", r.freed))));

        // Close-combat ability off: every man is ordinary.
        let sim = night_game(w, SNOWY, true, None, |f| f.switches.close_combat_ability = false);
        let r = sim.raid.as_ref().unwrap();
        out.push(("close-combat ability off: every man is ordinary", ok_if(r.ability.iter().all(|&a| a == 1.0), "some men were not ordinary")));

        // Shelters waking off: the shelters sleep through the gunfire.
        let mut stood_to = false;
        let sim = raid_with(&|f| f.switches.shelters_wake = false, &mut |sim| stood_to |= sim.standing_to());
        let fired = sim.raid.as_ref().unwrap().record.shots > 0;
        out.push(("shelters waking off: the shelters sleep through gunfire", ok_if(fired && !stood_to, format!("shots fired: {fired}, shelters stood to: {stood_to}"))));
        out
    })?;
    let failed: Vec<String> = checks.iter().filter_map(|(n, r)| r.as_ref().err().map(|e| format!("{n}: {e}"))).collect();
    if failed.is_empty() {
        Ok(format!("{} switches checked: {}", checks.len(), checks.iter().map(|c| c.0).collect::<Vec<_>>().join("; ")))
    } else {
        Err(failed.join("; "))
    }
}

// ---- 46. A prisoner who reaches the trench is lost --------------------------------------------

fn test_prisoners(w: &World) -> Outcome {
    // With the struggle and shooting off a guard reached is taken at once, and nobody can stop the
    // escort: the rule under test is only what happens when he reaches the trench.
    let (taken, wrong) = twice(|| {
        let mut sim = night_game(w, SNOWY, true, None, |f| {
            f.switches.struggle = false;
            f.switches.shooting = false;
        });
        play_raid(w, &mut sim, |_| {});
        let r = sim.raid.as_ref().unwrap();
        let mut wrong = Vec::new();
        if !r.prisoners.is_empty() {
            wrong.push(format!("{} prisoners still on the way when the raid ended", r.prisoners.len()));
        }
        for &(man, squad) in &r.record.taken {
            let s = &sim.soldiers[man as usize];
            if s.gone != Some(Lost::Taken) {
                wrong.push(format!("man {man} is not marked taken"));
            }
            if sim.squads[squad as usize].members.contains(&man) {
                wrong.push(format!("man {man} is still in squad {squad}"));
            }
            if !sim.lost.iter().any(|l| l.soldier == man && l.why == Lost::Taken) {
                wrong.push(format!("man {man} is not counted among the company's losses"));
            }
        }
        (r.record.taken.len(), wrong)
    })?;
    if taken == 0 {
        return Err("no guard was taken, so the rule could not be checked".into());
    }
    if !wrong.is_empty() {
        return Err(wrong.join("; "));
    }
    Ok(format!("{taken} of our men were taken on the snowy night; each left his squad and is counted lost to the company as taken prisoner"))
}

// ---- 47. Nothing is known until word reaches the command post ---------------------------------

fn test_word(w: &World) -> Outcome {
    // A clear night with the posts told to watch quietly and send a runner: our men see the raiders
    // well before word arrives.
    let (unseen_ticks, leaks, shown_after) = twice(|| {
        let mut sim = night_game(w, CLEAR, true, Some(Instruction::Quiet), |_| {});
        let mut seen_before_word = 0u32;
        let mut leaks: Vec<String> = Vec::new();
        let mut shown_after = false;
        play_raid(w, &mut sim, |sim| {
            let r = sim.raid.as_ref().unwrap();
            let drawn = sim.drawn_men(false);
            if !r.known() {
                if sim.soldiers.iter().any(|s| s.side == Side::Austria && s.sees.iter().any(|&e| r.is_raider(e))) {
                    seen_before_word += 1;
                }
                if r.raiders.iter().any(|&i| drawn[i as usize]) && leaks.len() < 3 {
                    leaks.push("a raider was drawn".into());
                }
                for e in &sim.events {
                    let about_raiders = match e {
                        Event::Contact { enemy_squad, .. } => r.raider_squads.contains(enemy_squad),
                        Event::Raid(alpine_sim::raid::RaidNews::Word { .. } | alpine_sim::raid::RaidNews::Over) => true,
                        _ => false,
                    };
                    if about_raiders && leaks.len() < 3 {
                        leaks.push(format!("the player was told: {e:?}"));
                    }
                }
            } else if r.raiders.iter().any(|&i| drawn[i as usize]) {
                shown_after = true;
            }
        });
        (seen_before_word, leaks, shown_after)
    })?;
    if unseen_ticks == 0 {
        return Err("our men never saw a raider before word arrived, so the rule could not be checked".into());
    }
    if !leaks.is_empty() {
        return Err(leaks.join("; "));
    }
    if !shown_after {
        return Err("no raider was shown even after word reached the command post".into());
    }
    Ok(format!(
        "on a clear night with the posts watching quietly, our men saw raiders for {:.0} game seconds before the runner reached the command post; \
         in that time no raider was drawn and the player was told nothing; once word arrived the raiders our men could see were shown",
        unseen_ticks as f32 * w.tuning.dt()
    ))
}

// ---- 48. Both sides see alike -----------------------------------------------------------------

fn test_alike(w: &World) -> Outcome {
    let t = &w.terrain;
    let a = t.nearest_main_point(20.0, 62.0, 30).expect("walkable ground");
    let b = t.nearest_main_point(-160.0, 240.0, 30).expect("walkable ground");
    let (a, b) = (t.point_xy(a.0, a.1), t.point_xy(b.0, b.1));
    let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
    let dir = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let found = twice(|| -> Result<Vec<String>, String> {
        let mut said = Vec::new();
        for (at, moving, label) in [(CLEAR, false, "a clear night"), (SNOWY, false, "heavy snow"), (SNOWY, true, "heavy snow, both moving (hearing)")] {
            let mut sim = night_game(w, at, true, None, |_| {});
            let ours = sim.soldiers.iter().find(|s| s.side == Side::Austria && s.gone.is_none() && !sim.squads[s.squad as usize].inside).ok_or("no man of ours outside")?.id;
            let r = sim.raid.as_ref().unwrap();
            let theirs = sim
                .soldiers
                .iter()
                .find(|s| s.side == Side::Italy && !s.hidden && !r.is_raider(s.id) && !sim.squads[s.squad as usize].inside)
                .ok_or("no Italian outside")?
                .id;
            let mut farthest = [0.0f32; 2];
            for tired in [0.0, 0.5, 1.0] {
                let range = sim.spotting_range_of(ours).max(1.0);
                let top = (range * 1.5).max(8.0);
                for k in 1..=60 {
                    let d = top * k as f32 / 60.0;
                    for (i, (x, y)) in [(ours, a), (theirs, (a.0 + dir.0 * d, a.1 + dir.1 * d))] {
                        let s = &mut sim.soldiers[i as usize];
                        s.x = x;
                        s.y = y;
                        s.tiredness = tired;
                        let v = if moving { 0.3 } else { 0.0 };
                        (s.vx, s.vy) = (v * dir.0, v * dir.1);
                    }
                    let ra = sim.spotting_range_of(ours);
                    let rb = sim.spotting_range_of(theirs);
                    if ra != rb {
                        return Err(format!("in {label}, tiredness {tired}: our man's eyes reach {ra:.1} m, the Italian's {rb:.1} m"));
                    }
                    let us_see = sim.spot_from(ours).contains(&theirs);
                    let they_see = sim.spot_from(theirs).contains(&ours);
                    if us_see != they_see {
                        return Err(format!("in {label}, tiredness {tired}, {d:.1} m apart: our man sees: {us_see}, the Italian sees: {they_see}"));
                    }
                    if us_see {
                        farthest[0] = farthest[0].max(d);
                    }
                    if they_see {
                        farthest[1] = farthest[1].max(d);
                    }
                }
                said.push(format!("{label}, tiredness {tired}: each saw the other up to {:.1} m", farthest[0]));
                farthest = [0.0; 2];
            }
        }
        Ok(said)
    })??;
    Ok(found.join("; "))
}

// ---- 49. Replays with a raid --------------------------------------------------------------------

fn test_replay(w: &World) -> Outcome {
    // The raid file as recorded, with the raid set for the first night just after midnight, so a
    // replay from Monday's first light reaches it.
    let raid_toml = w
        .raid_toml
        .replace("officer_picks_night = true", "officer_picks_night = false")
        .replace("fixed_night = 3", "fixed_night = 0")
        .replace("fixed_time = \"02:30\"", "fixed_time = \"00:30\"");
    if raid_toml == w.raid_toml {
        return Err("could not set the raid's night in the raid file".into());
    }
    let dir = std::env::temp_dir().join(format!("alpine_raid_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("raid.ron");
    let (shots, taken) = {
        let mut sim = scenario::build(
            scenario::STEP2,
            w.terrain.clone(),
            w.tuning.clone(),
            1916,
            make_pool(8),
            &w.weather_toml,
            &w.places_toml,
            &w.supply_toml,
            &w.snow_toml,
            &raid_toml,
        )?;
        let mut rec = Recorder::new(scenario::STEP2, 1916, w.terrain.fingerprint(), &w.tuning_toml, &w.weather_toml, &w.places_toml)
            .with_supply(&w.supply_toml)
            .with_snow(&w.snow_toml)
            .with_raid(&raid_toml);
        sim.order(Order::SetInstruction { instruction: Instruction::Flare });
        let limit = ticks(w, 22.0 * 3600.0);
        while sim.tick < limit {
            sim.step();
            rec.after_step(&sim);
            if sim.raid.as_ref().is_some_and(|r| matches!(r.stage, Stage::Over { .. })) {
                // A minute more, then stop.
                for _ in 0..ticks(w, 60.0) {
                    sim.step();
                    rec.after_step(&sim);
                }
                break;
            }
        }
        let r = &sim.raid.as_ref().ok_or("no raid in the game")?.record;
        if r.set_out.is_none() {
            return Err("the raid never set out in the recorded game".into());
        }
        rec.finish(&sim).save(&path)?;
        (r.shots, r.taken.len())
    };
    let replay = Replay::load(&path)?;
    let verdict = twice(|| replay.verify(w.terrain.clone(), make_pool(4)).map(|v| format!("{v:?}")))??;
    let _ = std::fs::remove_dir_all(&dir);
    if verdict != format!("{:?}", Verdict::Matches { ticks: replay.ticks }) {
        return Err(format!("the saved replay with a raid did not match: {verdict}"));
    }
    Ok(format!(
        "a game from Monday's first light through a raid just after midnight ({shots} shots, {taken} of ours taken, the standing instruction changed once) was saved, loaded and played back twice, matching exactly at every check over {:.1} game hours",
        replay.ticks as f32 * w.tuning.dt() / 3600.0
    ))
}

fn main() {
    let started = Instant::now();
    let (tuning, tuning_toml) = Tuning::load_default().expect("tuning file");
    let terrain = Arc::new(Terrain::load_col_di_lana(tuning.walking.steepest_walkable_deg).expect("terrain"));
    let (_, weather_toml) = Weather::load_march_1916().expect("weather diary");
    let world = World {
        terrain,
        tuning,
        tuning_toml,
        weather_toml,
        places_toml: roster::load_col_di_lana_1916_text().expect("places file"),
        supply_toml: alpine_sim::supply::load_text().expect("supply file"),
        snow_toml: alpine_sim::snow::load_text().expect("snow file"),
        raid_toml: alpine_sim::raid::load_text().expect("raid file"),
    };
    let w = &world;
    let only: Vec<String> = std::env::args().skip(1).filter(|a| a.chars().all(|c| c.is_ascii_digit())).collect();

    let mut results: Vec<(&str, &str, Outcome)> = Vec::new();
    let mut run = |n: &'static str, title: &'static str, f: &dyn Fn() -> Outcome| {
        if !only.is_empty() && !only.iter().any(|o| o == n) {
            return;
        }
        let t = Instant::now();
        let r = f();
        eprintln!("  test {n} done in {:.0} s", t.elapsed().as_secs_f64());
        results.push((n, title, r));
    };
    run("45", "Every raid switch, turned off, gives the game without its rule", &|| test_switches(w));
    run("46", "A prisoner who reaches the trench is lost", &|| test_prisoners(w));
    run("47", "Nothing is known until word reaches the command post", &|| test_word(w));
    run("48", "Both sides see alike", &|| test_alike(w));
    run("49", "Replays with a raid", &|| test_replay(w));

    let failed = results.iter().filter(|r| r.2.is_err()).count();
    println!();
    println!("Alpine Front: night raid tests (awaiting approval, docs/scenario-tests.md)");
    println!("==========================================================================");
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
