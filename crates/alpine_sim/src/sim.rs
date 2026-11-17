//! The simulation: soldiers, squads, orders, and the fixed update order.
//!
//! Every update (tick) runs these stages in this order:
//! 0. with a calendar (step 2 on): the clock moves on, the light and spotting distance are worked
//!    out for the new moment, and dawn and dusk are noted;
//! 1. orders scheduled for this tick are given to their squads;
//! 2. patrolling squads that have waited long enough pick a new destination;
//! 3. squads whose route is ready start moving;
//! 4. every soldier moves (split across threads; each man reads only the previous state);
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
use crate::sky::{self, Calendar, SkyState};
use crate::spatial::SpatialGrid;
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Order {
    /// Send a squad to a place (metres from the summit).
    Move { squad: u32, x: f32, y: f32 },
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
/// Moving less than this in a tick counts as standing still (1 cm per second at 10 ticks a second;
/// the slowest walking speed, straight up a 45° slope, is about 3 cm per second).
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
        self.clock = Some(Clock { calendar, weather, sky, day: 0 });
        self.spotting_range = sky::spotting_distance(sky.lux, &self.tuning.light, self.tuning.spotting.spotting_range_m);
    }

    /// Notes an event as having happened in the update now running.
    fn note(&mut self, event: Event) {
        self.journal.push(LoggedEvent { tick: self.tick + 1, event: event.clone() });
        self.events.push(event);
    }

    /// Whether events about this squad are worth telling the player: one of his own, under his
    /// orders rather than wandering on a patrol, in a game with a calendar.
    fn player_ordered(&self, squad: u32) -> bool {
        let q = &self.squads[squad as usize];
        self.clock.is_some() && q.side == PLAYER_SIDE && q.patrol.is_none()
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
        self.pool.spawn(move || {
            let f = Arc::new(FlowField::compute_bounded(&terrain, &walking, dp, &from));
            *job.result.lock().unwrap() = Some(f);
            job.ready.notify_all();
        });
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
            self.spotting_range = sky::spotting_distance(sky.lux, &self.tuning.light, self.tuning.spotting.spotting_range_m);
            let c = self.clock.as_mut().unwrap();
            c.sky = sky;
            if before < sky::DAWN_ALTITUDE_DEG && sky.sun_altitude_deg >= sky::DAWN_ALTITUDE_DEG {
                c.day += 1;
                let day = c.day;
                self.note(Event::Dawn { day });
            } else if before > sky::DUSK_ALTITUDE_DEG && sky.sun_altitude_deg <= sky::DUSK_ALTITUDE_DEG {
                self.note(Event::Dusk);
            }
        }

        // 1. Orders.
        while self.scheduled.front().is_some_and(|o| o.tick == tick) {
            let o = self.scheduled.pop_front().unwrap();
            match o.order {
                Order::Move { squad, x, y } => {
                    if (squad as usize) < self.squads.len() {
                        self.start_move(squad, x, y);
                    }
                }
            }
            self.order_log.push(o);
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
            };
            self.pool.install(|| self.soldiers.par_iter().map(|s| ctx.update(s)).collect())
        };
        for (s, u) in self.soldiers.iter_mut().zip(updates) {
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
            if self.squads[i].state != SquadState::Moving {
                continue;
            }
            let all = self.squads[i].members.iter().all(|&m| self.soldiers[m as usize].arrived);
            if all {
                self.squads[i].state = SquadState::Arrived;
                self.squads[i].arrived_tick = Some(tick);
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
            self.soldiers.iter().filter(|s| (s.id as u64) % look == tick % look).map(|s| s.id).collect();
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

    /// Enemies soldier `i` can see right now, within spotting range.
    pub fn spot_from(&self, i: u32) -> Vec<u32> {
        let o = &self.soldiers[i as usize];
        let t = &self.terrain;
        let eye = [o.x, o.y, t.height(o.x, o.y) + self.tuning.soldier.eye_height_m];
        let range2 = self.spotting_range.powi(2);
        let head = self.tuning.soldier.head_height_m;
        self.by_side[o.side.enemy().index()]
            .iter()
            .copied()
            .filter(|&e| {
                let s = &self.soldiers[e as usize];
                let d2 = (s.x - o.x).powi(2) + (s.y - o.y).powi(2);
                d2 <= range2 && los::visible(t, &self.los, eye, [s.x, s.y, t.height(s.x, s.y) + head])
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
        if sq.state != SquadState::Moving || s.arrived {
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
        let mut step = tu.walking.speed_mps(grade) * dt * ml.min(1.0);
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
            let rstep = (tu.walking.speed_mps(grade) * dt).min(if to_slot { dist } else { f32::INFINITY });
            if t.walkable_at(s.x + rx * rstep, s.y + ry * rstep) {
                nx = s.x + rx * rstep;
                ny = s.y + ry * rstep;
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
