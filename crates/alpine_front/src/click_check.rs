//! Automatic click check (environment variable `ALPINE_CLICK_CHECK=1`), for proving that both views
//! can be played the same way. In whichever view the game starts in, it:
//! 0. at the dawn orders screen the game opens with, presses Enter to carry on,
//! 1. left-clicks an Austro-Hungarian squad (its flag on the map, one of its men in 3D),
//! 2. right-clicks the ground about 150 m from the squad (a point the camera can see) to send it there,
//! 3. on the map only: zooms with the mouse wheel (the point under the cursor must stay put), holds W
//!    (the map must move north) and right-drags (the map must follow the mouse, with no order given),
//! 4. presses M to switch views,
//! 5. presses N for the skip-ahead (the game must run far faster than 64x, or stop on an event).
//!
//! The clicks go through the same code as the real mouse (`orders.rs`), with a stand-in cursor
//! position, so the real mouse pointer is never moved. Prints one CLICK-CHECK line and exits.

use crate::camera::FlyCam;
use crate::map_view::{FLAG_PICK_OFFSET_PX, MapView, map_controls, squad_anchor};
use crate::orders::{CursorOverride, clicks};
use crate::sim_link::SimLink;
use crate::terrain_view::{ground_hit, to_world};
use crate::ui::shortcuts;
use crate::units::{Selection, marker_scale};
use crate::view::{MapCam, TerrainCam, ViewMode, switch_key};
use alpine_sim::sim::Side;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

const SETTLE_S: f32 = 3.0;
/// The order must land within this many metres of the point clicked.
const ORDER_TOLERANCE_M: f32 = 3.0;
/// First step of the map-moving checks, and of pressing M.
const MOVE: u32 = 10;
const SWITCH: u32 = 30;
const SKIP: u32 = 40;
/// Frames the skip-ahead check watches.
const SKIP_FRAMES: u32 = 20;
/// How far the right-drag check moves the mouse, in pixels.
const DRAG_PX: f32 = 60.0;

pub struct ClickCheckPlugin;

impl Plugin for ClickCheckPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("ALPINE_CLICK_CHECK").is_ok() {
            app.add_systems(Update, run.before(clicks).before(switch_key).before(map_controls).before(shortcuts));
        }
    }
}

#[derive(Default)]
struct Check {
    step: u32,
    /// Frames left to wait after bringing the map or the 3D camera in close to the summit, where the
    /// squads stand close together at their posts (`None` before moving in).
    zoom_wait: Option<u32>,
    squad: Option<u32>,
    target: (f32, f32),
    started_in: Option<ViewMode>,
    /// The map before a moving check: where it looked and how close.
    before: Option<MapView>,
    /// The point the stand-in cursor rests on during the map-moving checks, and the ground under it.
    rest: Vec2,
    under: Vec2,
    message_before: Option<String>,
    results: Vec<String>,
    passed: bool,
    /// The dawn screen step: 0 not started, 1 Enter pressed, 2 done.
    dawn: u8,
    skip_from_tick: u64,
}

type Cam<'a> = (&'a Camera, &'a GlobalTransform);

/// Where something at this map/ground point appears on screen in the current view.
fn on_screen(mode: ViewMode, link: &SimLink, terrain: Cam, map: Cam, (x, y): (f32, f32), lift: f32) -> Option<Vec2> {
    match mode {
        ViewMode::Map => map.0.world_to_viewport(map.1, Vec3::new(x, y, 0.0)).ok(),
        ViewMode::Terrain => {
            let p = to_world(x, y, link.sim.terrain.height(x, y) + lift);
            terrain.0.world_to_viewport(terrain.1, p).ok()
        }
    }
}

