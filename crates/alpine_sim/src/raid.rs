//! The night raid (decisions 69 to 75): about 30 Italian raiders wait hidden in their front trench
//! below the summit; a simple enemy officer picks the night from what he could know; on it they creep
//! up to one lookout post, try to overpower a guard or two in a short struggle and walk them back
//! under escort. Word passes by voice, gunshots, flares and runners, and the player hears of the raid
//! only when word reaches the command post. Shooting is crude hit or miss. The Italians tire under
//! the same rule as our men. Read from `data/raid.toml`; without that file (the locked tests 1 to 44)
//! none of this happens.
//!
//! Every part can be switched off on its own in the file (decision 65).

use crate::flow::FlowField;
use crate::rng::Rng;
use crate::roster::PlaceKind;
use crate::sim::{Event, PLAYER_SIDE, Side, Sim, SquadState};
use crate::snow::{RouteSnow, SnowAmount, table_at};
use crate::supply::Lost;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ---- the file ------------------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize)]
pub struct RaidFile {
    #[serde(default)]
    pub switches: RaidSwitches,
    pub trench: TrenchEntry,
    pub command_post: CommandPostEntry,
    pub raiders: RaiderTuning,
    pub officer: OfficerTuning,
    pub hearing: HearingTuning,
    pub voice: VoiceTuning,
    pub flares: FlareTuning,
    pub shooting: ShootingTuning,
    pub guards: GuardTuning,
    pub struggle: StruggleTuning,
    pub withdrawal: WithdrawalTuning,
}

fn yes() -> bool {
    true
}

/// Each part of the raid, on or off (decision 65).
#[derive(Clone, Debug, Deserialize)]
pub struct RaidSwitches {
    #[serde(default = "yes")]
    pub raid: bool,
    #[serde(default = "yes")]
    pub officer_picks_night: bool,
    #[serde(default = "yes")]
    pub italians_tire: bool,
    #[serde(default = "yes")]
    pub whispered_orders: bool,
    #[serde(default = "yes")]
    pub snow_muffles_shouts: bool,
    #[serde(default = "yes")]
    pub hearing: bool,
    #[serde(default = "yes")]
    pub flares: bool,
    #[serde(default = "yes")]
    pub shooting: bool,
    #[serde(default = "yes")]
    pub cover_at_posts: bool,
    #[serde(default = "yes")]
    pub standing_instruction: bool,
    #[serde(default = "yes")]
    pub word_must_reach_command_post: bool,
    #[serde(default = "yes")]
    pub struggle: bool,
    #[serde(default = "yes")]
    pub escort_can_be_freed: bool,
    #[serde(default = "yes")]
    pub close_combat_ability: bool,
    #[serde(default = "yes")]
    pub shelters_wake: bool,
}

