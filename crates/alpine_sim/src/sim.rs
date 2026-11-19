//! The simulation: soldiers, squads, orders, and the fixed update order.
//!
//! Every update (tick) runs these stages in this order:
//! 0. with a calendar (step 2 on): the clock moves on, the light and spotting distance are worked
//!    out for the new moment, and dawn and dusk are noted;
//! 1. with a work roster: at dawn and dusk every rostered squad takes up its duty for the half of
//!    the day now beginning (and, with the supply file, the men eat); then orders scheduled for this
//!    tick are given to their squads; carrying squads load, unload and set off again; and once a game
//!    minute the player's men tire or recover (with the supply file also: fuel burns, men grow cold
//!    or warm up and hungrier, and the very cold may get frostbite or die); with the snow file, once
//!    a game minute fresh snow falls and settles;
//! 2. patrolling squads that have waited long enough pick a new destination;
//! 3. squads whose route is ready start moving;
//! 4. every soldier moves (split across threads; each man reads only the previous state); with the
//!    snow file, then, in soldier order, each man who moved packs the fresh snow where he walked;
//! 5. squads whose men have all arrived are marked arrived;
//! 6. the soldiers due to look around this tick check which enemies they can see (split across
//!    threads);
//! 7. with a calendar: enemy squads our men see that nobody had seen for a while are noted as new
//!    contacts.
//!
//! Events (dawn, new contacts, arrivals) are only noted with a calendar, so a scenario without one
//! (step 1) behaves, and checksums, exactly as before.
//!
//! Randomness comes only from the seeded generator, used only on the main thread, in squad order.
//! Parallel work is collected back in soldier order, so 1 thread or 8 give the same result.

use crate::flow::FlowField;
use crate::hash::Checksum;
use crate::los::{self, LosParams};
use crate::rng::Rng;
use crate::roster::{Duty, Place, PlaceKind, Roster};
use crate::sky::{self, Calendar, SkyState};
use crate::spatial::SpatialGrid;
use crate::snow::{Forecast, RouteSnow, Snow, SnowFile};
use crate::supply::{CarryLeg, ColdTuning, Lost, LostMan, Stock, Supply, SupplyFile};
use crate::terrain::Terrain;
use crate::tuning::Tuning;
use crate::weather::Weather;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Side {
    Austria,
    Italy,
}

impl Side {
    pub fn enemy(self) -> Side {
        match self {
            Side::Austria => Side::Italy,
            Side::Italy => Side::Austria,
        }
    }
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        match self {
            Side::Austria => "Austro-Hungarian",
            Side::Italy => "Italian",
        }
    }
}

/// The side the player commands.
pub const PLAYER_SIDE: Side = Side::Austria;

/// Something that happened, worth telling the player (and stopping the skip-ahead for).
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// First light: the daily orders moment (decision 37). Day 0 is the morning the game opens.
    Dawn { day: u32 },
    /// The end of civil twilight.
    Dusk,
    /// Our men see an enemy squad that nobody had seen for a while.
    Contact { enemy_squad: u32, seen_by_squad: u32, distance_m: f32 },
    /// A squad the player ordered has arrived.
    Arrived { squad: u32 },
    /// A squad the player ordered cannot get where it was sent.
    Unreachable { squad: u32 },
}

impl Event {
    /// Whether this event stops the skip-ahead and slows normal play (decision 36).
    pub fn stops_time(&self) -> bool {
        !matches!(self, Event::Dusk)
    }
}

/// An event and the update after which it was noted (the simulation's `tick` once that update ran).
#[derive(Clone, Debug, PartialEq)]
pub struct LoggedEvent {
    pub tick: u64,
    pub event: Event,
}

/// The game clock and the sky (scenarios with a calendar only).
#[derive(Clone, Debug)]
pub struct Clock {
    pub calendar: Calendar,
    pub weather: Weather,
    /// The sky at the current moment.
    pub sky: SkyState,
    /// Mornings that have begun since the game opened (0 on the first morning).
    pub day: u32,
    /// Between dusk and dawn: the work roster's night duties apply.
    pub night: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Order {
    /// Send a squad to a place (metres from the summit). A squad on the roster leaves its duty
    /// until the next dusk or dawn (decision 42).
    Move { squad: u32, x: f32, y: f32 },
    /// Set a squad's duties on the work roster. Takes effect at once for the half of the day now
    /// running, unless the squad has been sent elsewhere.
    SetRoster { squad: u32, day: Duty, night: Duty },
    /// Send a squad that was sent elsewhere back to its rostered duty now.
    BackToDuty { squad: u32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoggedOrder {
    pub tick: u64,
    pub order: Order,
}

#[derive(Clone, Debug)]
pub struct Soldier {
    pub id: u32,
    pub side: Side,
    pub squad: u32,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// Direction faced, radians counter-clockwise from east.
    pub facing: f32,
    /// His place in the squad's formation at the destination.
    pub slot: (f32, f32),
    pub arrived: bool,
    pub waypoint: Option<(f32, f32)>,
    /// Ticks in a row he has been on the move but not moved.
    pub still_ticks: u32,
    /// Enemies he can currently see, by soldier id, ascending.
    pub sees: Vec<u32>,
    /// How tired he is: 0 fresh, 1 exhausted (games with a work roster; decision 43).
    pub tiredness: f32,
    /// How cold he is: 0 warm, 1 the worst (games with the supply file; decision 51).
    pub chill: f32,
    /// Game minutes since his last meal (games with the supply file).
    pub minutes_since_meal: u32,
    /// Game minutes in a row he has been outside at the worst cold.
    pub worst_minutes: u32,
    /// Lost to the company: sent down with frostbite, or dead. He is out of the game.
    pub gone: Option<Lost>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SquadState {
    Idle,
    /// Working out the route; starts moving at this tick.
    Planning { ready_tick: u64 },
    Moving,
    Arrived,
    /// The destination cannot be reached from where the squad stands.
    Unreachable,
}

/// Where a patrolling squad picks its destinations: a ring around the summit.
#[derive(Clone, Copy, Debug)]
pub struct PatrolZone {
    pub min_r: f32,
    pub max_r: f32,
    pub max_height: f32,
    pub south_only: bool,
}

pub struct Squad {
    pub id: u32,
    pub side: Side,
    pub members: Vec<u32>,
    pub state: SquadState,
    pub dest: Option<(f32, f32)>,
    pub dest_point: Option<(usize, usize)>,
    pub field: Option<Arc<FlowField>>,
    pub order_tick: u64,
    pub arrived_tick: Option<u64>,
    pub patrol: Option<PatrolZone>,
    pub next_patrol_tick: u64,
    /// When one of the player's men last saw a man of this squad (enemy squads only).
    pub last_seen_by_player: Option<u64>,
    /// Its duties on the work roster (the player's squads, in games with one).
    pub roster: Option<Roster>,
    /// Sent elsewhere by the player: off duty until the next dusk or dawn, or "Back to duty".
    pub detached: bool,
    /// While on duty: the place (an index into `Sim::places`) and the spot there it is going to or
    /// standing at.
    pub post: Option<(usize, usize)>,
    /// Inside its shelter: it sees nothing, cannot be seen, and its men sleep.
    pub inside: bool,
    /// The current move was made by the roster, so its arrival is not worth a message.
    pub roster_move: bool,
    /// What it has done since the latest dawn, and what it did in the 24 hours before that.
    pub today: DayRecord,
    pub yesterday: Option<DayRecord>,
    /// On carry duty: where it is in its round (games with the supply file).
    pub carry: Option<CarryLeg>,
    /// Its men walk at this share of their speed (carrying loads, carrying in the dark); 1 otherwise.
    pub pace: f32,
    /// Its men have snowshoes (games with the snow file; decisions 58 and 59).
    pub snowshoes: bool,
}

/// What a squad on the roster did in a day, for the morning report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DayRecord {
    /// Game minutes standing at a lookout post, and asleep inside a shelter.
    pub guard_minutes: u32,
    pub sleep_minutes: u32,
    /// Sent elsewhere by the player at some point.
    pub sent_away: bool,
    /// Game minutes on carry duty, walking or loading (games with the supply file).
    pub carry_minutes: u32,
}

/// A route calculation running on a worker thread.
struct FieldJob {
    result: Mutex<Option<Arc<FlowField>>>,
    ready: Condvar,
}

impl FieldJob {
    fn wait(&self) -> Arc<FlowField> {
        let mut g = self.result.lock().unwrap();
        while g.is_none() {
            g = self.ready.wait(g).unwrap();
        }
        g.clone().unwrap()
    }
}

/// A man within this distance of his route point moves on to the next one.
const WAYPOINT_REACHED_M: f32 = 1.5;
/// Moving less than this in a tick counts as standing still (1 cm per second at 10 ticks a second).
/// The slowest walk on bare ground, straight up a 45° slope, is about 3 cm per second; since falling
/// snow, a tired, cold, loaded man in deep snow on such ground can be slower than this, and is then
/// treated as held up (he ignores personal space and re-joins his route). Harmless so far: every
/// trip in a played week completed. A number to watch when tuning.
const STILL_BELOW_M_PER_TICK: f32 = 0.001;
/// After this many ticks standing still a man ignores personal space until he moves again.
const IGNORE_SPACE_AFTER_TICKS: u32 = 10;
/// After every this many ticks standing still a man re-joins his route from where he is.
const STUCK_REJOIN_TICKS: u32 = 30;

pub struct Sim {
    pub tick: u64,
    pub seed: u64,
    pub terrain: Arc<Terrain>,
    pub tuning: Tuning,
    pub los: LosParams,
    pub soldiers: Vec<Soldier>,
    pub squads: Vec<Squad>,
    /// Every order given from outside, with the tick it took effect: the replay's order log.
    pub order_log: Vec<LoggedOrder>,
    /// The calendar and sky, for scenarios that have one (step 2 on).
    pub clock: Option<Clock>,
    /// How far a man can be spotted right now (the tuning's daylight distance without a calendar).
    pub spotting_range: f32,
    /// Events noted during the latest update.
    pub events: Vec<Event>,
    /// Every event since the start, in order.
    pub journal: Vec<LoggedEvent>,
    /// The lookout posts and shelters of the work roster (empty without one), then, with the supply
    /// file, the dump and the store.
    pub places: Vec<Place>,
    /// The summit's stocks, cold and hunger (games with the supply file; decisions 45 to 53).
    pub supply: Option<Supply>,
    /// Fresh snow on the map, and the forecast (games with the snow file; decisions 57 to 67).
    pub snow: Option<Snow>,
    rng: Rng,
    pool: Arc<rayon::ThreadPool>,
    scheduled: VecDeque<LoggedOrder>,
    /// Route calculations under way, by squad.
    jobs: BTreeMap<u32, Arc<FieldJob>>,
    grid: SpatialGrid,
    by_side: [Vec<u32>; 2],
    formation: Vec<(f32, f32)>,
}

pub fn make_pool(threads: usize) -> Arc<rayon::ThreadPool> {
    Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads.max(1))
            .thread_name(|i| format!("sim-worker-{i}"))
            .build()
            .expect("thread pool"),
    )
}

impl Sim {
    pub fn new(terrain: Arc<Terrain>, tuning: Tuning, seed: u64, pool: Arc<rayon::ThreadPool>) -> Sim {
        let spacing = tuning.squad.formation_spacing_m;
        let mut formation = Vec::new();
        for j in -6i32..=6 {
            for i in -6i32..=6 {
                formation.push((i, j));
            }
        }
        formation.sort_by_key(|&(i, j)| (i * i + j * j, j, i));
        let formation = formation.into_iter().map(|(i, j)| (i as f32 * spacing, j as f32 * spacing)).collect();
        Sim {
            tick: 0,
            seed,
            clock: None,
            spotting_range: tuning.spotting.spotting_range_m,
            events: Vec::new(),
            journal: Vec::new(),
            places: Vec::new(),
            supply: None,
            snow: None,
            los: LosParams::from_tuning(&tuning.line_of_sight),
            terrain,
            tuning,
            soldiers: Vec::new(),
            squads: Vec::new(),
            order_log: Vec::new(),
            rng: Rng::new(seed),
            pool,
            scheduled: VecDeque::new(),
            jobs: BTreeMap::new(),
            grid: SpatialGrid::new(4.0),
            by_side: [Vec::new(), Vec::new()],
            formation,
        }
    }

