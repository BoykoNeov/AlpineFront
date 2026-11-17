//! The morning report shown on the dawn orders screen (decision 32): what the company commander is
//! told at dawn about the last 24 hours. Plain sentences, built from the event journal.

use crate::sim::{Event, PLAYER_SIDE, Sim};
use crate::sky::{self, DUSK_ALTITUDE_DEG};

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
    let ours: Vec<_> = sim.squads.iter().filter(|q| q.side == PLAYER_SIDE).collect();
    let men: usize = ours.iter().map(|q| q.members.len()).sum();
    events.push(format!("The company: {men} men in {} squads, all present.", ours.len()));

    let Some((prev_i, prev_tick, _)) = previous else {
        events.push(
            "The first morning on the summit of Col di Lana. The Italians have been tunnelling toward it since mid-January."
                .into(),
        );
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
    Some(MorningReport { title, weather, events })
}
