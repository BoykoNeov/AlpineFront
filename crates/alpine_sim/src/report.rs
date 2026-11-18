//! The morning report shown on the dawn orders screen (decision 32): what the company commander is
//! told at dawn about the last 24 hours. Plain sentences, built from the event journal.

use crate::roster::{self, Duty};
use crate::sim::{DayRecord, Event, PLAYER_SIDE, Sim};
use crate::sky::{self, DUSK_ALTITUDE_DEG};
use crate::supply::{Lost, Stock};

pub struct MorningReport {
    /// "Monday 6 March 1916, first light at 06:12; sunrise at 06:42".
    pub title: String,
    /// The sky and the light now.
    pub weather: Vec<String>,
    /// What happened since the previous dawn.
    pub events: Vec<String>,
}

/// At most this many arrivals are listed one by one.
const LIST_AT_MOST: usize = 6;

/// The report for the latest dawn in the journal. `None` without a calendar.
pub fn morning_report(sim: &Sim) -> Option<MorningReport> {
    let clock = sim.clock.as_ref()?;
    let cal = &clock.calendar;
    let dt = sim.tuning.dt() as f64;
    let time_of = |tick: u64| cal.now(tick as f64 * dt);
    let dawns: Vec<(usize, u64, u32)> = sim
        .journal
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e.event {
            Event::Dawn { day } => Some((i, e.tick, day)),
            _ => None,
        })
        .collect();
    let &(last_i, last_tick, _) = dawns.last()?;
    let previous = dawns.len().checked_sub(2).map(|k| dawns[k]);
    let now = time_of(last_tick);
    let sunrise = cal.sunrise_on(now.to_seconds()).map(|t| format!("; sunrise at {}", sky::DateTime::from_seconds(t).hhmm())).unwrap_or_default();
    let title = format!("{}, first light at {}{sunrise}", now.long_date(), now.hhmm());

    let sky = clock.sky;
    let mut weather = vec![format!("Sky {}. Moon {}.", sky::cloud_words(sky.cloud), sky::moon_words(sky.moon_lit, sky.moon_waxing))];
    weather.push(format!("A man can be spotted at about {:.0} m now.", sim.spotting_range));

    let mut events = Vec::new();
    // Squads whose men are all lost are not counted.
    let ours: Vec<_> = sim.squads.iter().filter(|q| q.side == PLAYER_SIDE && !q.members.is_empty()).collect();
    let men: usize = ours.iter().map(|q| q.members.len()).sum();
    match &sim.supply {
        Some(sup) if !sup.lost.is_empty() => {
            let frost = sup.lost.iter().filter(|l| l.why == Lost::Frostbite).count();
            let dead = sup.lost.len() - frost;
            events.push(format!(
                "The company: {men} men in {} squads; lost since the first morning: {frost} sent down with frostbite, {dead} dead.",
                ours.len()
            ));
        }
        _ => events.push(format!("The company: {men} men in {} squads, all present.", ours.len())),
    }

    let Some((prev_i, prev_tick, _)) = previous else {
        events.push(
            "The first morning on the summit of Col di Lana. The Italians have been tunnelling toward it since mid-January."
                .into(),
        );
        supply_lines(sim, None, &mut events);
        roster_lines(sim, &mut events);
        return Some(MorningReport { title, weather, events });
    };
    let since = &sim.journal[prev_i + 1..last_i];
    events.push(format!("Since dawn yesterday ({}):", time_of(prev_tick).hhmm()));

    // Enemy contacts, summarised: by day the patrols are in sight most of the time.
    let contacts: Vec<(u64, f32, u32, u32)> = since
        .iter()
        .filter_map(|e| match e.event {
            Event::Contact { enemy_squad, seen_by_squad, distance_m } => Some((e.tick, distance_m, enemy_squad, seen_by_squad)),
            _ => None,
        })
        .collect();
    if contacts.is_empty() {
        events.push("No enemy was seen.".into());
    } else {
        let at_night = contacts
            .iter()
            .filter(|c| cal.sun_altitude(cal.local_seconds(c.0 as f64 * dt)) < DUSK_ALTITUDE_DEG)
            .count();
        let nearest = contacts.iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        events.push(format!(
            "Italians came into sight {} time{}, first at {} and last at {}; {}.",
            contacts.len(),
            if contacts.len() == 1 { "" } else { "s" },
            time_of(contacts[0].0).hhmm(),
            time_of(contacts[contacts.len() - 1].0).hhmm(),
            match at_night {
                0 => "none of them in the dark".to_string(),
                1 => "one of them in the dark".to_string(),
                n => format!("{n} of them in the dark"),
            }
        ));
        events.push(format!(
            "Closest: Italian squad {} at {:.0} m, seen by our squad {} at {}.",
            nearest.2,
            nearest.1,
            nearest.3,
            time_of(nearest.0).hhmm()
        ));
    }

    // Our squads' moves.
    let moves: Vec<String> = since
        .iter()
        .filter_map(|e| match e.event {
            Event::Arrived { squad } => Some(format!("squad {squad} arrived at {}", time_of(e.tick).hhmm())),
            Event::Unreachable { squad } => Some(format!("squad {squad} could not get where it was sent ({})", time_of(e.tick).hhmm())),
            _ => None,
        })
        .collect();
    if !moves.is_empty() {
        let shown = moves.len().min(LIST_AT_MOST);
        let mut line = moves[..shown].join("; ");
        if moves.len() > shown {
            line.push_str(&format!("; and {} more", moves.len() - shown));
        }
        let mut c = line.chars();
        let line = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
        events.push(format!("{line}."));
    }
    supply_lines(sim, Some(prev_tick), &mut events);
    roster_lines(sim, &mut events);
    Some(MorningReport { title, weather, events })
}