    /// Puts the simulation on the calendar: update 0 is the calendar's start. The light, and with it
    /// the spotting distance, follow the sun, the moon and the weather from now on.
    pub fn set_clock(&mut self, calendar: Calendar, weather: Weather) {
        let t = calendar.start;
        let sky = calendar.sky(t, weather.cloud_at(t), &self.tuning.light);
        let night = sky.sun_altitude_deg < sky::DAWN_ALTITUDE_DEG;
        self.clock = Some(Clock { calendar, weather, sky, day: 0, night });
        self.spotting_range = sky::spotting_distance(sky.lux, &self.tuning.light, self.tuning.spotting.spotting_range_m);
    }

    /// Notes an event as having happened in the update now running.
    fn note(&mut self, event: Event) {
        self.journal.push(LoggedEvent { tick: self.tick + 1, event: event.clone() });
        self.events.push(event);
    }

    /// Whether events about this squad are worth telling the player: one of his own, under his
    /// orders rather than wandering on a patrol or walking to its roster place, in a game with a
    /// calendar.
    fn player_ordered(&self, squad: u32) -> bool {
        let q = &self.squads[squad as usize];
        self.clock.is_some() && q.side == PLAYER_SIDE && q.patrol.is_none() && !q.roster_move
    }

    pub fn rng(&mut self) -> &mut Rng {
        &mut self.rng
    }

    pub fn pool(&self) -> &Arc<rayon::ThreadPool> {
        &self.pool
    }

    /// Adds a squad standing at the given positions (one man each), idle.
    pub fn add_squad(&mut self, side: Side, positions: &[(f32, f32)]) -> u32 {
        let sid = self.squads.len() as u32;
        let mut members = Vec::new();
        for &(x, y) in positions {
            let id = self.soldiers.len() as u32;
            members.push(id);
            self.by_side[side.index()].push(id);
            self.soldiers.push(Soldier {
                id,
                side,
                squad: sid,
                x,
                y,
                vx: 0.0,
                vy: 0.0,
                facing: 0.0,
                slot: (x, y),
                arrived: true,
                waypoint: None,
                still_ticks: 0,
                sees: Vec::new(),
                tiredness: 0.0,
                chill: 0.0,
                minutes_since_meal: 0,
                worst_minutes: 0,
                gone: None,
            });
        }
        self.squads.push(Squad {
            id: sid,
            side,
            members,
            state: SquadState::Idle,
            dest: None,
            dest_point: None,
            field: None,
            order_tick: 0,
            arrived_tick: None,
            patrol: None,
            next_patrol_tick: u64::MAX,
            last_seen_by_player: None,
            roster: None,
            detached: false,
            post: None,
            inside: false,
            roster_move: false,
            today: DayRecord::default(),
            yesterday: None,
            carry: None,
            pace: 1.0,
            snowshoes: false,
        });
        self.grid.rebuild(self.soldiers.iter().map(|s| (s.x, s.y)));
        sid
    }

    /// Formation places around a point, `n` of them, each on ground that can be stood on and
    /// reachable in a straight line from the point.
    pub fn formation_around(&self, x: f32, y: f32, n: usize) -> Vec<(f32, f32)> {
        let t = &self.terrain;
        let mut out: Vec<(f32, f32)> = self
            .formation
            .iter()
            .map(|&(dx, dy)| (x + dx, y + dy))
            .filter(|&(px, py)| t.walkable_at(px, py) && t.segment_walkable(x, y, px, py))
            .take(n)
            .collect();
        while out.len() < n {
            out.push((x, y));
        }
        out
    }

    /// Makes a squad pick its own destinations within a zone, pausing between them.
    pub fn set_patrol(&mut self, squad: u32, zone: PatrolZone, first_move_tick: u64) {
        let s = &mut self.squads[squad as usize];
        s.patrol = Some(zone);
        s.next_patrol_tick = first_move_tick;
    }

    /// Gives an order from outside (the player or a replay). It takes effect at the next update and
    /// is written to the order log.
    pub fn order(&mut self, order: Order) {
        self.schedule(LoggedOrder { tick: self.tick, order });
    }

    /// Schedules an order for a given tick (not in the past).
    pub fn schedule(&mut self, o: LoggedOrder) {
        assert!(o.tick >= self.tick, "order scheduled in the past");
        let pos = self.scheduled.partition_point(|e| e.tick <= o.tick);
        self.scheduled.insert(pos, o);
    }

    pub fn game_seconds(&self) -> f64 {
        self.tick as f64 / self.tuning.time.ticks_per_second as f64
    }

    pub fn ground(&self, i: u32) -> [f32; 3] {
        let s = &self.soldiers[i as usize];
        [s.x, s.y, self.terrain.height(s.x, s.y)]
    }

    // ---- routes --------------------------------------------------------------------------------