/// An Austro-Hungarian squad whose click target is on screen, in the open part of the window, and at
/// least 25 pixels from any other squad's (so the click cannot be ambiguous).
#[allow(clippy::too_many_arguments)]
fn choose(mode: ViewMode, link: &SimLink, map: &MapView, window: &Window, terrain: Cam, map_cam: Cam) -> Option<(u32, Vec2)> {
    let sim = &link.sim;
    // Each squad's click point: the flag on the map, the first man in 3D. Squads inside a shelter
    // are not drawn and cannot be clicked.
    let points: Vec<(u32, Side, Vec2)> = sim
        .squads
        .iter()
        .filter(|q| !q.inside)
        .filter_map(|q| {
            let p = match mode {
                ViewMode::Map if !map.shows_men() => {
                    on_screen(mode, link, terrain, map_cam, squad_anchor(link, q.id)?, 0.0)? + FLAG_PICK_OFFSET_PX
                }
                _ => {
                    let lead = q.members[0] as usize;
                    let (x, y) = link.drawn_position(lead);
                    let distance = terrain.1.translation().distance(to_world(x, y, sim.terrain.height(x, y)));
                    let lift = if mode == ViewMode::Terrain { 0.8 * marker_scale(distance) } else { 0.0 };
                    let p = on_screen(mode, link, terrain, map_cam, (x, y), lift)?;
                    // In 3D, skip men hidden behind a ridge: the ground under the click must be his.
                    if mode == ViewMode::Terrain {
                        let feet = on_screen(mode, link, terrain, map_cam, (x, y), 0.0)?;
                        let ray = terrain.0.viewport_to_world(terrain.1, feet).ok()?;
                        let (hx, hy) = ground_hit(&sim.terrain, ray.origin, *ray.direction)?;
                        if ((hx - x).powi(2) + (hy - y).powi(2)).sqrt() > 10.0 {
                            return None;
                        }
                    }
                    p
                }
            };
            Some((q.id, q.side, p))
        })
        .collect();
    let open = |p: Vec2| p.x > 520.0 && p.x < window.width() - 40.0 && p.y > 60.0 && p.y < window.height() - 40.0;
    points
        .iter()
        .filter(|(_, side, p)| *side == Side::Austria && open(*p))
        .find(|(id, _, p)| points.iter().all(|(o, _, op)| o == id || op.distance(*p) > 25.0))
        .map(|&(id, _, p)| (id, p))
}