/// "squads 0, 1 and 4" / "squad 3".
fn squad_list(ids: &[u32]) -> String {
    let names: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
    match names.len() {
        0 => String::new(),
        1 => format!("squad {}", names[0]),
        n => format!("squads {} and {}", names[..n - 1].join(", "), names[n - 1]),
    }
}

fn hours(minutes: u32) -> String {
    format!("{:.0} h", minutes as f64 / 60.0)
}

/// The work roster's part of the report (decisions 39 to 43): what each squad did in the last 24
/// hours, how tired the company is, and the duties now set.
fn roster_lines(sim: &Sim, events: &mut Vec<String>) {
    let ours: Vec<_> = sim.squads.iter().filter(|q| q.side == PLAYER_SIDE && q.roster.is_some()).collect();
    if ours.is_empty() {
        return;
    }
    // What they did, grouped by hours on guard and asleep (to the nearest hour).
    let mut groups: Vec<((u32, u32, u32), Vec<u32>)> = Vec::new();
    let mut sent = Vec::new();
    for q in &ours {
        let Some(d) = q.yesterday else { continue };
        let DayRecord { guard_minutes, sleep_minutes, sent_away, carry_minutes } = d;
        let key = ((guard_minutes + 30) / 60, (sleep_minutes + 30) / 60, (carry_minutes + 30) / 60);
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push(q.id),
            None => groups.push((key, vec![q.id])),
        }
        if sent_away {
            sent.push(q.id);
        }
    }
    for ((g, sl, c), ids) in &groups {
        let mut s = squad_list(ids);
        if let Some(f) = s.get_mut(0..1) {
            f.make_ascii_uppercase();
        }
        let carrying = if *c > 0 { format!(", {} carrying", hours(c * 60)) } else { String::new() };
        events.push(format!("{s}: {} on guard, {} asleep{carrying}.", hours(g * 60), hours(sl * 60)));
    }
    if !sent.is_empty() {
        events.push(format!("Sent away from their duty by you: {}.", squad_list(&sent)));
    }

    // How tired they are now (each squad by its men's average).
    let mut by_word: [(&str, Vec<u32>); 3] = [("fresh", vec![]), ("tired", vec![]), ("exhausted", vec![])];
    for q in &ours {
        let t = q.members.iter().map(|&m| sim.soldiers[m as usize].tiredness).sum::<f32>() / q.members.len().max(1) as f32;
        let w = roster::tiredness_word(t);
        by_word.iter_mut().find(|b| b.0 == w).unwrap().1.push(q.id);
    }
    let count = |n: usize| match n {
        0 => "none".to_string(),
        1 => "1 squad".to_string(),
        n => format!("{n} squads"),
    };
    let mut line = format!(
        "The men now: {} fresh, {} tired, {} exhausted",
        count(by_word[0].1.len()),
        count(by_word[1].1.len()),
        count(by_word[2].1.len())
    );
    if !by_word[2].1.is_empty() {
        line.push_str(&format!(" ({}; an exhausted man sees half as far and walks slower)", squad_list(&by_word[2].1)));
    }
    events.push(format!("{line}."));

    let n = |f: &dyn Fn(Duty, Duty) -> bool| ours.iter().filter(|q| q.roster.is_some_and(|r| f(r.day, r.night))).count();
    let carry = if sim.supply.is_some() {
        format!(
            ", {} carrying by day, {} by night",
            count(n(&|d, _| d == Duty::Carry)),
            count(n(&|_, nt| nt == Duty::Carry))
        )
    } else {
        String::new()
    };
    events.push(format!(
        "The roster, same as yesterday unless you change it: {} on guard by day, {} by night, {} resting all day and night{carry}.",
        count(n(&|d, _| d == Duty::Guard)),
        count(n(&|_, nt| nt == Duty::Guard)),
        count(n(&|d, nt| d == Duty::Rest && nt == Duty::Rest)),
    ));
}