    /// Starts working out a squad's route on a worker thread. It covers the area around the squad's
    /// men and the way to the destination, not the whole map. (Men stand still while their route is
    /// worked out, so where they stand now is where they start from.)
    fn request_field(&mut self, squad: u32, dp: (usize, usize)) {
        let t = &self.terrain;
        let mut from = Vec::new();
        for &m in &self.squads[squad as usize].members {
            let s = &self.soldiers[m as usize];
            let (c0, r0) = t.nearest_point(s.x, s.y);
            for dr in -1i32..=1 {
                for dc in -1i32..=1 {
                    let (c, r) = (c0 as i32 + dc, r0 as i32 + dr);
                    if c >= 0 && r >= 0 && c < t.cols as i32 && r < t.rows as i32 {
                        from.push((c as usize, r as usize));
                    }
                }
            }
        }
        let job = Arc::new(FieldJob { result: Mutex::new(None), ready: Condvar::new() });
        self.jobs.insert(squad, job.clone());
        let terrain = self.terrain.clone();
        let walking = self.tuning.walking.clone();
        let snow = self.route_snow(squad);
        self.pool.spawn(move || {
            let f = Arc::new(FlowField::compute_bounded(&terrain, &walking, dp, &from, snow));
            *job.result.lock().unwrap() = Some(f);
            job.ready.notify_all();
        });
    }

    /// What squad `squad`'s route should know of the snow (decision 67): the fresh snow as at the
    /// latest game minute and its men's footwear; None where routes do not see snow.
    fn route_snow(&self, squad: u32) -> Option<RouteSnow> {
        let sn = self.snow.as_ref().filter(|s| s.routes_see_snow())?;
        let shoes = self.squads[squad as usize].snowshoes && sn.tuning.switches.snowshoes;
        let table = if shoes { sn.tuning.walking.snowshoes.clone() } else { sn.tuning.walking.on_foot.clone() };
        Some(RouteSnow { loose: sn.for_routes.clone(), table })
    }

    /// The squad's route, waiting for the worker thread if it is not finished yet.
    fn take_field(&mut self, squad: u32, dp: (usize, usize)) -> Arc<FlowField> {
        if !self.jobs.contains_key(&squad) {
            self.request_field(squad, dp);
        }
        self.jobs.remove(&squad).unwrap().wait()
    }

    fn start_move(&mut self, squad: u32, x: f32, y: f32) {
        let tick = self.tick;
        let delay = self.tuning.seconds_to_ticks(self.tuning.squad.order_delay_s);
        let dp = self.terrain.nearest_main_point(x, y, 30);
        let sq = &mut self.squads[squad as usize];
        sq.order_tick = tick;
        sq.arrived_tick = None;
        sq.field = None;
        sq.inside = false;
        let Some(dp) = dp else {
            sq.state = SquadState::Unreachable;
            sq.dest = None;
            if self.player_ordered(squad) {
                self.note(Event::Unreachable { squad });
            }
            return;
        };
        sq.dest_point = Some(dp);
        sq.dest = Some(self.terrain.point_xy(dp.0, dp.1));
        sq.state = SquadState::Planning { ready_tick: tick + delay };
        for &m in &sq.members.clone() {
            let s = &mut self.soldiers[m as usize];
            s.waypoint = None;
            s.still_ticks = 0;
        }
        self.request_field(squad, dp);
    }

    fn begin_moving(&mut self, squad: u32) {
        let dp = self.squads[squad as usize].dest_point.unwrap();
        let field = self.take_field(squad, dp);
        let (dx, dy) = self.terrain.point_xy(dp.0, dp.1);
        let members = self.squads[squad as usize].members.clone();
        let reachable = members.iter().any(|&m| {
            let s = &self.soldiers[m as usize];
            field.time_from(&self.terrain, &self.tuning.walking, s.x, s.y).is_finite()
        });
        if !reachable {
            let sq = &mut self.squads[squad as usize];
            sq.state = SquadState::Unreachable;
            if self.player_ordered(squad) {
                self.note(Event::Unreachable { squad });
            }
            return;
        }
        let slots = self.formation_around(dx, dy, members.len());
        for (k, &m) in members.iter().enumerate() {
            let s = &mut self.soldiers[m as usize];
            s.slot = slots[k];
            s.arrived = false;
            s.waypoint = None;
            s.still_ticks = 0;
        }
        let sq = &mut self.squads[squad as usize];
        sq.field = Some(field);
        sq.state = SquadState::Moving;
    }

    fn pick_patrol_destination(&mut self, zone: PatrolZone) -> Option<(f32, f32)> {
        for _ in 0..200 {
            let a = if zone.south_only {
                self.rng.range(std::f32::consts::PI * 1.1, std::f32::consts::PI * 1.9)
            } else {
                self.rng.range(0.0, std::f32::consts::TAU)
            };
            let r = self.rng.range(zone.min_r, zone.max_r);
            let (x, y) = (r * a.cos(), r * a.sin());
            if self.terrain.in_main_region(x, y) && self.terrain.height(x, y) <= zone.max_height {
                return Some((x, y));
            }
        }
        None
    }

    fn patrol_pause_ticks(&mut self) -> u64 {
        let sc = &self.tuning.scenario;
        let (lo, hi) = (sc.patrol_pause_min_minutes, sc.patrol_pause_max_minutes);
        let minutes = self.rng.range(lo, hi);
        self.tuning.seconds_to_ticks(minutes * 60.0)
    }

    // ---- the work roster -----------------------------------------------------------------------

    /// The duties the player has set for squad `i` but that have not taken effect yet (orders given
    /// while time stands still take effect at the next update), else its current duties.
    pub fn roster_shown(&self, i: usize) -> Option<Roster> {
        let pending = self.scheduled.iter().rev().find_map(|o| match o.order {
            Order::SetRoster { squad, day, night } if squad as usize == i => Some(Roster { day, night }),
            _ => None,
        });
        pending.or(self.squads.get(i)?.roster)
    }

    /// Whether a "Back to duty" for squad `i` is waiting for the next update.
    pub fn back_to_duty_pending(&self, i: usize) -> bool {
        self.scheduled.iter().any(|o| matches!(o.order, Order::BackToDuty { squad } if squad as usize == i))
    }

    /// The duty squad `i` should be doing now, if it is on the roster and not sent elsewhere.
    pub fn duty_now(&self, i: usize) -> Option<Duty> {
        let q = &self.squads[i];
        let night = self.clock.as_ref().is_some_and(|c| c.night);
        if q.detached {
            return None;
        }
        // Carrying needs the supply file with carrying switched on; otherwise a squad set to carry
        // rests instead.
        q.roster.map(|r| match r.duty(night) {
            Duty::Carry if !self.carrying_on() => Duty::Rest,
            d => d,
        })
    }

    /// Whether carrying parties exist in this game: the supply file, with carrying switched on.
    pub fn carrying_on(&self) -> bool {
        self.supply.as_ref().is_some_and(|s| s.tuning.switches.carrying)
    }

    /// Picks a place and spot for squad `i`'s current duty: the first free regular spot at a place
    /// of the right kind, in the places file's order; when those are all taken, a free spare spot
    /// at the place of that kind with the fewest squads; only when those are gone too does it share
    /// a spot.
    fn choose_spot(&self, i: usize, kind: PlaceKind) -> Option<(usize, usize)> {
        let mut used: Vec<Vec<u32>> = self.places.iter().map(|p| vec![0; p.spot_count()]).collect();
        for (j, q) in self.squads.iter().enumerate() {
            if let Some((p, k)) = q.post
                && j != i
            {
                used[p][k] += 1;
            }
        }
        let of_kind = || self.places.iter().enumerate().filter(move |(_, p)| p.kind == kind).map(|(n, _)| n);
        for p in of_kind() {
            if let Some(k) = used[p][..self.places[p].spots.len()].iter().position(|&u| u == 0) {
                return Some((p, k));
            }
        }
        let regular = |p: usize| self.places[p].spots.len();
        let spare = of_kind()
            .filter_map(|p| used[p][regular(p)..].iter().position(|&u| u == 0).map(|k| (p, regular(p) + k)))
            .min_by_key(|&(p, _)| (used[p].iter().sum::<u32>(), p));
        if spare.is_some() {
            return spare;
        }
        let p = of_kind().min_by_key(|&p| (used[p].iter().sum::<u32>(), p))?;
        let k = used[p].iter().enumerate().min_by_key(|&(k, &u)| (u, k)).map(|(k, _)| k)?;
        Some((p, k))
    }

    /// Squad `i` takes up its current duty: stays if it already has a place of the right kind,
    /// otherwise is given one and walks there.
    fn take_up_duty(&mut self, i: usize) {
        let Some(duty) = self.duty_now(i) else { return };
        if self.squads[i].members.is_empty() {
            return;
        }
        if duty == Duty::Carry {
            self.take_up_carrying(i);
            return;
        }
        self.squads[i].carry = None;
        self.squads[i].pace = 1.0;
        let kind = duty.place_kind();
        if let Some((p, _)) = self.squads[i].post
            && self.places[p].kind == kind
        {
            return;
        }
        self.send_to(i, kind);
    }