#[allow(clippy::too_many_arguments)] // a Bevy system: each argument is a piece of game state it reads
fn run(
    time: Res<Time>,
    mode: Res<ViewMode>,
    link: Res<SimLink>,
    mut map: ResMut<MapView>,
    selection: Res<Selection>,
    window: Single<&Window, With<PrimaryWindow>>,
    terrain_cam: Single<Cam, With<TerrainCam>>,
    map_cam: Single<Cam, With<MapCam>>,
    mut cursor: ResMut<CursorOverride>,
    mut scroll: ResMut<AccumulatedMouseScroll>,
    mut motion: ResMut<AccumulatedMouseMotion>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut check: Local<Check>,
    mut exit: MessageWriter<AppExit>,
    mut fly: Single<(&mut Transform, &mut FlyCam), With<TerrainCam>>,
) {
    if time.elapsed_secs() < SETTLE_S {
        return;
    }
    let (terrain_cam, map_cam) = (*terrain_cam, *map_cam);
    let view_name = |m: ViewMode| if m == ViewMode::Map { "map" } else { "terrain" };
    let c = &mut *check;
    // 0. The game opens at dawn with the orders screen: carry on.
    match c.dawn {
        0 if link.dawn_screen => {
            keys.press(KeyCode::Enter);
            c.dawn = 1;
            return;
        }
        0 => {
            c.results.push("dawn=none (no calendar)".into());
            c.dawn = 2;
        }
        1 => {
            keys.release(KeyCode::Enter);
            if !link.dawn_screen && !link.paused {
                c.results.push("dawn=ok (Enter closed the orders screen and time runs)".into());
            } else {
                c.results.push("dawn=FAIL (the orders screen stayed open)".into());
                c.passed = false;
            }
            c.dawn = 2;
            return;
        }
        _ => {}
    }
    match c.step {
        0 => {
            c.started_in = Some(*mode);
            c.passed = !c.results.iter().any(|r| r.contains("FAIL"));
            if let Some(n) = c.zoom_wait
                && n > 0
            {
                c.zoom_wait = Some(n - 1);
                return;
            }
            let chosen = choose(*mode, &link, &map, &window, terrain_cam, map_cam);
            // From afar the squads at their posts stand too close to tell apart: move in on the
            // summit, as a player would, and try again.
            if chosen.is_none() && c.zoom_wait.is_none() {
                match *mode {
                    ViewMode::Map => *map = MapView { centre: Vec2::new(0.0, 0.0), scale: 0.8 },
                    ViewMode::Terrain => {
                        let (tf, cam) = &mut *fly;
                        let pos = to_world(-60.0, -260.0, link.sim.terrain.height(-60.0, -260.0) + 120.0);
                        let dir = (to_world(0.0, 0.0, link.sim.terrain.height(0.0, 0.0)) - pos).normalize();
                        tf.translation = pos;
                        cam.yaw = (-dir.x).atan2(-dir.z);
                        cam.pitch = dir.y.asin();
                        tf.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
                    }
                }
                c.zoom_wait = Some(3);
                return;
            }
            match chosen {
                Some((squad, p)) => {
                    c.squad = Some(squad);
                    cursor.0 = Some(p);
                    mouse.press(MouseButton::Left);
                }
                None => {
                    c.results.push("select=FAIL (no Austro-Hungarian squad clearly on screen)".into());
                    c.passed = false;
                    c.step = SWITCH;
                    return;
                }
            }
        }
        1 => mouse.release(MouseButton::Left),
        2 => {
            let squad = c.squad.unwrap();
            if selection.squad == Some(squad) && selection.soldier.is_some() {
                c.results.push(format!("select=ok (squad {squad})"));
            } else {
                c.results.push(format!("select=FAIL (wanted squad {squad}, got {:?})", selection.squad));
                c.passed = false;
            }
            // A point near the squad that is on screen and, in 3D, not hidden behind a ridge.
            let (x, y) = squad_anchor(&link, squad).unwrap();
            let t = &link.sim.terrain;
            let open = |p: Vec2| p.x > 520.0 && p.x < window.width() - 40.0 && p.y > 60.0 && p.y < window.height() - 40.0;
            let visible = |target: (f32, f32), p: Vec2| match *mode {
                ViewMode::Map => true,
                ViewMode::Terrain => terrain_cam
                    .0
                    .viewport_to_world(terrain_cam.1, p)
                    .ok()
                    .and_then(|ray| ground_hit(t, ray.origin, *ray.direction))
                    .is_some_and(|(hx, hy)| ((hx - target.0).powi(2) + (hy - target.1).powi(2)).sqrt() <= ORDER_TOLERANCE_M),
            };
            let found = [(150.0, 0.0), (-150.0, 0.0), (0.0, -150.0), (0.0, 150.0), (100.0, -100.0), (-100.0, -100.0)]
                .into_iter()
                .map(|(dx, dy)| (x + dx, y + dy))
                .filter(|&(tx, ty)| t.in_bounds(tx, ty))
                .find_map(|target| {
                    let p = on_screen(*mode, &link, terrain_cam, map_cam, target, 0.0)?;
                    (open(p) && visible(target, p)).then_some((target, p))
                });
            match found {
                Some((target, p)) => {
                    c.target = target;
                    cursor.0 = Some(p);
                    mouse.press(MouseButton::Right);
                }
                None => {
                    c.results.push("order=FAIL (no visible point near the squad)".into());
                    c.passed = false;
                    c.step = SWITCH;
                    return;
                }
            }
        }
        3 => mouse.release(MouseButton::Right),
        4 => {
            let squad = c.squad.unwrap();
            let msg = link.last_message.clone().unwrap_or_default();
            let ordered = msg.starts_with(&format!("Squad {squad} ordered to ("));
            let at = msg
                .split(['(', ')'])
                .nth(1)
                .and_then(|xy| xy.split_once(", "))
                .and_then(|(x, y)| Some((x.parse::<f32>().ok()?, y.parse::<f32>().ok()?)));
            let off = at.map(|(x, y)| ((x - c.target.0).powi(2) + (y - c.target.1).powi(2)).sqrt());
            match off {
                Some(d) if ordered && d <= ORDER_TOLERANCE_M => c.results.push(format!("order=ok ({d:.1} m from the point clicked)")),
                _ => {
                    c.results.push(format!("order=FAIL ({msg})"));
                    c.passed = false;
                }
            }
            c.step = if *mode == ViewMode::Map { MOVE } else { SWITCH };
            return;
        }
        // Mouse wheel: one notch in, about a point right of the centre.
        MOVE => {
            c.rest = Vec2::new(window.width() * 0.65, window.height() * 0.45);
            c.under = map_cam.0.viewport_to_world_2d(map_cam.1, c.rest).unwrap_or_default();
            c.before = Some(*map);
            cursor.0 = Some(c.rest);
            scroll.delta = Vec2::new(0.0, 1.0);
            scroll.unit = MouseScrollUnit::Line;
        }
        n if n == MOVE + 1 => {
            let before = c.before.unwrap();
            let now_under = map_cam.0.viewport_to_world_2d(map_cam.1, c.rest).unwrap_or_default();
            let drift_px = now_under.distance(c.under) / map.scale;
            let ratio = map.scale / before.scale;
            if (ratio - 0.8).abs() < 0.01 && drift_px < 1.5 {
                c.results.push(format!("zoom=ok (closer by {:.0}%, the point under the cursor moved {drift_px:.1} px)", (1.0 - ratio) * 100.0));
            } else {
                c.results.push(format!("zoom=FAIL (scale x{ratio:.3}, the point under the cursor moved {drift_px:.1} px)"));
                c.passed = false;
            }
            c.before = Some(*map);
            keys.press(KeyCode::KeyW);
        }
        // W held for ten frames, then released.
        n if n == MOVE + 12 => {
            keys.release(KeyCode::KeyW);
            let before = c.before.unwrap();
            let moved = map.centre - before.centre;
            if moved.y > 0.0 && moved.x.abs() < 0.01 {
                c.results.push(format!("keys=ok (W moved the map {:.0} m north)", moved.y));
            } else {
                c.results.push(format!("keys=FAIL (W moved the map by {moved:?})"));
                c.passed = false;
            }
        }
        // Right-drag: press, move the mouse, release.
        n if n == MOVE + 13 => {
            c.before = Some(*map);
            c.message_before = link.last_message.clone();
            mouse.press(MouseButton::Right);
        }
        n if n == MOVE + 14 => motion.delta = Vec2::new(DRAG_PX, 0.0),
        n if n == MOVE + 15 => mouse.release(MouseButton::Right),
        n if n == MOVE + 16 => {
            let before = c.before.unwrap();
            let moved_px = (before.centre.x - map.centre.x) / map.scale;
            let ordered = link.last_message != c.message_before;
            if (moved_px - DRAG_PX).abs() < 0.5 && !ordered {
                c.results.push(format!("drag=ok (the map followed the mouse {moved_px:.0} px, no order given)"));
            } else {
                c.results.push(format!("drag=FAIL (moved {moved_px:.1} px, order given: {ordered})"));
                c.passed = false;
            }
            c.step = SWITCH;
            return;
        }
        SWITCH => {
            cursor.0 = None;
            keys.press(KeyCode::KeyM);
        }
        n if n == SWITCH + 1 => keys.release(KeyCode::KeyM),
        n if n == SWITCH + 2 => {
            let started = c.started_in.unwrap_or(*mode);
            if *mode != started {
                c.results.push(format!("switch=ok (now {})", view_name(*mode)));
            } else {
                c.results.push("switch=FAIL (M did not change the view)".into());
                c.passed = false;
            }
            c.step = SKIP;
            return;
        }
        // The skip-ahead: press N, then watch.
        SKIP => {
            if link.sim.clock.is_none() {
                c.results.push("skip=none (no calendar)".into());
                c.step = SKIP + SKIP_FRAMES + 1;
                return;
            }
            c.skip_from_tick = link.sim.tick;
            c.message_before = link.last_message.clone();
            keys.press(KeyCode::KeyN);
        }
        n if n == SKIP + 1 => keys.release(KeyCode::KeyN),
        n if n > SKIP + 1 && n <= SKIP + SKIP_FRAMES => {
            // Stopped by an event (its message names the time, "HH:MM: ...") or the dawn screen.
            let stopped = !link.skipping
                && (link.dawn_screen
                    || link.last_message.as_ref().is_some_and(|m| m.as_bytes().get(5) == Some(&b':') && link.last_message != c.message_before));
            if stopped || n == SKIP + SKIP_FRAMES {
                let ran = link.sim.tick - c.skip_from_tick;
                let frames = n - SKIP - 1;
                // At 64x, a frame of about 1/60 s runs about 11 updates.
                let fast = ran as f32 / frames as f32 > 30.0;
                let minutes = ran as f64 * link.sim.tuning.dt() as f64 / 60.0;
                if stopped {
                    c.results.push(format!(
                        "skip=ok (ran {minutes:.1} game minutes in {frames} frames, then stopped: {})",
                        if link.dawn_screen { "dawn".to_string() } else { link.last_message.clone().unwrap_or_default() }
                    ));
                } else if fast {
                    c.results.push(format!("skip=ok (ran {minutes:.1} game minutes in {frames} frames)"));
                } else {
                    c.results.push(format!("skip=FAIL (only {ran} updates in {frames} frames)"));
                    c.passed = false;
                }
                c.step = SKIP + SKIP_FRAMES + 1;
                return;
            }
        }
        n if n == SKIP + SKIP_FRAMES + 1 => {
            let started = c.started_in.unwrap_or(*mode);
            println!(
                "CLICK-CHECK view={} {} {}",
                view_name(started),
                if c.passed { "PASS" } else { "FAIL" },
                c.results.join(" ")
            );
            exit.write(AppExit::Success);
        }
        _ => {}
    }
    c.step += 1;
}