/// "2.5 days" / "1 day" / "none".
fn days(d: f32) -> String {
    if d < 0.05 {
        "none".to_string()
    } else if (d - 1.0).abs() < 0.05 {
        "1 day".to_string()
    } else {
        format!("{d:.1} days")
    }
}

/// Carrying, cold and hunger's part of the report (decisions 45 to 53): the stores, what was
/// carried, meals missed, men lost since the previous dawn, and how cold and hungry the men are.
fn supply_lines(sim: &Sim, since_tick: Option<u64>, events: &mut Vec<String>) {
    let Some(sup) = &sim.supply else { return };
    events.push(format!(
        "The store (it holds {}): food for {}, fuel for {}.",
        days(sup.tuning.stocks.store_days),
        days(sup.days_left(Stock::Food)),
        days(sup.days_left(Stock::Fuel))
    ));
    if since_tick.is_some() {
        let c = sup.carried_yesterday.unwrap_or([0.0; 2]);
        if c[0] + c[1] > 0.0 {
            events.push(format!("Carried up from the {}: {:.0} kg of food and {:.0} kg of fuel.", sim.places[sup.dump].name, c[0], c[1]));
        } else {
            events.push("Nothing was carried up.".into());
        }
        match sup.missed_meals_yesterday.unwrap_or(0) {
            0 => {}
            1 => events.push("At one meal there was not enough food for every man.".into()),
            n => events.push(format!("At {n} meals there was not enough food for every man.")),
        }
        if sup.fuel_kg <= 0.0 {
            events.push("There is no fuel: the shelters are not heated, and the men cannot warm up in them.".into());
        }
    }
    if let Some(from) = since_tick {
        let recent: Vec<_> = sup.lost.iter().filter(|l| l.tick > from).collect();
        for (why, words) in [(Lost::Frostbite, "sent down with frostbite"), (Lost::Died, "died of the cold")] {
            let mut ids: Vec<u32> = recent.iter().filter(|l| l.why == why).map(|l| l.squad).collect();
            if ids.is_empty() {
                continue;
            }
            let n = ids.len();
            ids.sort_unstable();
            ids.dedup();
            events.push(format!("{n} {} {words} ({}).", if n == 1 { "man" } else { "men" }, squad_list(&ids)));
        }
    }
    // How cold and hungry the men are now, man by man.
    let ct = &sup.tuning.cold;
    let ht = &sup.tuning.hunger;
    let ours: Vec<u32> = sim.squads.iter().filter(|q| q.side == PLAYER_SIDE).flat_map(|q| q.members.iter().copied()).collect();
    let tally = |f: &dyn Fn(u32) -> &'static str, words: [&str; 3]| -> String {
        let n: Vec<usize> = words.iter().map(|w| ours.iter().filter(|&&m| f(m) == *w).count()).collect();
        format!("{} {}, {} {}, {} {}", n[0], words[0], n[1], words[1], n[2], words[2])
    };
    events.push(format!(
        "The men now: {}; {}.",
        tally(&|m| ct.word(sim.soldiers[m as usize].chill), ["warm", "cold", "very cold"]),
        tally(&|m| ht.word(sim.soldiers[m as usize].minutes_since_meal), ["fed", "hungry", "starving"])
    ));
}