    /// Sends squad `i` to a free spot at a place of `kind`, as a roster move (no arrival message).
    fn send_to(&mut self, i: usize, kind: PlaceKind) {
        self.squads[i].post = None;
        let Some((p, k)) = self.choose_spot(i, kind) else { return };
        let (x, y) = self.places[p].spot(k);
        let q = &mut self.squads[i];
        q.post = Some((p, k));
        q.roster_move = true;
        self.start_move(i as u32, x, y);
    }

    /// Whether squad `i` is at, or walking to, a place of `kind`.
    fn post_kind(&self, i: usize) -> Option<PlaceKind> {
        self.squads[i].post.map(|(p, _)| self.places[p].kind)
    }

    /// Squad `i` takes up carrying (decision 49): it carries on with a round already under way;
    /// otherwise it sets off for the dump, unless the store is full, when it rests in a shelter.
    fn take_up_carrying(&mut self, i: usize) {
        let at = self.post_kind(i);
        match self.squads[i].carry {
            Some(CarryLeg::Done) | None => {}
            Some(_) if matches!(at, Some(PlaceKind::Dump | PlaceKind::Store)) => return,
            Some(_) => {}
        }
        let full = self.supply.as_ref().is_some_and(Supply::full);
        if full {
            self.squads[i].carry = Some(CarryLeg::Done);
            self.squads[i].pace = 1.0;
            if at != Some(PlaceKind::Shelter) {
                self.send_to(i, PlaceKind::Shelter);
            }
        } else {
            self.squads[i].carry = Some(CarryLeg::ToDump);
            self.set_pace(i);
            self.send_to(i, PlaceKind::Dump);
        }
    }

    /// Sets squad `i`'s walking pace from its carrying: slower loaded, slower again in the dark.
    fn set_pace(&mut self, i: usize) {
        let Some(sup) = &self.supply else { return };
        let night = self.clock.as_ref().is_some_and(|c| c.night);
        let ct = &sup.tuning.carrying;
        let q = &mut self.squads[i];
        q.pace = match q.carry {
            None | Some(CarryLeg::Done) => 1.0,
            Some(leg) => {
                let laden = matches!(leg, CarryLeg::ToStore { .. } | CarryLeg::Unloading { .. });
                (if laden { ct.laden_walking_factor } else { 1.0 }) * (if night { ct.night_walking_factor } else { 1.0 })
            }
        };
    }

    /// Carrying squads that have finished loading or unloading move on (once per update).
    fn advance_carrying(&mut self, tick: u64) {
        if self.supply.is_none() {
            return;
        }
        for i in 0..self.squads.len() {
            match self.squads[i].carry {
                Some(CarryLeg::Loading { until }) if until <= tick => {
                    let stock = self.supply.as_ref().unwrap().lower();
                    self.squads[i].carry = Some(CarryLeg::ToStore { stock });
                    self.set_pace(i);
                    self.send_to(i, PlaceKind::Store);
                }
                Some(CarryLeg::Unloading { until, stock }) if until <= tick => {
                    let men = self.squads[i].members.len() as f32;
                    let sup = self.supply.as_mut().unwrap();
                    sup.unload(stock, men * sup.tuning.carrying.load_kg);
                    if sup.full() {
                        self.squads[i].carry = Some(CarryLeg::Done);
                        self.set_pace(i);
                        self.send_to(i, PlaceKind::Shelter);
                    } else {
                        self.squads[i].carry = Some(CarryLeg::ToDump);
                        self.set_pace(i);
                        self.send_to(i, PlaceKind::Dump);
                    }
                }
                _ => {}
            }
        }
    }

    /// A carrying squad has arrived at the dump or the store: it starts loading or unloading.
    fn carrier_arrived(&mut self, i: usize, tick: u64) {
        let Some(sup) = &self.supply else { return };
        let ct = &sup.tuning.carrying;
        let (load, unload) = (self.tuning.seconds_to_ticks(ct.loading_minutes * 60.0), self.tuning.seconds_to_ticks(ct.unloading_minutes * 60.0));
        let at = self.post_kind(i);
        let q = &mut self.squads[i];
        q.carry = match (q.carry, at) {
            (Some(CarryLeg::ToDump), Some(PlaceKind::Dump)) => Some(CarryLeg::Loading { until: tick + load }),
            (Some(CarryLeg::ToStore { stock }), Some(PlaceKind::Store)) => Some(CarryLeg::Unloading { until: tick + unload, stock }),
            (c, _) => c,
        };
    }

    /// Puts the supply file in force (decisions 45 to 53), in a game with a work roster: adds the dump
    /// and the store to the places, and fills the store for the men on the summit now.
    pub fn start_supply(&mut self, file: SupplyFile) -> Result<(), String> {
        if self.places.is_empty() {
            return Err("the supply file needs a game with a work roster".into());
        }
        let (dump, store) = crate::supply::places(&file, &self.terrain, &self.places)?;
        self.places.push(dump);
        self.places.push(store);
        let men = self.by_side[PLAYER_SIDE.index()].len() as f32;
        let st = &file.stocks;
        let (food_per_day_kg, fuel_per_day_kg) = (men * st.food_kg_per_man_day, men * st.fuel_kg_per_man_day);
        self.supply = Some(Supply {
            food_kg: food_per_day_kg * st.store_days,
            fuel_kg: fuel_per_day_kg * st.store_days,
            food_cap_kg: food_per_day_kg * st.store_days,
            fuel_cap_kg: fuel_per_day_kg * st.store_days,
            food_per_day_kg,
            fuel_per_day_kg,
            dump: self.places.len() - 2,
            store: self.places.len() - 1,
            lost: Vec::new(),
            carried_today: [0.0; 2],
            carried_yesterday: None,
            missed_meals_today: 0,
            missed_meals_yesterday: None,
            tuning: file,
        });
        // Squads set to carry now set off.
        for i in 0..self.squads.len() {
            if self.duty_now(i) == Some(Duty::Carry) {
                self.take_up_duty(i);
            }
        }
        Ok(())
    }

    /// A meal, at dawn and at dusk: each of the player's men eats half a day's food, while the store
    /// has it, in soldier order.
    fn serve_meal(&mut self) {
        let Some(sup) = self.supply.as_mut() else { return };
        // With hunger switched off nobody needs food, so none is eaten.
        if !sup.tuning.switches.hunger {
            return;
        }
        let ration = sup.tuning.stocks.food_kg_per_man_day * 0.5;
        let mut missed = false;
        for &i in &self.by_side[PLAYER_SIDE.index()] {
            let s = &mut self.soldiers[i as usize];
            if sup.food_kg >= ration - 1e-4 {
                sup.food_kg = (sup.food_kg - ration).max(0.0);
                s.minutes_since_meal = 0;
            } else {
                missed = true;
            }
        }
        if missed {
            sup.missed_meals_today += 1;
        }
    }

    /// Soldier `i` leaves the company: sent down with frostbite, or dead.
    fn lose_man(&mut self, i: u32, why: Lost, tick: u64) {
        let s = &mut self.soldiers[i as usize];
        s.gone = Some(why);
        s.sees.clear();
        s.arrived = true;
        s.waypoint = None;
        let (squad, side) = (s.squad, s.side);
        self.by_side[side.index()].retain(|&m| m != i);
        let q = &mut self.squads[squad as usize];
        q.members.retain(|&m| m != i);
        if q.members.is_empty() {
            q.post = None;
            q.carry = None;
            q.inside = false;
            q.state = SquadState::Idle;
            q.field = None;
        }
        if let Some(sup) = self.supply.as_mut() {
            sup.lost.push(LostMan { tick: tick + 1, soldier: i, squad, why });
        }
    }