impl Default for RaidSwitches {
    fn default() -> Self {
        RaidSwitches {
            raid: true,
            officer_picks_night: true,
            italians_tire: true,
            whispered_orders: true,
            snow_muffles_shouts: true,
            hearing: true,
            flares: true,
            shooting: true,
            cover_at_posts: true,
            standing_instruction: true,
            word_must_reach_command_post: true,
            struggle: true,
            escort_can_be_freed: true,
            close_combat_ability: true,
            shelters_wake: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct TrenchEntry {
    pub name: String,
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub source: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CommandPostEntry {
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RaiderTuning {
    pub squads: u32,
    pub men_per_squad: u32,
    pub creeping_pace: f32,
    pub creep_within_m: f32,
    pub running_pace: f32,
    pub escort_pace: f32,
    pub go_for_within_m: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OfficerTuning {
    pub first_night: u32,
    pub last_night: u32,
    pub chance_clear: f32,
    pub chance_overcast: f32,
    pub snow_forecast_bonus: f32,
    pub heavy_snow_forecast_bonus: f32,
    pub weak_post_bonus: f32,
    pub weak_below_share: f32,
    pub usual_men_at_post: u32,
    pub per_later_night: f32,
    pub count_minutes_after_dusk: f32,
    pub facing_within_m: f32,
    pub sets_out_from: String,
    pub sets_out_until: String,
    pub fixed_night: u32,
    pub fixed_time: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HearingTuning {
    pub within_m: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VoiceTuning {
    pub shout_by_snowfall: Vec<[f32; 2]>,
    pub whisper_m: f32,
    pub repeat_every_s: f32,
    pub on_own_after_alarm_minutes: f32,
    pub idle_at_post_minutes: f32,
    pub runner_mps: f32,
    pub runner_start_s: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FlareTuning {
    pub burst_ahead_m: f32,
    pub radius_m: f32,
    pub burns_s: f32,
    pub light_lux: f64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ShootingTuning {
    pub seconds_between_shots: f32,
    pub hit_by_distance: Vec<[f32; 2]>,
    pub hold_fire_m: f32,
    pub in_post_within_m: f32,
    pub post_cover_factor: f32,
    pub creeping_factor: f32,
    pub exhausted_shooter_factor: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GuardTuning {
    pub trigger_within_m: f32,
    pub closing_mps: f32,
    pub react_fresh_s: f32,
    pub react_exhausted_s: f32,
    pub shout_keep_coming_s: f32,
    pub shout_fire_within_m: f32,
    pub quiet_fire_within_m: f32,
    pub first_instruction: Instruction,
}

#[derive(Clone, Debug, Deserialize)]
pub struct StruggleTuning {
    pub contact_m: f32,
    pub raiders_needed: u32,
    pub lasts_from_s: f32,
    pub lasts_until_s: f32,
    pub dagger_factor: f32,
    pub alert_factor: f32,
    pub surprised_factor: f32,
    pub exhausted_factor: f32,
    pub comrades_within_m: f32,
    pub killed_share: f32,
    pub ability_spread: f32,
    pub raider_ability_bonus: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WithdrawalTuning {
    pub escorts: u32,
    pub prisoners_enough: u32,
    pub hit_share: f32,
    pub minutes_out: f32,
    pub alarm_far_m: f32,
}

/// "02:30" as seconds after midnight.
fn clock_seconds(text: &str) -> Result<f64, String> {
    let bad = || format!("raid file: '{text}' is not a time like 02:30");
    let (h, m) = text.trim().split_once(':').ok_or_else(bad)?;
    let (h, m): (u32, u32) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    if h > 23 || m > 59 {
        return Err(bad());
    }
    Ok((h * 3600 + m * 60) as f64)
}

impl RaidFile {
    pub fn from_toml(text: &str) -> Result<RaidFile, String> {
        let f: RaidFile = toml::from_str(text).map_err(|e| format!("raid file: {e}"))?;
        for (name, t) in [("voice.shout_by_snowfall", &f.voice.shout_by_snowfall), ("shooting.hit_by_distance", &f.shooting.hit_by_distance)] {
            if t.is_empty() || t.windows(2).any(|w| w[1][0] <= w[0][0]) {
                return Err(format!("raid file: {name} needs points in rising order"));
            }
        }
        clock_seconds(&f.officer.sets_out_from)?;
        clock_seconds(&f.officer.sets_out_until)?;
        clock_seconds(&f.officer.fixed_time)?;
        if f.raiders.squads == 0 || f.raiders.men_per_squad == 0 {
            return Err("raid file: the raiding party needs men".into());
        }
        if f.officer.first_night > f.officer.last_night {
            return Err("raid file: officer.first_night comes after last_night".into());
        }
        Ok(f)
    }

    /// How far a shout carries while snow falls at this rate.
    pub fn shout_m(&self, cm_per_hour: f32) -> f32 {
        let cm = if self.switches.snow_muffles_shouts { cm_per_hour } else { 0.0 };
        table_at(&self.voice.shout_by_snowfall, cm)
    }

    /// The chance to hit at this distance in good light.
    pub fn hit_chance(&self, d: f32) -> f32 {
        table_at(&self.shooting.hit_by_distance, d).clamp(0.0, 1.0)
    }
}

/// Whether a squad with a place stands at it: arrived there, or placed there when the game opened
/// (and so never walked).
fn standing(state: SquadState) -> bool {
    matches!(state, SquadState::Arrived | SquadState::Idle)
}

/// `data/raid.toml`'s text.
pub fn load_text() -> Result<String, String> {
    let path = crate::data_dir().join("raid.toml");
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

// ---- the state -------------------------------------------------------------------------------------

/// What the guards at the night posts do when men come at them (decision 73), set at the dawn
/// orders screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Instruction {
    /// Send up a flare at once and fire once they are within the hold-fire distance.
    Flare,
    /// Challenge them, waking the post and any post in earshot; flare and fire if they keep coming.
    Shout,
    /// Watch quietly and send a runner to the command post; fire only if they come very close.
    Quiet,
}

impl Instruction {
    pub const ALL: [Instruction; 3] = [Instruction::Flare, Instruction::Shout, Instruction::Quiet];

    pub fn words(self) -> &'static str {
        match self {
            Instruction::Flare => "flare and fire",
            Instruction::Shout => "shout first",
            Instruction::Quiet => "watch quietly and send word",
        }
    }

    pub fn explain(self) -> &'static str {
        match self {
            Instruction::Flare => "Send up a flare at once, and fire once they are within 100 m.",
            Instruction::Shout => "Challenge them, which wakes the post and any post in earshot; flare and fire if they keep coming.",
            Instruction::Quiet => "Keep quiet and send a runner to the command post; fire only if they come within 30 m.",
        }
    }
}

/// Where the raid is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stage {
    /// The officer has not chosen his night yet.
    Waiting,
    /// He has chosen tonight: the raiders set out at `depart` against the post `post` (an index into
    /// the places).
    Planned { depart: u64, post: usize },
    /// The raiders are out.
    Out { since: u64 },
    /// It is over (or there was no post to raid): no more raids this week.
    Over { at: u64 },
}

/// What a raider is doing, or what has become of one of our men.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    /// Not part of the raid.
    None,
    /// Waiting hidden in the trench.
    InTrench,
    /// Going for the post.
    Advance,
    /// In a struggle with a guard.
    Struggle,
    /// Walking a prisoner back.
    Escort,
    /// Going back to the trench.
    Withdraw,
    /// Back in the trench, hidden.
    Home,
    /// One of our men, taken and walked away.
    Captive,
}

/// The order the raiders' officer gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaidOrder {
    Advance,
    Withdraw,
}

/// How alert a post is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alert {
    Quiet,
    /// Someone there has seen men coming; he acts once his reaction time is up.
    Noticed { at: u64 },
    /// Challenged them (or woken by a challenge nearby).
    Challenged { at: u64 },
    /// Watching quietly; a runner is on his way.
    Watching,
    /// Firing (and sending up flares).
    Firing,
}

/// How word of the raid reached the command post.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordHow {
    Gunfire,
    Flare,
    Shout,
    Runner,
    /// The first sighting (with word_must_reach_command_post switched off).
    Sighting,
}

impl WordHow {
    pub fn words(self) -> &'static str {
        match self {
            WordHow::Gunfire => "gunfire",
            WordHow::Flare => "a flare going up",
            WordHow::Shout => "shouts",
            WordHow::Runner => "a runner",
            WordHow::Sighting => "the guards' sighting",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flare {
    pub x: f32,
    pub y: f32,
    pub until: u64,
    /// The post that sent it up.
    pub post: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Struggle {
    pub guard: u32,
    pub raiders: Vec<u32>,
    pub ends: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Prisoner {
    pub man: u32,
    /// His squad when taken.
    pub squad: u32,
    pub escorts: Vec<u32>,
}

/// A man hit, lying where he fell until dawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fallen {
    pub man: u32,
    pub side: Side,
    pub x: f32,
    pub y: f32,
}

/// What happened, for the messages and the morning report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RaidRecord {
    /// The night (0 = Monday) and the update the raiders set out.
    pub night: Option<u32>,
    pub set_out: Option<u64>,
    pub post: Option<usize>,
    /// Our men taken prisoner (soldier, squad), killed in a struggle, hit by a shot; prisoners freed.
    pub taken: Vec<(u32, u32)>,
    pub killed: Vec<(u32, u32)>,
    pub hit_ours: Vec<(u32, u32)>,
    pub freed: u32,
    pub raiders_hit: u32,
    pub shots: u32,
    pub flares: u32,
    pub struggles: u32,
    /// When and how word reached the command post.
    pub word: Option<(u64, WordHow)>,
    pub over_at: Option<u64>,
}

/// The raid, while the raid file is in force.
#[derive(Clone)]
pub struct Raid {
    pub tuning: RaidFile,
    pub rng: Rng,
    /// The trench's two ends and each raider's spot in it.
    pub trench: [(f32, f32); 2],
    pub trench_spots: Vec<(f32, f32)>,
    pub command_post: (f32, f32),
    pub raiders: Vec<u32>,
    pub raider_squads: Vec<u32>,
    pub officer: u32,
    pub instruction: Instruction,
    pub stage: Stage,
    /// The officer's current order, and the order each soldier last heard (by soldier id).
    pub order: RaidOrder,
    pub heard: Vec<Option<RaidOrder>>,
    pub heard_at: Vec<u64>,
    pub task: Vec<Task>,
    pub ability: Vec<f32>,
    pub next_shot: Vec<u64>,
    /// Raiders creeping now (harder to hit).
    pub creeping: Vec<bool>,
    /// Ticks a raider has stood at the post with nobody to take.
    pub idle_since: Vec<Option<u64>>,
    /// Each place's alert (posts only matter).
    pub alert: Vec<Alert>,
    pub flares: Vec<Flare>,
    pub struggles: Vec<Struggle>,
    pub prisoners: Vec<Prisoner>,
    pub fallen: Vec<Fallen>,
    /// The first flare or shot: the raiders know they are seen.
    pub alarm: Option<u64>,
    /// The first shot: heard all over the summit.
    pub gunfire: Option<u64>,
    /// A runner on his way: arrives at this update, from this post.
    pub runner: Option<(u64, usize)>,
    /// The update the latest dusk began.
    pub dusk_at: Option<u64>,
    /// The latest night the officer has made up his mind about.
    pub decided: Option<u32>,
    /// Men lost in silence, found missing at the next change of shift: (squad, men).
    pub missing: Vec<(u32, u32)>,
    pub record: RaidRecord,
    /// The routes to the post and back to the trench.
    pub to_post: Option<Arc<FlowField>>,
    pub to_trench: Option<Arc<FlowField>>,
}

impl std::fmt::Debug for Raid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Raid {{ stage: {:?}, record: {:?} }}", self.stage, self.record)
    }
}

impl Raid {
    pub fn is_raider(&self, i: u32) -> bool {
        !matches!(self.task.get(i as usize), None | Some(Task::None) | Some(Task::Captive))
    }

    pub fn captive(&self, i: u32) -> bool {
        self.task.get(i as usize) == Some(&Task::Captive)
    }

    pub fn in_struggle(&self, i: u32) -> bool {
        self.struggles.iter().any(|s| s.guard == i || s.raiders.contains(&i))
    }

    /// Whether the player knows of the raid: word has reached the command post (or the switch that
    /// asks for it is off and it has been seen).
    pub fn known(&self) -> bool {
        self.record.word.is_some()
    }

    /// Whether the raiders are out now.
    pub fn out(&self) -> bool {
        matches!(self.stage, Stage::Out { .. })
    }
}

/// News of the raid for the player (each stops the skip-ahead).
#[derive(Clone, Debug, PartialEq)]
pub enum RaidNews {
    /// Word has reached the command post.
    Word { post: usize, how: WordHow },
    /// The raid is over (told only if word had reached the command post).
    Over,
    /// Men found missing at a change of shift after a raid nobody heard.
    Missing { squads: Vec<(u32, u32)> },
}

// ---- the rules -------------------------------------------------------------------------------------

/// A random number spread around 0 with roughly this spread (the sum of three uniform numbers).
fn spread(rng: &mut Rng, s: f32) -> f32 {
    (rng.unit() + rng.unit() + rng.unit() - 1.5) * 2.0 * s
}

/// The posts facing the raid's trench, nearest first.
fn facing_posts(places: &[crate::roster::Place], r: &Raid) -> Vec<usize> {
    let mid = ((r.trench[0].0 + r.trench[1].0) / 2.0, (r.trench[0].1 + r.trench[1].1) / 2.0);
    let mut v: Vec<(f32, usize)> = places
        .iter()
        .enumerate()
        .filter(|(_, p)| p.kind == PlaceKind::Post)
        .map(|(i, p)| (dist((p.x, p.y), mid), i))
        .filter(|&(d, _)| d <= r.tuning.officer.facing_within_m)
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    v.into_iter().map(|(_, i)| i).collect()
}

/// Distance from `p` to the line between the two ends `ab`.
fn to_segment(p: (f32, f32), ab: [(f32, f32); 2]) -> f32 {
    let [(ax, ay), (bx, by)] = ab;
    let (dx, dy) = (bx - ax, by - ay);
    let l2 = (dx * dx + dy * dy).max(1e-6);
    let t = (((p.0 - ax) * dx + (p.1 - ay) * dy) / l2).clamp(0.0, 1.0);
    dist(p, (ax + dx * t, ay + dy * t))
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

impl Sim {
    /// Puts the raid file in force (decisions 69 to 75), in a game with a work roster: the raiders
    /// wait hidden in their trench. With the raid switched off nothing happens, as without the file.
    pub fn start_raid(&mut self, file: RaidFile) -> Result<(), String> {
        if self.places.is_empty() || self.clock.is_none() {
            return Err("the raid file needs a game with a work roster".into());
        }
        if !file.switches.raid {
            return Ok(());
        }
        let t = &self.terrain;
        let (a, b) = ((file.trench.from[0], file.trench.from[1]), (file.trench.to[0], file.trench.to[1]));
        let n = (file.raiders.squads * file.raiders.men_per_squad) as usize;
        let mut spots = Vec::with_capacity(n);
        for k in 0..n {
            let f = (k as f32 + 0.5) / n as f32;
            let (x, y) = (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f);
            let (x, y) = match t.nearest_main_point(x, y, 3) {
                Some(p) if !t.walkable_at(x, y) => t.point_xy(p.0, p.1),
                Some(_) => (x, y),
                None => return Err(format!("raid file: the trench at ({x:.0}, {y:.0}) is not on ground a man can stand on")),
            };
            spots.push((x, y));
        }
        let mut raiders = Vec::new();
        let mut squads = Vec::new();
        let per = file.raiders.men_per_squad as usize;
        for q in 0..file.raiders.squads as usize {
            let id = self.add_squad(Side::Italy, &spots[q * per..(q + 1) * per]);
            squads.push(id);
            raiders.extend(self.squads[id as usize].members.iter().copied());
        }
        let men = self.soldiers.len();
        let mut rng = Rng::new(self.seed ^ 0x5241_4944_1916);
        let mut ability = vec![1.0; men];
        let st = &file.struggle;
        if file.switches.close_combat_ability {
            for (i, a) in ability.iter_mut().enumerate() {
                let bonus = if raiders.contains(&(i as u32)) { st.raider_ability_bonus } else { 0.0 };
                *a = (1.0 + bonus + spread(&mut rng, st.ability_spread)).max(0.2);
            }
        }
        let mut task = vec![Task::None; men];
        for &r in &raiders {
            task[r as usize] = Task::InTrench;
            self.soldiers[r as usize].hidden = true;
        }
        let instruction = if file.switches.standing_instruction { file.guards.first_instruction } else { Instruction::Shout };
        self.raid = Some(Raid {
            rng,
            trench: [a, b],
            trench_spots: spots,
            command_post: (file.command_post.x, file.command_post.y),
            officer: raiders[0],
            raiders,
            raider_squads: squads,
            instruction,
            stage: Stage::Waiting,
            order: RaidOrder::Advance,
            heard: vec![None; men],
            heard_at: vec![0; men],
            task,
            ability,
            next_shot: vec![0; men],
            creeping: vec![false; men],
            idle_since: vec![None; men],
            alert: vec![Alert::Quiet; self.places.len()],
            flares: Vec::new(),
            struggles: Vec::new(),
            prisoners: Vec::new(),
            fallen: Vec::new(),
            alarm: None,
            gunfire: None,
            runner: None,
            dusk_at: None,
            decided: None,
            missing: Vec::new(),
            record: RaidRecord::default(),
            to_post: None,
            to_trench: None,
            tuning: file,
        });
        Ok(())
    }

    /// The standing instruction the player has set, including one given while time stands still.
    pub fn instruction_shown(&self) -> Option<Instruction> {
        let r = self.raid.as_ref()?;
        if !r.tuning.switches.standing_instruction {
            return None;
        }
        Some(self.pending_instruction().unwrap_or(r.instruction))
    }

    /// For checks only: the raiders set out now against `post` (an index into the places), whatever
    /// the night. Not something the game does by itself; not recorded in replays.
    pub fn force_raid(&mut self, post: usize) {
        let tick = self.tick;
        if let Some(r) = self.raid.as_mut()
            && !matches!(r.stage, Stage::Out { .. })
        {
            r.stage = Stage::Planned { depart: tick, post };
        }
    }

    /// The posts facing the Italian trench, nearest first.
    pub fn posts_facing_trench(&self) -> Vec<usize> {
        match &self.raid {
            Some(r) => facing_posts(&self.places, r),
            None => Vec::new(),
        }
    }

    /// How far a man sees, in fresh eyes, someone lit by a flare now: the flare's light, cut by the
    /// falling snow as daylight is.
    pub(crate) fn flare_sight(&self) -> f32 {
        match &self.raid {
            Some(r) => self.sight_in(r.tuning.flares.light_lux),
            None => 0.0,
        }
    }

    /// How far a shout carries now.
    pub fn shout_m(&self) -> f32 {
        self.raid.as_ref().map_or(0.0, |r| self.shout_with(r))
    }

    /// How far a shout carries now, with the raid file `r` (usable while the raid is updated, when
    /// it is out of `self.raid`).
    fn shout_with(&self, r: &Raid) -> f32 {
        let falling = self.snow.as_ref().map_or(0.0, |s| s.falling);
        r.tuning.shout_m(falling)
    }

    /// Our men at post `p` now: their squad stands there, outside.
    fn guards_at(&self, p: usize) -> Vec<u32> {
        self.squads
            .iter()
            .filter(|q| q.side == PLAYER_SIDE && q.post.map(|x| x.0) == Some(p) && !q.inside && standing(q.state))
            .flat_map(|q| q.members.iter().copied())
            .collect()
    }

    /// Whether our man `g` stands in his post (for cover).
    fn in_post(&self, g: u32, within: f32) -> bool {
        let s = &self.soldiers[g as usize];
        let q = &self.squads[s.squad as usize];
        match q.post {
            Some((p, k)) if self.places[p].kind == PlaceKind::Post && standing(q.state) && !q.inside => {
                dist((s.x, s.y), self.places[p].spot(k)) <= within
            }
            _ => false,
        }
    }

    /// The route for an Italian on foot through the snow as it lies now.
    fn italian_route_snow(&self) -> Option<RouteSnow> {
        let sn = self.snow.as_ref().filter(|s| s.routes_see_snow())?;
        Some(RouteSnow { loose: Arc::new(sn.loose.clone()), table: sn.tuning.walking.on_foot.clone() })
    }

    /// Sends soldier `i` on his own way: along `field` (or straight) to `goal`, at `pace`. Keeps his
    /// progress if it is the same way.
    fn send_own(&mut self, i: u32, field: Option<&Arc<FlowField>>, goal: (f32, f32), pace: f32) {
        let s = &mut self.soldiers[i as usize];
        if let Some(o) = s.own.as_mut() {
            let same_field = match (&o.field, field) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            };
            if same_field && dist(o.goal, goal) < 0.5 {
                o.pace = pace;
                return;
            }
            if same_field && field.is_none() {
                // Chasing a moving man: just move the goal.
                o.goal = goal;
                o.pace = pace;
                s.arrived = false;
                return;
            }
        }
        s.own = Some(crate::sim::OwnMove { field: field.cloned(), goal, pace });
        s.arrived = false;
        s.waypoint = None;
        s.still_ticks = 0;
    }

    fn stop_own(&mut self, i: u32) {
        let s = &mut self.soldiers[i as usize];
        s.own = None;
        s.arrived = true;
        s.waypoint = None;
    }

    /// Once a game minute with the raid in force: the Italians tire and rest by the same rule as our
    /// men (decision 75). The raiders sleep in their trench; the far patrols are relieved by fresh men
    /// at dawn and dusk.
    pub(crate) fn italian_tiredness_minute(&mut self, up: f32, down: f32) {
        let Some(r) = &self.raid else { return };
        if !r.tuning.switches.italians_tire || !self.tuning.tiredness.enabled {
            return;
        }
        for &i in &self.by_side[Side::Italy.index()] {
            let s = &mut self.soldiers[i as usize];
            let resting = r.task[i as usize] == Task::InTrench || r.task[i as usize] == Task::Home;
            s.tiredness = if resting { (s.tiredness - down).max(0.0) } else { (s.tiredness + up).min(1.0) };
        }
    }

    /// Dawn or dusk with the raid in force: the far patrols are relieved by fresh men.
    pub(crate) fn relieve_patrols(&mut self) {
        let Some(r) = &self.raid else { return };
        if !r.tuning.switches.italians_tire {
            return;
        }
        for &i in &self.by_side[Side::Italy.index()] {
            if r.task[i as usize] == Task::None {
                self.soldiers[i as usize].tiredness = 0.0;
            }
        }
    }

    /// The raid's part of each update (after spotting): the officer's choice of night, the raiders,
    /// the guards, shooting, struggles, escorts and word to the command post.
    pub(crate) fn raid_step(&mut self, tick: u64) {
        let Some(mut r) = self.raid.take() else { return };
        let dawn = self.events.iter().any(|e| matches!(e, Event::Dawn { .. }));
        let dusk = self.events.iter().any(|e| matches!(e, Event::Dusk));
        if dusk {
            r.dusk_at = Some(tick);
        }
        if dawn || dusk {
            // A change of shift: men taken in silence are found missing.
            if !r.missing.is_empty() && !r.out() {
                let squads = std::mem::take(&mut r.missing);
                self.note(Event::Raid(RaidNews::Missing { squads }));
            }
        }
        if dawn {
            r.fallen.clear();
        }
        self.decide_night(&mut r, tick);
        if let Stage::Planned { depart, post } = r.stage
            && tick >= depart
        {
            self.set_out(&mut r, tick, post);
        }
        if r.out() {
            self.raid_tick(&mut r, tick, dawn);
        }
        r.flares.retain(|f| f.until > tick);
        self.raid = Some(r);
    }

    /// The officer makes up his mind about tonight, a while after dusk (decision 75).
    fn decide_night(&mut self, r: &mut Raid, tick: u64) {
        if r.stage != Stage::Waiting {
            return;
        }
        let Some(dusk) = r.dusk_at else { return };
        let ot = r.tuning.officer.clone();
        if tick != dusk + self.tuning.seconds_to_ticks(ot.count_minutes_after_dusk * 60.0) {
            return;
        }
        let Some(c) = &self.clock else { return };
        let night = c.day;
        if r.decided.is_some_and(|d| d >= night) {
            return;
        }
        r.decided = Some(night);
        let picks = r.tuning.switches.officer_picks_night;
        if picks && night < ot.first_night {
            return;
        }
        if !picks && night != ot.fixed_night {
            return;
        }
        // What he sees from his trench: men at the posts facing him.
        let mid = ((r.trench[0].0 + r.trench[1].0) / 2.0, (r.trench[0].1 + r.trench[1].1) / 2.0);
        let t = &self.terrain;
        let eye = [mid.0, mid.1, t.height(mid.0, mid.1) + self.tuning.soldier.eye_height_m];
        let head = self.tuning.soldier.head_height_m;
        let facing = facing_posts(&self.places, r);
        let range = self.spotting_range;
        let mut seen: Vec<(usize, u32)> = Vec::new();
        for &p in &facing {
            let mut n = 0;
            for g in self.guards_at(p) {
                let s = &self.soldiers[g as usize];
                if dist((s.x, s.y), mid) <= range && crate::los::visible(t, &self.los, eye, [s.x, s.y, t.height(s.x, s.y) + head]) {
                    n += 1;
                }
            }
            seen.push((p, n));
        }
        let usual = ot.usual_men_at_post as f32;
        let weak = seen.iter().any(|&(_, n)| n > 0 && (n as f32) < usual * ot.weak_below_share);
        let target = seen
            .iter()
            .filter(|&&(_, n)| n > 0)
            .min_by_key(|&&(p, n)| (n, facing.iter().position(|&f| f == p)))
            .map(|&(p, _)| p)
            .or_else(|| facing.iter().copied().find(|&p| self.squads.iter().any(|q| q.side == PLAYER_SIDE && q.post.map(|x| x.0) == Some(p) && !q.members.is_empty())));
        let Some(post) = target else {
            if night >= ot.last_night || !picks {
                r.stage = Stage::Over { at: tick };
            }
            return;
        };
        let go = if !picks || night >= ot.last_night {
            true
        } else {
            let cover = c.sky.cloud as f32;
            let mut chance = ot.chance_clear + (ot.chance_overcast - ot.chance_clear) * cover;
            // His own forecast for the night, from the weather diary, wrong as often as ours.
            if let Some(sn) = self.snow.as_ref().filter(|s| s.tuning.switches.snowfall) {
                let now = c.calendar.local_seconds(self.game_seconds());
                let midnight = (now / 86_400.0).floor() * 86_400.0;
                let truth = sn.tuning.amount(c.weather.snowfall_between(now, midnight + 30.0 * 3600.0) as f32);
                let mut said = truth;
                if sn.tuning.switches.wrong_forecasts && r.rng.unit() < sn.tuning.forecast.wrong_chance {
                    said = if truth == SnowAmount::None { SnowAmount::Light } else { SnowAmount::None };
                }
                chance += match said {
                    SnowAmount::None => 0.0,
                    SnowAmount::Light => ot.snow_forecast_bonus,
                    SnowAmount::Heavy => ot.heavy_snow_forecast_bonus,
                };
            }
            if weak {
                chance += ot.weak_post_bonus;
            }
            chance += ot.per_later_night * (night - ot.first_night) as f32;
            r.rng.unit() < chance
        };
        if !go {
            return;
        }
        let now = c.calendar.local_seconds(self.game_seconds());
        let next_midnight = (now / 86_400.0).floor() * 86_400.0 + 86_400.0;
        let at = if picks {
            let (a, b) = (clock_seconds(&ot.sets_out_from).unwrap(), clock_seconds(&ot.sets_out_until).unwrap());
            a + (b - a) * r.rng.unit() as f64
        } else {
            clock_seconds(&ot.fixed_time).unwrap()
        };
        let secs = next_midnight + at - c.calendar.start;
        let depart = (secs / self.tuning.dt() as f64).round() as u64;
        r.record.night = Some(night);
        r.stage = Stage::Planned { depart: depart.max(tick + 1), post };
    }

    /// The raiders climb out of their trench and set off for the post.
    fn set_out(&mut self, r: &mut Raid, tick: u64, post: usize) {
        r.stage = Stage::Out { since: tick };
        r.record.set_out = Some(tick);
        r.record.post = Some(post);
        r.order = RaidOrder::Advance;
        let t = self.terrain.clone();
        let p = &self.places[post];
        let target = (p.x, p.y);
        let snow = self.italian_route_snow();
        let raiders: Vec<u32> = r.raiders.iter().copied().filter(|&i| self.soldiers[i as usize].gone.is_none()).collect();
        let from: Vec<(usize, usize)> = raiders.iter().map(|&i| t.nearest_point(self.soldiers[i as usize].x, self.soldiers[i as usize].y)).collect();
        let dp = t.nearest_main_point(target.0, target.1, 5).unwrap_or_else(|| t.nearest_point(target.0, target.1));
        let to_post = Arc::new(FlowField::compute_bounded(&t, &self.tuning.walking, dp, &from, snow.clone()));
        let mid = ((r.trench[0].0 + r.trench[1].0) / 2.0, (r.trench[0].1 + r.trench[1].1) / 2.0);
        let mp = t.nearest_main_point(mid.0, mid.1, 5).unwrap_or_else(|| t.nearest_point(mid.0, mid.1));
        // Back to the trench from anywhere near the post or the trench.
        let mut back_from = from.clone();
        let (pc, pr) = t.nearest_point(target.0, target.1);
        for dr in -6i32..=6 {
            for dc in -6i32..=6 {
                let (c, rr) = (pc as i32 + dc, pr as i32 + dr);
                if c >= 0 && rr >= 0 && (c as usize) < t.cols && (rr as usize) < t.rows {
                    back_from.push((c as usize, rr as usize));
                }
            }
        }
        let to_trench = Arc::new(FlowField::compute_bounded(&t, &self.tuning.walking, mp, &back_from, snow));
        r.to_post = Some(to_post);
        r.to_trench = Some(to_trench);
        let goals = self.formation_around(target.0, target.1, raiders.len());
        for (k, &i) in raiders.iter().enumerate() {
            r.task[i as usize] = Task::Advance;
            r.heard[i as usize] = Some(RaidOrder::Advance);
            r.heard_at[i as usize] = tick;
            self.soldiers[i as usize].hidden = false;
            let field = r.to_post.clone();
            self.send_own(i, field.as_ref(), goals[k], 1.0);
        }
    }

    /// One update of a raid under way.
    fn raid_tick(&mut self, r: &mut Raid, tick: u64, dawn: bool) {
        let tps = self.tuning.time.ticks_per_second as f32;
        let secs = move |s: f32| (s * tps).round().max(1.0) as u64;
        let Stage::Out { since } = r.stage else { return };
        let post = r.record.post.unwrap();
        let post_xy = (self.places[post].x, self.places[post].y);
        let shout = self.shout_with(r);
        let sw = r.tuning.switches.clone();

        // Word without the command-post rule: the first sighting tells the player.
        if !sw.word_must_reach_command_post && r.record.word.is_none() {
            let seen = self.by_side[PLAYER_SIDE.index()].iter().any(|&g| self.soldiers[g as usize].sees.iter().any(|&e| r.is_raider(e)));
            if seen {
                self.word(r, tick, post, WordHow::Sighting);
            }
        }

        // The officer: decides, and passes his order to the men who hear him.
        let officer_ok = self.soldiers[r.officer as usize].gone.is_none() && matches!(r.task[r.officer as usize], Task::Advance | Task::Withdraw);
        if officer_ok && r.order == RaidOrder::Advance {
            let alive = r.raiders.iter().filter(|&&i| self.soldiers[i as usize].gone.is_none()).count();
            let wt = &r.tuning.withdrawal;
            let o = &self.soldiers[r.officer as usize];
            let far = dist((o.x, o.y), post_xy) > wt.alarm_far_m;
            let held = r.record.taken.len() + r.prisoners.len();
            let empty_post = self.guards_at(post).is_empty() && r.struggles.is_empty();
            let reason = (r.prisoners.len() as u32 + r.record.taken.len() as u32 >= wt.prisoners_enough)
                || (held >= 1 && r.alarm.is_some())
                || (r.raiders.len() - alive) as f32 >= wt.hit_share * r.raiders.len() as f32
                || tick - since >= secs(wt.minutes_out * 60.0)
                || (r.alarm.is_some() && far && r.prisoners.is_empty())
                || (empty_post && dist((o.x, o.y), post_xy) < 15.0);
            if reason {
                r.order = RaidOrder::Withdraw;
                r.heard[r.officer as usize] = Some(RaidOrder::Withdraw);
            }
        }
        if officer_ok && (tick - since).is_multiple_of(secs(r.tuning.voice.repeat_every_s)) {
            let voice = if !sw.whispered_orders { f32::INFINITY } else if r.alarm.is_some() { shout } else { r.tuning.voice.whisper_m };
            let o = &self.soldiers[r.officer as usize];
            let at = (o.x, o.y);
            for &i in &r.raiders {
                let s = &self.soldiers[i as usize];
                if s.gone.is_none() && dist((s.x, s.y), at) <= voice {
                    r.heard[i as usize] = Some(r.order);
                    r.heard_at[i as usize] = tick;
                }
            }
        }

        // Each raider.
        let rt = r.tuning.raiders.clone();
        let on_own = secs(r.tuning.voice.on_own_after_alarm_minutes * 60.0);
        let idle_limit = secs(r.tuning.voice.idle_at_post_minutes * 60.0);
        let raiders = r.raiders.clone();
        for &i in &raiders {
            let iu = i as usize;
            if self.soldiers[iu].gone.is_some() {
                continue;
            }
            match r.task[iu] {
                Task::Advance => {
                    let alarm_old = r.alarm.is_some_and(|a| tick - a >= on_own);
                    if r.heard[iu] == Some(RaidOrder::Withdraw) || (alarm_old && tick - r.heard_at[iu] >= on_own) || r.idle_since[iu].is_some_and(|t0| tick - t0 >= idle_limit) {
                        r.task[iu] = Task::Withdraw;
                        r.creeping[iu] = false;
                        continue;
                    }
                    let s = &self.soldiers[iu];
                    let me = (s.x, s.y);
                    // The nearest of our men he has noticed, close enough to go for.
                    let prey = s
                        .sees
                        .iter()
                        .copied()
                        .filter(|&e| !r.captive(e) && self.soldiers[e as usize].gone.is_none())
                        .map(|e| (dist(me, (self.soldiers[e as usize].x, self.soldiers[e as usize].y)), e))
                        .filter(|&(d, _)| d <= rt.go_for_within_m)
                        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                    let near_post = dist(me, post_xy) <= rt.creep_within_m;
                    match prey {
                        Some((_, e)) => {
                            r.idle_since[iu] = None;
                            r.creeping[iu] = false;
                            let g = &self.soldiers[e as usize];
                            let goal = (g.x, g.y);
                            self.send_own(i, None, goal, rt.running_pace);
                        }
                        None => {
                            let creeping = near_post && r.alarm.is_none();
                            r.creeping[iu] = creeping;
                            let pace = if creeping { rt.creeping_pace } else { 1.0 };
                            let at_post = dist(me, post_xy) <= 10.0;
                            if at_post {
                                r.idle_since[iu].get_or_insert(tick);
                            } else {
                                r.idle_since[iu] = None;
                            }
                            // Back on the route to the post (after chasing a man, say).
                            let field = r.to_post.clone();
                            let goal = match &self.soldiers[iu].own {
                                Some(o) if o.field.is_some() => o.goal,
                                _ => post_xy,
                            };
                            self.send_own(i, field.as_ref(), goal, pace);
                        }
                    }
                }
                Task::Withdraw => {
                    let k = raiders.iter().position(|&x| x == i).unwrap();
                    let goal = r.trench_spots[k];
                    let field = r.to_trench.clone();
                    self.send_own(i, field.as_ref(), goal, 1.0);
                    let s = &self.soldiers[iu];
                    if s.arrived || dist((s.x, s.y), goal) < 2.0 || to_segment((s.x, s.y), r.trench) < 4.0 {
                        r.task[iu] = Task::Home;
                        self.stop_own(i);
                        self.soldiers[iu].hidden = true;
                        self.soldiers[iu].sees.clear();
                    }
                }
                Task::Struggle => self.stop_own(i),
                _ => {}
            }
        }

        // The guards at each post, by the standing instruction.
        let gt = r.tuning.guards.clone();
        let instruction = if sw.standing_instruction { r.instruction } else { Instruction::Shout };
        let posts: Vec<usize> = (0..self.places.len()).filter(|&p| self.places[p].kind == PlaceKind::Post).collect();
        for &p in &posts {
            let guards = self.guards_at(p);
            if guards.is_empty() {
                continue;
            }
            let pxy = (self.places[p].x, self.places[p].y);
            // Raiders the guards here notice: (distance to the post, closing).
            let mut nearest_seen = f32::INFINITY;
            let mut coming: Option<u32> = None;
            for &g in &guards {
                for &e in &self.soldiers[g as usize].sees {
                    if !r.is_raider(e) {
                        continue;
                    }
                    let en = &self.soldiers[e as usize];
                    let d = dist((en.x, en.y), pxy);
                    nearest_seen = nearest_seen.min(d);
                    let (dx, dy) = (pxy.0 - en.x, pxy.1 - en.y);
                    let closing = (en.vx * dx + en.vy * dy) / d.max(0.1);
                    if d <= gt.trigger_within_m && (closing >= gt.closing_mps || r.task[e as usize] == Task::Struggle) {
                        coming = Some(g);
                    }
                }
            }
            match r.alert[p] {
                Alert::Quiet => {
                    if coming.is_some() {
                        r.alert[p] = Alert::Noticed { at: tick };
                    }
                }
                Alert::Noticed { at } => {
                    let tired = guards.iter().map(|&g| self.soldiers[g as usize].tiredness).fold(1.0f32, f32::min);
                    let react = gt.react_fresh_s + (gt.react_exhausted_s - gt.react_fresh_s) * tired;
                    if tick - at >= secs(react) {
                        match instruction {
                            Instruction::Flare => {
                                r.alert[p] = Alert::Firing;
                                self.send_flare(r, tick, p);
                            }
                            Instruction::Shout => {
                                r.alert[p] = Alert::Challenged { at: tick };
                                // The challenge wakes every post in earshot, and the command post if near.
                                for &q in &posts {
                                    if q != p && r.alert[q] == Alert::Quiet && dist((self.places[q].x, self.places[q].y), pxy) <= shout {
                                        r.alert[q] = Alert::Challenged { at: tick };
                                    }
                                }
                                if dist(pxy, r.command_post) <= shout {
                                    self.word(r, tick, p, WordHow::Shout);
                                }
                            }
                            Instruction::Quiet => {
                                r.alert[p] = Alert::Watching;
                                if r.runner.is_none() && r.record.word.is_none() {
                                    let speed = r.tuning.voice.runner_mps
                                        * self.snow.as_ref().map_or(1.0, |sn| {
                                            let (c, rr) = self.terrain.nearest_point(pxy.0, pxy.1);
                                            sn.tuning.walking_factor(sn.loose[self.terrain.idx(c, rr)], false)
                                        });
                                    let s = r.tuning.voice.runner_start_s + dist(pxy, r.command_post) / speed.max(0.05);
                                    r.runner = Some((tick + secs(s), p));
                                }
                            }
                        }
                    }
                }
                Alert::Challenged { at } => {
                    let fire = nearest_seen <= gt.shout_fire_within_m || (coming.is_some() && tick - at >= secs(gt.shout_keep_coming_s));
                    if fire {
                        r.alert[p] = Alert::Firing;
                        self.send_flare(r, tick, p);
                    }
                }
                Alert::Watching => {
                    if nearest_seen <= gt.quiet_fire_within_m {
                        r.alert[p] = Alert::Firing;
                        self.send_flare(r, tick, p);
                    }
                }
                Alert::Firing => {
                    // Another flare when the last has burnt out and they are still about.
                    if nearest_seen.is_finite() && !r.flares.iter().any(|f| f.post == p) {
                        self.send_flare(r, tick, p);
                    }
                }
            }
        }

        // Struggles begin where two raiders reach one of our men.
        let st = r.tuning.struggle.clone();
        let ours: Vec<u32> = self.by_side[PLAYER_SIDE.index()].clone();
        for &g in &ours {
            if r.in_struggle(g) || self.squads[self.soldiers[g as usize].squad as usize].inside {
                continue;
            }
            let gs = &self.soldiers[g as usize];
            let gxy = (gs.x, gs.y);
            let mut close: Vec<(f32, u32)> = r
                .raiders
                .iter()
                .copied()
                .filter(|&i| r.task[i as usize] == Task::Advance && self.soldiers[i as usize].gone.is_none())
                .map(|i| (dist(gxy, (self.soldiers[i as usize].x, self.soldiers[i as usize].y)), i))
                .filter(|&(d, _)| d <= st.contact_m)
                .collect();
            if (close.len() as u32) < st.raiders_needed {
                continue;
            }
            close.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let men: Vec<u32> = close.iter().take(3).map(|&(_, i)| i).collect();
            r.record.struggles += 1;
            for &i in &men {
                r.task[i as usize] = Task::Struggle;
                self.stop_own(i);
            }
            if !sw.struggle {
                // Off: the guard is taken at once.
                self.take_prisoner(r, tick, g, men);
                continue;
            }
            let lasts = st.lasts_from_s + (st.lasts_until_s - st.lasts_from_s) * r.rng.unit();
            r.struggles.push(Struggle { guard: g, raiders: men, ends: tick + secs(lasts) });
            // The noise carries as far as a shout: posts in earshot notice.
            for &p in &posts {
                let pxy = (self.places[p].x, self.places[p].y);
                if r.alert[p] == Alert::Quiet && dist(pxy, gxy) <= shout && !self.guards_at(p).is_empty() {
                    r.alert[p] = Alert::Noticed { at: tick };
                }
            }
            if dist(gxy, r.command_post) <= shout {
                self.word(r, tick, post, WordHow::Shout);
            }
        }

        // Struggles that are over.
        let mut k = 0;
        while k < r.struggles.len() {
            let sg = r.struggles[k].clone();
            let guard_gone = self.soldiers[sg.guard as usize].gone.is_some();
            let men: Vec<u32> = sg.raiders.iter().copied().filter(|&i| self.soldiers[i as usize].gone.is_none()).collect();
            if guard_gone || men.is_empty() {
                for &i in &men {
                    r.task[i as usize] = Task::Advance;
                }
                r.struggles.remove(k);
                continue;
            }
            if tick < sg.ends {
                r.struggles[k].raiders = men;
                k += 1;
                continue;
            }
            r.struggles.remove(k);
            let raiders_side: f32 = men.iter().map(|&i| r.ability[i as usize] * st.dagger_factor).sum();
            let gxy = (self.soldiers[sg.guard as usize].x, self.soldiers[sg.guard as usize].y);
            let mut guard_side = 0.0;
            for &m in &ours {
                let ms = &self.soldiers[m as usize];
                if ms.gone.is_some() || r.captive(m) || (m != sg.guard && (r.in_struggle(m) || dist(gxy, (ms.x, ms.y)) > st.comrades_within_m)) {
                    continue;
                }
                if self.squads[ms.squad as usize].inside {
                    continue;
                }
                let p = self.squads[ms.squad as usize].post.map(|x| x.0);
                let alert = r.alarm.is_some() || p.is_some_and(|p| !matches!(r.alert[p], Alert::Quiet | Alert::Noticed { .. }));
                let f = if alert { st.alert_factor } else { st.surprised_factor };
                guard_side += r.ability[m as usize] * f * (1.0 - (1.0 - st.exhausted_factor) * ms.tiredness);
            }
            let lose = raiders_side / (raiders_side + guard_side).max(1e-6);
            if r.rng.unit() < lose {
                if r.rng.unit() < st.killed_share {
                    let squad = self.soldiers[sg.guard as usize].squad;
                    r.record.killed.push((sg.guard, squad));
                    self.fall(r, sg.guard, tick, Lost::Hit);
                    if r.record.word.is_none() {
                        Self::add_missing(r, squad);
                    }
                    for &i in &men {
                        r.task[i as usize] = Task::Advance;
                    }
                } else {
                    self.take_prisoner(r, tick, sg.guard, men);
                }
            } else {
                // He breaks free; one raider is hit in the scuffle.
                let hit = men[(r.rng.unit() * men.len() as f32) as usize % men.len()];
                r.record.raiders_hit += 1;
                self.fall(r, hit, tick, Lost::Hit);
                for &i in &men {
                    if i != hit {
                        r.task[i as usize] = Task::Advance;
                    }
                }
                let p = self.squads[self.soldiers[sg.guard as usize].squad as usize].post.map(|x| x.0);
                if let Some(p) = p
                    && matches!(r.alert[p], Alert::Quiet | Alert::Noticed { .. })
                {
                    r.alert[p] = Alert::Noticed { at: 0 };
                }
            }
        }

        // Prisoners and their escorts.
        let mut k = 0;
        while k < r.prisoners.len() {
            let pr = r.prisoners[k].clone();
            let escorts: Vec<u32> = pr.escorts.iter().copied().filter(|&i| self.soldiers[i as usize].gone.is_none()).collect();
            let man = &self.soldiers[pr.man as usize];
            let mid = r.trench_spots[r.trench_spots.len() / 2];
            let home = man.arrived || to_segment((man.x, man.y), r.trench) < 4.0 || (dawn && !escorts.is_empty());
            if escorts.is_empty() && sw.escort_can_be_freed {
                // Freed: he goes back to his squad.
                r.prisoners.remove(k);
                r.record.freed += 1;
                self.free_man(r, pr.man, pr.squad);
                continue;
            }
            if home {
                r.prisoners.remove(k);
                r.record.taken.push((pr.man, pr.squad));
                self.lose_man(pr.man, Lost::Taken, tick);
                self.soldiers[pr.man as usize].own = None;
                if r.record.word.is_none() {
                    Self::add_missing(r, pr.squad);
                }
                for &e in &escorts {
                    r.task[e as usize] = Task::Home;
                    self.stop_own(e);
                    self.soldiers[e as usize].hidden = true;
                    self.soldiers[e as usize].sees.clear();
                }
                continue;
            }
            r.prisoners[k].escorts = escorts.clone();
            let field = r.to_trench.clone();
            let pace = r.tuning.raiders.escort_pace;
            self.send_own(pr.man, field.as_ref(), mid, pace);
            for &e in &escorts {
                self.send_own(e, field.as_ref(), mid, pace);
            }
            k += 1;
        }

        // Shooting.
        if sw.shooting {
            self.shoot(r, tick);
        }

        // The runner.
        if let Some((at, p)) = r.runner
            && tick >= at
        {
            r.runner = None;
            self.word(r, tick, p, WordHow::Runner);
        }

        // At dawn whoever is still out goes home unseen.
        if dawn {
            for &i in &raiders {
                if matches!(r.task[i as usize], Task::Advance | Task::Withdraw | Task::Struggle) && self.soldiers[i as usize].gone.is_none() {
                    r.task[i as usize] = Task::Home;
                    self.stop_own(i);
                    self.soldiers[i as usize].hidden = true;
                }
            }
            r.struggles.clear();
        }

        // Over when nobody is out any more.
        let anyone_out = raiders.iter().any(|&i| self.soldiers[i as usize].gone.is_none() && matches!(r.task[i as usize], Task::Advance | Task::Withdraw | Task::Struggle | Task::Escort));
        if !anyone_out && r.prisoners.is_empty() {
            r.stage = Stage::Over { at: tick };
            r.record.over_at = Some(tick);
            r.runner = None;
            r.alert.fill(Alert::Quiet);
            if r.record.word.is_some() {
                self.note(Event::Raid(RaidNews::Over));
            }
        }
    }

    fn add_missing(r: &mut Raid, squad: u32) {
        match r.missing.iter_mut().find(|m| m.0 == squad) {
            Some(m) => m.1 += 1,
            None => r.missing.push((squad, 1)),
        }
    }

    /// Word reaches the command post (once).
    fn word(&mut self, r: &mut Raid, tick: u64, post: usize, how: WordHow) {
        if r.record.word.is_some() {
            return;
        }
        r.record.word = Some((tick, how));
        r.missing.clear();
        self.note(Event::Raid(RaidNews::Word { post, how }));
    }

    /// Post `p` sends up a flare toward the enemy trench.
    fn send_flare(&mut self, r: &mut Raid, tick: u64, p: usize) {
        if !r.tuning.switches.flares {
            return;
        }
        let ft = &r.tuning.flares;
        let pxy = (self.places[p].x, self.places[p].y);
        let mid = ((r.trench[0].0 + r.trench[1].0) / 2.0, (r.trench[0].1 + r.trench[1].1) / 2.0);
        let d = dist(pxy, mid).max(1.0);
        let k = (ft.burst_ahead_m / d).min(1.0);
        let at = (pxy.0 + (mid.0 - pxy.0) * k, pxy.1 + (mid.1 - pxy.1) * k);
        r.flares.push(Flare { x: at.0, y: at.1, until: tick + self.tuning.seconds_to_ticks(ft.burns_s), post: p });
        r.record.flares += 1;
        r.alarm.get_or_insert(tick);
        self.word(r, tick, p, WordHow::Flare);
    }

    /// Our man `g` is overpowered and walked away by (up to) the escort's number of `men`.
    fn take_prisoner(&mut self, r: &mut Raid, tick: u64, g: u32, men: Vec<u32>) {
        let squad = self.soldiers[g as usize].squad;
        // Out of his squad and out of sight lists: he no longer guards, tires or counts as ours.
        self.by_side[PLAYER_SIDE.index()].retain(|&m| m != g);
        self.squads[squad as usize].members.retain(|&m| m != g);
        self.soldiers[g as usize].sees.clear();
        r.task[g as usize] = Task::Captive;
        let n = r.tuning.withdrawal.escorts as usize;
        let escorts: Vec<u32> = men.iter().copied().take(n).collect();
        for &i in &men {
            r.task[i as usize] = if escorts.contains(&i) { Task::Escort } else { Task::Advance };
        }
        r.prisoners.push(Prisoner { man: g, squad, escorts });
        let _ = tick;
    }

    /// A prisoner whose escort was hit goes back to his squad.
    fn free_man(&mut self, r: &mut Raid, m: u32, squad: u32) {
        r.task[m as usize] = Task::None;
        let side = &mut self.by_side[PLAYER_SIDE.index()];
        let pos = side.partition_point(|&x| x < m);
        side.insert(pos, m);
        let q = &mut self.squads[squad as usize];
        let pos = q.members.partition_point(|&x| x < m);
        q.members.insert(pos, m);
        let goal = match q.post {
            Some((p, k)) => self.places[p].spot(k),
            None => {
                let s = &self.soldiers[m as usize];
                (s.x, s.y)
            }
        };
        self.send_own(m, None, goal, 1.0);
    }

    /// Soldier `i` is hit (or killed in a struggle): out of the game, lying where he fell till dawn.
    fn fall(&mut self, r: &mut Raid, i: u32, tick: u64, why: Lost) {
        let s = &self.soldiers[i as usize];
        r.fallen.push(Fallen { man: i, side: s.side, x: s.x, y: s.y });
        if s.side == Side::Italy {
            r.task[i as usize] = Task::None;
        }
        // An escort hit: drop him from the escort; a man in a struggle: from the struggle.
        for st in &mut r.struggles {
            st.raiders.retain(|&x| x != i);
        }
        for p in &mut r.prisoners {
            p.escorts.retain(|&x| x != i);
        }
        self.soldiers[i as usize].own = None;
        self.lose_man(i, why, tick);
    }

    /// Everyone allowed to fire and due a shot fires at the nearest enemy he notices within the
    /// hold-fire distance (decisions 72 and 75).
    /// The chance that a shot from `shooter` hits `target` now: by distance, by how well he sees
    /// the target, the target's cover, creeping, and the shooter's tiredness.
    fn shot_chance(&self, r: &Raid, shooter: u32, target: u32) -> f32 {
        let sh = &r.tuning.shooting;
        let s = &self.soldiers[shooter as usize];
        let en = &self.soldiers[target as usize];
        let d = dist((s.x, s.y), (en.x, en.y));
        let tf = self.tuning.tiredness.spotting_factor(s.tiredness);
        let hearing = if r.tuning.switches.hearing { r.tuning.hearing.within_m } else { 0.0 };
        let lit = r.flares.iter().any(|f| dist((f.x, f.y), (en.x, en.y)) <= r.tuning.flares.radius_m);
        let flare_m = if lit { self.sight_in(r.tuning.flares.light_lux) } else { 0.0 };
        let sight = (self.spotting_range.max(flare_m) * tf).max(hearing * tf).max(0.1);
        let seen = (1.0 - d / sight).max(0.0);
        let mut p = r.tuning.hit_chance(d) * seen;
        if r.tuning.switches.cover_at_posts && en.side == PLAYER_SIDE && self.in_post(target, sh.in_post_within_m) {
            p *= sh.post_cover_factor;
        }
        if en.side == Side::Italy && r.creeping[target as usize] {
            p *= sh.creeping_factor;
        }
        p * (1.0 - (1.0 - sh.exhausted_shooter_factor) * s.tiredness)
    }

    /// For checks: the chance a shot from `shooter` at `target` would hit now (None without a raid).
    pub fn chance_to_hit(&self, shooter: u32, target: u32) -> Option<f32> {
        self.raid.as_ref().map(|r| self.shot_chance(r, shooter, target))
    }

    fn shoot(&mut self, r: &mut Raid, tick: u64) {
        let sh = r.tuning.shooting.clone();
        let interval = self.tuning.seconds_to_ticks(sh.seconds_between_shots);
        let mut shooters: Vec<u32> = Vec::new();
        for &g in &self.by_side[PLAYER_SIDE.index()] {
            let s = &self.soldiers[g as usize];
            let q = &self.squads[s.squad as usize];
            if q.inside || r.in_struggle(g) {
                continue;
            }
            let at_firing_post = q.post.is_some_and(|(p, _)| r.alert[p] == Alert::Firing);
            if at_firing_post || r.gunfire.is_some() {
                shooters.push(g);
            }
        }
        if r.alarm.is_some() {
            for &i in &r.raiders {
                if matches!(r.task[i as usize], Task::Advance | Task::Withdraw) && self.soldiers[i as usize].gone.is_none() {
                    shooters.push(i);
                }
            }
        }
        for i in shooters {
            if self.soldiers[i as usize].gone.is_some() {
                continue;
            }
            let iu = i as usize;
            if r.next_shot[iu] == 0 {
                r.next_shot[iu] = tick + 1 + (r.rng.unit() * interval as f32) as u64;
                continue;
            }
            if tick < r.next_shot[iu] {
                continue;
            }
            let s = &self.soldiers[iu];
            let me = (s.x, s.y);
            let target = s
                .sees
                .iter()
                .copied()
                .filter(|&e| self.soldiers[e as usize].gone.is_none() && !r.captive(e))
                .filter(|&e| s.side != PLAYER_SIDE || r.is_raider(e))
                .map(|e| (dist(me, (self.soldiers[e as usize].x, self.soldiers[e as usize].y)), e))
                .filter(|&(d, _)| d <= sh.hold_fire_m)
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let Some((_, e)) = target else { continue };
            r.next_shot[iu] = tick + interval;
            r.record.shots += 1;
            if r.gunfire.is_none() {
                r.gunfire = Some(tick);
                r.alarm.get_or_insert(tick);
                let post = r.record.post.unwrap_or(0);
                self.word(r, tick, post, WordHow::Gunfire);
            }
            let p = self.shot_chance(r, i, e);
            let en = &self.soldiers[e as usize];
            if r.rng.unit() < p {
                if en.side == PLAYER_SIDE {
                    let squad = en.squad;
                    r.record.hit_ours.push((e, squad));
                } else {
                    r.record.raiders_hit += 1;
                }
                self.fall(r, e, tick, Lost::Hit);
            }
        }
    }

    /// Whether the men in the shelters are standing ready (awake) rather than asleep.
    pub fn standing_to(&self) -> bool {
        self.raid.as_ref().is_some_and(|r| r.tuning.switches.shelters_wake && r.gunfire.is_some() && r.out())
    }

    /// For the views: which men the player can see drawn now. Men inside shelters, hidden in the
    /// trench, or gone are not drawn; with the command-post rule, raiders (and men they have taken)
    /// are drawn only once word has reached the command post, and then raiders only while one of
    /// our men can see them. `everything` (the checking overlay) draws every man who is not gone,
    /// the hidden raiders included.
    pub fn drawn_men(&self, everything: bool) -> Vec<bool> {
        let n = self.soldiers.len();
        let mut seen_by_us = vec![false; n];
        for &g in &self.by_side[PLAYER_SIDE.index()] {
            for &e in &self.soldiers[g as usize].sees {
                seen_by_us[e as usize] = true;
            }
        }
        let rule = self.raid.as_ref().filter(|r| r.tuning.switches.word_must_reach_command_post);
        (0..n)
            .map(|i| {
                let s = &self.soldiers[i];
                if s.gone.is_some() {
                    return false;
                }
                if everything {
                    return true;
                }
                if s.hidden || self.squads[s.squad as usize].inside && !self.raid.as_ref().is_some_and(|r| r.captive(i as u32)) {
                    return false;
                }
                match &self.raid {
                    Some(r) if r.captive(i as u32) => rule.is_none_or(|r| r.known()),
                    Some(r) if r.is_raider(i as u32) => match rule {
                        Some(r) => r.known() && seen_by_us[i],
                        None => true,
                    },
                    _ => true,
                }
            })
            .collect()
    }

    /// For the views: for each man, whether an enemy can see him, as the player may know it (a
    /// raider seeing one of our men does not show).
    pub fn seen_by_enemy_shown(&self) -> Vec<bool> {
        let mut seen = vec![false; self.soldiers.len()];
        for s in &self.soldiers {
            if self.raid.as_ref().is_some_and(|r| r.is_raider(s.id)) {
                continue;
            }
            for &e in &s.sees {
                seen[e as usize] = true;
            }
        }
        seen
    }
}