    /// Once a game minute with the supply file: fuel burns, the player's men grow cold or warm up and
    /// grow hungrier, and the very cold may get frostbite or die (decisions 48, 51 and 52).
    fn supply_minute(&mut self, tick: u64) {
        let Some(sup) = self.supply.as_mut() else { return };
        let (cold_on, hunger_on) = (sup.tuning.switches.cold, sup.tuning.switches.hunger);
        let men = self.by_side[PLAYER_SIDE.index()].len() as f32;
        // With cold switched off nobody needs fuel, so none burns.
        if cold_on {
            sup.fuel_kg = (sup.fuel_kg - men * sup.tuning.stocks.fuel_kg_per_man_day / (24.0 * 60.0)).max(0.0);
        }
        let heated = sup.fuel_kg > 0.0;
        let celsius = self.clock.as_ref().map_or(crate::weather::DEFAULT_CELSIUS, |c| {
            c.weather.celsius_at(c.calendar.local_seconds(tick as f64 * self.tuning.dt() as f64))
        }) as f32;
        let (ct, ht) = (&sup.tuning.cold, &sup.tuning.hunger);
        let mut losses = Vec::new();
        for &i in &self.by_side[PLAYER_SIDE.index()] {
            let s = &mut self.soldiers[i as usize];
            if hunger_on {
                s.minutes_since_meal = s.minutes_since_meal.saturating_add(1);
            }
            if !cold_on {
                continue;
            }
            let inside = self.squads[s.squad as usize].inside;
            let hunger = ht.hunger(s.minutes_since_meal.saturating_sub(1));
            if inside {
                if heated {
                    s.chill -= ct.warming_per_hour / 60.0 * ht.scale(hunger, ht.warming_factor);
                }
            } else {
                let moving = if s.arrived { 1.0 } else { ct.moving_factor };
                s.chill += ct.per_degree_hour * (-celsius).max(0.0) / 60.0 * moving * ht.scale(hunger, ht.chilling_factor);
            }
            s.chill = s.chill.clamp(0.0, 1.0);
            s.worst_minutes = if !inside && s.chill >= 0.999 { s.worst_minutes + 1 } else { 0 };
            if !inside {
                if s.worst_minutes as f32 >= ct.death_after_hours_at_worst * 60.0 {
                    losses.push((i, Lost::Died, ct.death_chance_per_hour / 60.0));
                } else if s.chill >= ct.frostbite_from {
                    losses.push((i, Lost::Frostbite, ct.frostbite_per_hour(s.chill) / 60.0));
                }
            }
        }
        for (i, why, p) in losses {
            if self.rng.unit() < p {
                self.lose_man(i, why, tick);
            }
        }
    }

    /// Puts the snow file in force (decisions 57 to 67), in a game with a calendar: fresh snow from
    /// now on, the snowshoe squads, and the first morning's forecast. With snowfall switched off the
    /// whole snow piece is off, as if there were no snow file.
    pub fn start_snow(&mut self, file: SnowFile) -> Result<(), String> {
        if self.clock.is_none() {
            return Err("the snow file needs a game with a calendar".into());
        }
        if !file.switches.snowfall {
            return Ok(());
        }
        for &q in &file.snowshoes.squads {
            match self.squads.get_mut(q as usize) {
                Some(sq) if sq.side == PLAYER_SIDE => sq.snowshoes = file.switches.snowshoes,
                _ => return Err(format!("snow file: squad {q} is not one of ours")),
            }
        }
        self.snow = Some(Snow::new(file, self.terrain.cols * self.terrain.rows));
        let c = self.clock.as_ref().unwrap();
        let t = c.calendar.local_seconds(self.game_seconds());
        let lux = c.sky.lux;
        self.spotting_range = self.sight_at(lux, t);
        self.forecast(t);
        Ok(())
    }

    /// How far a man can be spotted at local time `t` in light `lux`: the light's distance, cut
    /// while snow falls (decision 62).
    fn sight_at(&self, lux: f64, t: f64) -> f32 {
        let clear = self.tuning.spotting.spotting_range_m;
        let light = sky::spotting_distance(lux, &self.tuning.light, clear);
        match (&self.snow, &self.clock) {
            (Some(sn), Some(c)) => light * sn.tuning.sight_factor(c.weather.snowfall_at(t) as f32, clear),
            _ => light,
        }
    }

    /// The forecast at a dawn (local time `t`), for the day to 18:00 and the night after to 06:00
    /// (decision 61): what the weather diary brings, but with the wrong-forecast chance each half
    /// says otherwise. The seeded generator decides, so replays match.
    fn forecast(&mut self, t: f64) {
        let (Some(sn), Some(c)) = (&self.snow, &self.clock) else { return };
        if !sn.tuning.switches.forecast {
            return;
        }
        let midnight = (t / 86_400.0).floor() * 86_400.0;
        let evening = (midnight + 18.0 * 3600.0).max(t);
        let morning = midnight + 30.0 * 3600.0;
        let truth = [
            sn.tuning.amount(c.weather.snowfall_between(t, evening) as f32),
            sn.tuning.amount(c.weather.snowfall_between(evening, morning) as f32),
        ];
        let mut said = truth;
        if sn.tuning.switches.wrong_forecasts {
            let chance = sn.tuning.forecast.wrong_chance;
            for a in &mut said {
                if self.rng.unit() < chance {
                    *a = match *a {
                        crate::snow::SnowAmount::None => crate::snow::SnowAmount::Light,
                        _ => crate::snow::SnowAmount::None,
                    };
                }
            }
        }
        self.snow.as_mut().unwrap().forecast = Some(Forecast { said, truth });
    }

    /// Once a game minute with the snow file: fresh snow falls as the diary says and settles
    /// (decision 66), and the routes' copy of it is brought up to date.
    fn snow_minute(&mut self, tick: u64) {
        let (Some(sn), Some(c)) = (self.snow.as_mut(), &self.clock) else { return };
        let t = c.calendar.local_seconds(tick as f64 * self.tuning.dt() as f64);
        let fall = c.weather.snowfall_between(t, t + 60.0) as f32;
        let keep = 0.5f32.powf(1.0 / (sn.tuning.settling.half_life_hours * 60.0));
        sn.fallen_today += fall;
        sn.untrodden = sn.untrodden * keep + fall;
        let (loose, packed) = (&mut sn.loose, &mut sn.packed);
        self.pool.install(|| {
            loose.par_iter_mut().for_each(|l| *l = *l * keep + fall);
            packed.par_iter_mut().for_each(|p| *p *= keep);
        });
        if sn.routes_see_snow() {
            sn.for_routes = Arc::new(sn.loose.clone());
        }
    }

    /// Dawn or dusk: every rostered squad, including those sent elsewhere, takes up its duty for
    /// the half of the day now beginning. Squads give up places of the wrong kind first, so a squad
    /// coming off guard can take the shelter spot of one going on.
    fn change_shift(&mut self) {
        if self.places.is_empty() {
            return;
        }
        for i in 0..self.squads.len() {
            let q = &mut self.squads[i];
            if q.roster.is_some() {
                q.detached = false;
            }
            let duty = self.duty_now(i);
            let wanted = duty.map(Duty::place_kind);
            let at = self.post_kind(i);
            // A squad staying on carry duty carries on with its round (or stays resting if done).
            let carrying_on = duty == Some(Duty::Carry)
                && match self.squads[i].carry {
                    Some(CarryLeg::Done) => at == Some(PlaceKind::Shelter),
                    Some(_) => matches!(at, Some(PlaceKind::Dump | PlaceKind::Store)),
                    None => false,
                };
            if at.is_some() && wanted != at && !carrying_on {
                self.squads[i].post = None;
            }
            if self.squads[i].carry.is_some() {
                self.set_pace(i);
            }
        }
        for i in 0..self.squads.len() {
            self.take_up_duty(i);
        }
    }

    /// Puts the work roster in place at the start of a game: each listed squad gets its duties and
    /// stands at its place for the duty now running (inside, if that is a shelter).
    pub fn start_roster(&mut self, places: Vec<Place>, rosters: &[(u32, Roster)]) {
        self.places = places;
        for &(squad, roster) in rosters {
            let i = squad as usize;
            self.squads[i].roster = Some(roster);
            let Some(duty) = self.duty_now(i) else { continue };
            let kind = duty.place_kind();
            let Some((p, k)) = self.choose_spot(i, kind) else { continue };
            let (x, y) = self.places[p].spot(k);
            let members = self.squads[i].members.clone();
            let slots = self.formation_around(x, y, members.len());
            for (m, &slot) in members.iter().zip(&slots) {
                let s = &mut self.soldiers[*m as usize];
                (s.x, s.y) = slot;
                s.slot = slot;
                s.arrived = true;
                s.waypoint = None;
            }
            let q = &mut self.squads[i];
            q.post = Some((p, k));
            q.inside = kind == PlaceKind::Shelter;
        }
        self.grid.rebuild(self.soldiers.iter().map(|s| (s.x, s.y)));
    }

    // ---- the update ----------------------------------------------------------------------------

    /// Advances the simulation by one tick.
    pub fn step(&mut self) {
        let tick = self.tick;
        self.events.clear();

        // 0. The clock and the light, for the moment this update ends at.
        if let Some(c) = &self.clock {
            let t = c.calendar.local_seconds((tick + 1) as f64 * self.tuning.dt() as f64);
            let before = c.sky.sun_altitude_deg;
            let sky = c.calendar.sky(t, c.weather.cloud_at(t), &self.tuning.light);
            let falling = c.weather.snowfall_at(t) as f32;
            self.spotting_range = self.sight_at(sky.lux, t);
            if let Some(sn) = self.snow.as_mut() {
                sn.falling = falling;
            }
            let c = self.clock.as_mut().unwrap();
            c.sky = sky;
            if before < sky::DAWN_ALTITUDE_DEG && sky.sun_altitude_deg >= sky::DAWN_ALTITUDE_DEG {
                c.day += 1;
                c.night = false;
                let day = c.day;
                self.note(Event::Dawn { day });
                for q in self.squads.iter_mut().filter(|q| q.roster.is_some()) {
                    q.yesterday = Some(std::mem::take(&mut q.today));
                }
                if let Some(sup) = self.supply.as_mut() {
                    sup.carried_yesterday = Some(std::mem::take(&mut sup.carried_today));
                    sup.missed_meals_yesterday = Some(std::mem::take(&mut sup.missed_meals_today));
                }
                if let Some(sn) = self.snow.as_mut() {
                    sn.fallen_yesterday = Some(std::mem::take(&mut sn.fallen_today));
                }
                self.serve_meal();
                self.change_shift();
                self.forecast(t);
            } else if before > sky::DUSK_ALTITUDE_DEG && sky.sun_altitude_deg <= sky::DUSK_ALTITUDE_DEG {
                c.night = true;
                self.note(Event::Dusk);
                self.serve_meal();
                self.change_shift();
            }
        }

        // 1. Orders.
        while self.scheduled.front().is_some_and(|o| o.tick == tick) {
            let o = self.scheduled.pop_front().unwrap();
            match o.order {
                Order::Move { squad, x, y } => {
                    // A squad whose men are all lost cannot be sent anywhere.
                    if self.squads.get(squad as usize).is_some_and(|q| !q.members.is_empty()) {
                        let q = &mut self.squads[squad as usize];
                        q.roster_move = false;
                        // Sent elsewhere, a carrying squad drops its round (and any load it carries).
                        q.carry = None;
                        q.pace = 1.0;
                        if q.roster.is_some() {
                            q.detached = true;
                            q.post = None;
                            q.today.sent_away = true;
                        }
                        self.start_move(squad, x, y);
                    }
                }
                Order::SetRoster { squad, day, night } => {
                    if (squad as usize) < self.squads.len() && !self.places.is_empty() {
                        self.squads[squad as usize].roster = Some(Roster { day, night });
                        self.take_up_duty(squad as usize);
                    }
                }
                Order::BackToDuty { squad } => {
                    if let Some(q) = self.squads.get_mut(squad as usize)
                        && q.roster.is_some()
                        && q.detached
                    {
                        q.detached = false;
                        self.take_up_duty(squad as usize);
                    }
                }
            }
            self.order_log.push(o);
        }
        self.advance_carrying(tick);

        // Tiredness, once a game minute (games with a work roster only).
        let minute = self.tuning.seconds_to_ticks(60.0);
        if !self.places.is_empty() && tick.is_multiple_of(minute) {
            let tt = &self.tuning.tiredness;
            let (up, down) = (1.0 / (tt.hours_awake_to_exhausted * 60.0), 1.0 / (tt.hours_asleep_to_fresh * 60.0));
            for &i in &self.by_side[PLAYER_SIDE.index()] {
                let s = &mut self.soldiers[i as usize];
                let q = &self.squads[s.squad as usize];
                let asleep = q.inside;
                // With the supply file: cold, hunger and carrying tire a man faster; hunger spoils
                // his sleep.
                let (up, down) = match &self.supply {
                    None => (up, down),
                    Some(sup) => {
                        let ht = &sup.tuning.hunger;
                        let hunger = ht.hunger(s.minutes_since_meal);
                        let carrying = matches!(q.carry, Some(c) if c != CarryLeg::Done);
                        let carry = if carrying { sup.tuning.carrying.tiring_factor } else { 1.0 };
                        (
                            up * sup.tuning.cold.tiring_factor(s.chill) * ht.scale(hunger, ht.tiring_factor) * carry,
                            down * ht.scale(hunger, ht.sleep_factor),
                        )
                    }
                };
                if tt.enabled {
                    s.tiredness = if asleep { (s.tiredness - down).max(0.0) } else { (s.tiredness + up).min(1.0) };
                }
            }
            for q in self.squads.iter_mut().filter(|q| q.roster.is_some()) {
                if q.inside {
                    q.today.sleep_minutes += 1;
                } else if matches!(q.carry, Some(c) if c != CarryLeg::Done) {
                    q.today.carry_minutes += 1;
                } else if let Some((p, _)) = q.post
                    && self.places[p].kind == PlaceKind::Post
                    && q.state == SquadState::Arrived
                {
                    q.today.guard_minutes += 1;
                }
            }
            self.supply_minute(tick);
        }
        if self.snow.is_some() && tick.is_multiple_of(minute) {
            self.snow_minute(tick);
        }

        // 2. Patrols.
        for i in 0..self.squads.len() {
            let sq = &self.squads[i];
            let Some(zone) = sq.patrol else { continue };
            let waiting = matches!(sq.state, SquadState::Idle | SquadState::Arrived | SquadState::Unreachable);
            if waiting && tick >= sq.next_patrol_tick {
                self.squads[i].next_patrol_tick = u64::MAX;
                match self.pick_patrol_destination(zone) {
                    Some((x, y)) => self.start_move(i as u32, x, y),
                    None => {
                        let p = self.patrol_pause_ticks();
                        self.squads[i].next_patrol_tick = tick + p;
                    }
                }
                if self.squads[i].state == SquadState::Unreachable {
                    let p = self.patrol_pause_ticks();
                    self.squads[i].next_patrol_tick = tick + p;
                }
            }
        }

        // 3. Routes ready.
        for i in 0..self.squads.len() {
            if let SquadState::Planning { ready_tick } = self.squads[i].state
                && ready_tick <= tick
            {
                self.begin_moving(i as u32);
            }
        }

        // 4. Movement.
        let updates: Vec<Update> = {
            let ctx = MoveCtx {
                t: &self.terrain,
                tuning: &self.tuning,
                soldiers: &self.soldiers,
                squads: &self.squads,
                grid: &self.grid,
                cold: self.supply.as_ref().map(|s| &s.tuning.cold),
                snow: self.snow.as_ref(),
            };
            self.pool.install(|| self.soldiers.par_iter().map(|s| ctx.update(s)).collect())
        };
        // Walking packs the fresh snow, man by man in soldier order (decision 66).
        let packing = self.snow.as_ref().filter(|sn| sn.tuning.switches.trail_breaking).map(|sn| sn.tuning.packing_per_m(self.terrain.spacing));
        for (s, u) in self.soldiers.iter_mut().zip(updates) {
            if let (Some(k), Some(sn)) = (packing, self.snow.as_mut()) {
                let d = ((u.x - s.x).powi(2) + (u.y - s.y).powi(2)).sqrt();
                if d > 0.0 {
                    let (c, r) = self.terrain.nearest_point(u.x, u.y);
                    let i = self.terrain.idx(c, r);
                    let before = sn.loose[i];
                    let after = before * (-k * d).exp();
                    sn.loose[i] = after;
                    sn.packed[i] += before - after;
                }
            }
            s.x = u.x;
            s.y = u.y;
            s.vx = u.vx;
            s.vy = u.vy;
            s.facing = u.facing;
            s.arrived = u.arrived;
            s.waypoint = u.waypoint;
            s.still_ticks = u.still_ticks;
        }
        self.grid.rebuild(self.soldiers.iter().map(|s| (s.x, s.y)));

        // 5. Arrivals.
        for i in 0..self.squads.len() {
            if self.squads[i].state != SquadState::Moving || self.squads[i].members.is_empty() {
                continue;
            }
            let all = self.squads[i].members.iter().all(|&m| self.soldiers[m as usize].arrived);
            if all {
                self.squads[i].state = SquadState::Arrived;
                self.squads[i].arrived_tick = Some(tick);
                if let Some((p, _)) = self.squads[i].post
                    && self.places[p].kind == PlaceKind::Shelter
                {
                    self.squads[i].inside = true;
                }
                if self.squads[i].carry.is_some() {
                    self.carrier_arrived(i, tick);
                }
                if self.squads[i].patrol.is_some() {
                    let p = self.patrol_pause_ticks();
                    self.squads[i].next_patrol_tick = tick + p;
                }
                if self.player_ordered(i as u32) {
                    self.note(Event::Arrived { squad: i as u32 });
                }
            }
        }

        // 6. Spotting.
        let look = self.tuning.seconds_to_ticks(self.tuning.spotting.look_interval_s);
        let due: Vec<u32> =
            self.soldiers.iter().filter(|s| s.gone.is_none() && (s.id as u64) % look == tick % look).map(|s| s.id).collect();
        let seen: Vec<Vec<u32>> = {
            let me = &*self;
            me.pool.install(|| due.par_iter().map(|&i| me.spot_from(i)).collect())
        };
        for (i, v) in due.into_iter().zip(seen) {
            self.soldiers[i as usize].sees = v;
        }

        // 7. New contacts.
        if self.clock.is_some() {
            self.note_contacts(tick);
        }

        self.tick += 1;
    }

    /// The skip-ahead (decision 29): runs updates while `more` allows, stopping after the first
    /// update that notes an event that stops time (dawn, a new contact, an arrival), whose events it
    /// returns. `after` runs after every update (for recording a replay, say). Running updates this
    /// way or one at a time gives exactly the same result.
    pub fn skip(&mut self, mut more: impl FnMut(&Sim) -> bool, mut after: impl FnMut(&Sim)) -> Option<Vec<Event>> {
        while more(self) {
            self.step();
            after(self);
            if self.events.iter().any(Event::stops_time) {
                return Some(self.events.clone());
            }
        }
        None
    }

    /// Notes every enemy squad the player's men see now that none of them had seen for the tuning's
    /// while (or ever) as a new contact, with the nearest of our squads seeing it.
    fn note_contacts(&mut self, tick: u64) {
        let gap = self.tuning.seconds_to_ticks(self.tuning.events.new_contact_after_unseen_minutes * 60.0);
        // Per enemy squad seen: (distance, our squad) of the nearest sighting.
        let mut nearest: BTreeMap<u32, (f32, u32)> = BTreeMap::new();
        for &i in &self.by_side[PLAYER_SIDE.index()] {
            let s = &self.soldiers[i as usize];
            for &e in &s.sees {
                let en = &self.soldiers[e as usize];
                let d = ((en.x - s.x).powi(2) + (en.y - s.y).powi(2)).sqrt();
                let entry = nearest.entry(en.squad).or_insert((f32::INFINITY, s.squad));
                if d < entry.0 {
                    *entry = (d, s.squad);
                }
            }
        }
        for (q, (d, by)) in nearest {
            let new = self.squads[q as usize].last_seen_by_player.is_none_or(|t| tick - t > gap);
            self.squads[q as usize].last_seen_by_player = Some(tick);
            if new {
                self.note(Event::Contact { enemy_squad: q, seen_by_squad: by, distance_m: d });
            }
        }
    }

    /// The local date and time now, with a calendar.
    pub fn now(&self) -> Option<sky::DateTime> {
        self.clock.as_ref().map(|c| c.calendar.now(self.game_seconds()))
    }

    /// The local date and time after update `tick`, with a calendar.
    pub fn time_at(&self, tick: u64) -> Option<sky::DateTime> {
        let dt = self.tuning.dt() as f64;
        self.clock.as_ref().map(|c| c.calendar.now(tick as f64 * dt))
    }

    /// How far soldier `i` can spot a man right now: the light's distance, less for a tired man.
    pub fn spotting_range_of(&self, i: u32) -> f32 {
        self.spotting_range * self.tuning.tiredness.spotting_factor(self.soldiers[i as usize].tiredness)
    }

    /// Enemies soldier `i` can see right now, within his spotting range. Men inside a shelter
    /// neither see nor can be seen.
    pub fn spot_from(&self, i: u32) -> Vec<u32> {
        let o = &self.soldiers[i as usize];
        if self.squads[o.squad as usize].inside {
            return Vec::new();
        }
        let t = &self.terrain;
        let eye = [o.x, o.y, t.height(o.x, o.y) + self.tuning.soldier.eye_height_m];
        let range2 = self.spotting_range_of(i).powi(2);
        let head = self.tuning.soldier.head_height_m;
        self.by_side[o.side.enemy().index()]
            .iter()
            .copied()
            .filter(|&e| {
                let s = &self.soldiers[e as usize];
                let d2 = (s.x - o.x).powi(2) + (s.y - o.y).powi(2);
                d2 <= range2
                    && !self.squads[s.squad as usize].inside
                    && los::visible(t, &self.los, eye, [s.x, s.y, t.height(s.x, s.y) + head])
            })
            .collect()
    }

    /// Whether soldier `a`'s eye can see soldier `b`'s head (ignoring spotting range).
    pub fn can_see(&self, a: u32, b: u32) -> bool {
        let (sa, sb) = (&self.soldiers[a as usize], &self.soldiers[b as usize]);
        let t = &self.terrain;
        let eye = [sa.x, sa.y, t.height(sa.x, sa.y) + self.tuning.soldier.eye_height_m];
        let head = [sb.x, sb.y, t.height(sb.x, sb.y) + self.tuning.soldier.head_height_m];
        los::visible(t, &self.los, eye, head)
    }

    /// A checksum of the whole simulation state. Two runs match only if these match every tick.
    pub fn checksum(&self) -> u64 {
        let mut h = Checksum::default();
        h.u64(self.tick);
        for w in self.rng.state() {
            h.u64(w);
        }
        for s in &self.soldiers {
            for v in [s.x, s.y, s.vx, s.vy, s.facing, s.slot.0, s.slot.1] {
                h.f32(v);
            }
            h.u32(s.arrived as u32);
            h.u32(s.still_ticks);
            match s.waypoint {
                Some((x, y)) => {
                    h.f32(x);
                    h.f32(y);
                }
                None => h.u32(0xFFFF_FFFF),
            }
            h.u32(s.sees.len() as u32);
            for &e in &s.sees {
                h.u32(e);
            }
        }
        for q in &self.squads {
            let st = match q.state {
                SquadState::Idle => 0,
                SquadState::Planning { ready_tick } => 1 + 8 * ready_tick,
                SquadState::Moving => 2,
                SquadState::Arrived => 3,
                SquadState::Unreachable => 4,
            };
            h.u64(st);
            h.u64(q.order_tick);
            h.u64(q.arrived_tick.unwrap_or(u64::MAX));
            h.u64(q.next_patrol_tick);
            if let Some((x, y)) = q.dest {
                h.f32(x);
                h.f32(y);
            }
        }
        // Only with a calendar, so step 1's checksums are exactly what they were.
        if let Some(c) = &self.clock {
            h.u32(c.day);
            h.u64(c.sky.lux.to_bits());
            h.f32(self.spotting_range);
            h.u64(self.journal.len() as u64);
            for q in &self.squads {
                h.u64(q.last_seen_by_player.unwrap_or(u64::MAX));
            }
            // The work roster, where there is one.
            if !self.places.is_empty() {
                h.u32(c.night as u32);
                for s in &self.soldiers {
                    h.f32(s.tiredness);
                }
                for q in &self.squads {
                    let duty = |d: Duty| d as u32;
                    h.u32(q.roster.map(|r| 1 + duty(r.day) * 2 + duty(r.night) * 4).unwrap_or(0));
                    h.u32(q.detached as u32 | (q.inside as u32) << 1 | (q.roster_move as u32) << 2);
                    let (p, k) = q.post.unwrap_or((usize::MAX, usize::MAX));
                    h.u64(p as u64);
                    h.u64(k as u64);
                    for d in [Some(q.today), q.yesterday] {
                        let d = d.unwrap_or(DayRecord { guard_minutes: u32::MAX, ..DayRecord::default() });
                        h.u32(d.guard_minutes);
                        h.u32(d.sleep_minutes);
                        h.u32(d.sent_away as u32);
                    }
                }
            }
            // Carrying, cold and hunger, where the supply file is in force.
            if let Some(sup) = &self.supply {
                h.f32(sup.food_kg);
                h.f32(sup.fuel_kg);
                h.u64(sup.lost.len() as u64);
                h.f32(sup.carried_today[0]);
                h.f32(sup.carried_today[1]);
                h.u32(sup.missed_meals_today);
                for s in &self.soldiers {
                    h.f32(s.chill);
                    h.u32(s.minutes_since_meal);
                    h.u32(s.worst_minutes);
                    h.u32(match s.gone {
                        None => 0,
                        Some(Lost::Frostbite) => 1,
                        Some(Lost::Died) => 2,
                    });
                }
                for q in &self.squads {
                    let stock = |s: Stock| s as u64;
                    h.u64(match q.carry {
                        None => 0,
                        Some(CarryLeg::ToDump) => 1,
                        Some(CarryLeg::Loading { until }) => 2 + 16 * until,
                        Some(CarryLeg::ToStore { stock: s }) => 3 + 16 * stock(s),
                        Some(CarryLeg::Unloading { until, stock: s }) => 4 + 16 * (until * 2 + stock(s)),
                        Some(CarryLeg::Done) => 5,
                    });
                    h.f32(q.pace);
                    h.u32(q.today.carry_minutes);
                }
            }
            // Falling snow, where the snow file is in force: the fresh snow added up over the
            // whole map, a sample of single points, the forecast and the snowshoes.
            if let Some(sn) = &self.snow {
                let (mut a, mut b) = (0.0f64, 0.0f64);
                for (l, p) in sn.loose.iter().zip(&sn.packed) {
                    a += *l as f64;
                    b += *p as f64;
                }
                h.u64(a.to_bits());
                h.u64(b.to_bits());
                for i in (0..sn.loose.len()).step_by(997) {
                    h.f32(sn.loose[i]);
                }
                h.f32(sn.falling);
                h.f32(sn.untrodden);
                h.f32(sn.fallen_today);
                match sn.forecast {
                    Some(f) => {
                        for a in f.said.iter().chain(&f.truth) {
                            h.u32(a.index());
                        }
                    }
                    None => h.u32(0xFFFF),
                }
                for q in &self.squads {
                    h.u32(q.snowshoes as u32);
                }
            }
        }
        h.finish()
    }
}

struct Update {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    facing: f32,
    arrived: bool,
    waypoint: Option<(f32, f32)>,
    still_ticks: u32,
}

struct MoveCtx<'a> {
    t: &'a Terrain,
    tuning: &'a Tuning,
    soldiers: &'a [Soldier],
    squads: &'a [Squad],
    grid: &'a SpatialGrid,
    /// Cold's numbers, with the supply file.
    cold: Option<&'a ColdTuning>,
    /// Fresh snow, with the snow file.
    snow: Option<&'a Snow>,
}

impl MoveCtx<'_> {
    fn update(&self, s: &Soldier) -> Update {
        let t = self.t;
        let tu = self.tuning;
        let dt = tu.dt();
        let sq = &self.squads[s.squad as usize];
        let resting = Update {
            x: s.x,
            y: s.y,
            vx: 0.0,
            vy: 0.0,
            facing: s.facing,
            arrived: s.arrived,
            waypoint: s.waypoint,
            still_ticks: 0,
        };
        if sq.state != SquadState::Moving || s.arrived || s.gone.is_some() {
            return resting;
        }
        let field = sq.field.as_ref().expect("moving squad has a route");

        // Where to head: straight for his formation place once it is close and in clear reach,
        // otherwise along the route.
        let (sx, sy) = s.slot;
        let d_slot = ((sx - s.x).powi(2) + (sy - s.y).powi(2)).sqrt();
        let mut waypoint = s.waypoint;
        let to_slot = d_slot < 3.0 * t.spacing && t.segment_walkable(s.x, s.y, sx, sy);
        let target = if to_slot {
            waypoint = None;
            (sx, sy)
        } else {
            // Join the route at the best nearby point, then follow it point by point. Re-join if
            // held up for a while (pushed off the route).
            let stuck = s.still_ticks > 0 && s.still_ticks.is_multiple_of(STUCK_REJOIN_TICKS);
            match waypoint {
                None => waypoint = field.next_waypoint(t, &tu.walking, s.x, s.y),
                Some(_) if stuck => waypoint = field.next_waypoint(t, &tu.walking, s.x, s.y),
                Some(wp) if (wp.0 - s.x).powi(2) + (wp.1 - s.y).powi(2) < WAYPOINT_REACHED_M * WAYPOINT_REACHED_M => {
                    if let Some(next) = field.advance(t, &tu.walking, s.x, s.y, wp) {
                        waypoint = Some(next);
                    }
                }
                Some(_) => {}
            }
            waypoint.unwrap_or((sx, sy))
        };
        let (dx, dy) = (target.0 - s.x, target.1 - s.y);
        let dist = (dx * dx + dy * dy).sqrt();
        let (mut mx, mut my) = if dist > 1e-4 { (dx / dist, dy / dist) } else { (0.0, 0.0) };

        // Keep personal space.
        let ps = tu.soldier.personal_space_m;
        let (mut px, mut py) = (0.0f32, 0.0f32);
        self.grid.for_each_near(s.x, s.y, ps, |j| {
            if j == s.id {
                return;
            }
            let o = &self.soldiers[j as usize];
            // Men inside a shelter are under rock or a roof, not in the way of anyone outside; men
            // lost to the company are gone.
            if self.squads[o.squad as usize].inside || o.gone.is_some() {
                return;
            }
            let (ex, ey) = (s.x - o.x, s.y - o.y);
            let d = (ex * ex + ey * ey).sqrt();
            if d < 1e-4 {
                // Standing on the same spot: step aside in a direction fixed by his id.
                let a = s.id as f32 * 2.399_963;
                px += a.cos();
                py += a.sin();
            } else if d < ps {
                let k = (ps - d) / ps;
                px += ex / d * k;
                py += ey / d * k;
            }
        });
        if s.still_ticks > IGNORE_SPACE_AFTER_TICKS {
            // Held up: walk on regardless of personal space until moving again.
            px = 0.0;
            py = 0.0;
        } else if px * mx + py * my < 0.0 {
            // Someone in the way ahead: keep right, so two men meeting head-on pass each other.
            let push = (px * px + py * py).sqrt();
            px += my * push * 0.7;
            py += -mx * push * 0.7;
        }
        mx += 1.5 * px;
        my += 1.5 * py;
        let ml = (mx * mx + my * my).sqrt();
        if ml < 1e-5 {
            return Update { still_ticks: s.still_ticks + 1, waypoint, ..resting };
        }
        let (ux, uy) = (mx / ml, my / ml);
        let grade = t.height(s.x + ux, s.y + uy) - t.height(s.x, s.y);
        // Tiredness and cold do not add up: the worse counts. Then the squad's pace (carrying).
        let mut tired = tu.tiredness.walking_factor(s.tiredness);
        if let Some(cold) = self.cold {
            tired = tired.min(cold.walking_factor(s.chill)) * sq.pace;
        }
        // Fresh snow where he stands slows him, less on snowshoes (decision 66).
        if let Some(sn) = self.snow {
            let (c, r) = t.nearest_point(s.x, s.y);
            tired *= sn.tuning.walking_factor(sn.loose[t.idx(c, r)], sq.snowshoes);
        }
        let mut step = tu.walking.speed_mps(grade) * tired * dt * ml.min(1.0);
        if to_slot && dist < step && px == 0.0 && py == 0.0 {
            step = dist;
        }

        // Never step onto ground too steep to stand on. If keeping personal space would push him
        // onto it, walk his route alone (its line is known to be clear); otherwise slide along it.
        let mut nx = s.x + ux * step;
        let mut ny = s.y + uy * step;
        if !t.walkable_at(nx, ny) && dist > 1e-4 {
            let (rx, ry) = (dx / dist, dy / dist);
            let grade = t.height(s.x + rx, s.y + ry) - t.height(s.x, s.y);
            let rstep = (tu.walking.speed_mps(grade) * tired * dt).min(if to_slot { dist } else { f32::INFINITY });
            if t.walkable_at(s.x + rx * rstep, s.y + ry * rstep) {
                nx = s.x + rx * rstep;
                ny = s.y + ry * rstep;
            }
        }
        // Still blocked: he has been pushed off the route's line beside steep ground (on a narrow
        // ridge, say). Step back onto the nearest grid point, where the route's lines run.
        if !t.walkable_at(nx, ny) {
            let (gc, gr) = t.nearest_point(s.x, s.y);
            let (gx, gy) = t.point_xy(gc, gr);
            let (ex, ey) = (gx - s.x, gy - s.y);
            let gd = (ex * ex + ey * ey).sqrt();
            if gd > 1e-3 && t.walkable_at(gx, gy) {
                let grade = t.height(gx, gy) - t.height(s.x, s.y);
                let gstep = (tu.walking.speed_mps(grade / gd.max(1.0)) * tired * dt).min(gd);
                let (qx, qy) = (s.x + ex / gd * gstep, s.y + ey / gd * gstep);
                if t.walkable_at(qx, qy) {
                    nx = qx;
                    ny = qy;
                }
            }
        }
        if !t.walkable_at(nx, ny) {
            if t.walkable_at(s.x + ux * step, s.y) {
                ny = s.y;
            } else if t.walkable_at(s.x, s.y + uy * step) {
                nx = s.x;
            } else {
                nx = s.x;
                ny = s.y;
            }
        }
        let moved = ((nx - s.x).powi(2) + (ny - s.y).powi(2)).sqrt();
        let arrived = to_slot && ((nx - sx).powi(2) + (ny - sy).powi(2)).sqrt() <= tu.squad.arrival_radius_m;
        Update {
            x: nx,
            y: ny,
            vx: if arrived { 0.0 } else { (nx - s.x) / dt },
            vy: if arrived { 0.0 } else { (ny - s.y) / dt },
            facing: if moved > 1e-4 { (ny - s.y).atan2(nx - s.x) } else { s.facing },
            arrived,
            waypoint: if arrived { None } else { waypoint },
            still_ticks: if moved < STILL_BELOW_M_PER_TICK && !arrived { s.still_ticks + 1 } else { 0 },
        }
    }
}
