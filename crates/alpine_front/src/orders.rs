//! Mouse orders, shared by both views. Left-click selects a squad (and one of its men); right-click
//! the ground without dragging sends the selected Austro-Hungarian squad there. Only finding what is
//! under the cursor differs between the 3D view and the map.

use crate::map_view::{FLAG_PICK_OFFSET_PX, MapView, squad_anchor};
use crate::sim_link::SimLink;
use crate::terrain_view::{ground_hit, to_world};
use crate::units::{Selection, marker_scale};
use crate::view::{MapCam, TerrainCam, ViewMode};
use alpine_sim::sim::{Order, Side};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::input::EguiWantsInput;

/// How close (in screen pixels) the cursor must be to a man or a squad flag to select it.
const PICK_PX: f32 = 18.0;

#[derive(Resource, Default)]
pub struct Clicks {
    pub left_start: Option<Vec2>,
    /// Set while the right button is held after being pressed outside the panels.
    pub right_start: Option<Vec2>,
    pub right_dragged: f32,
}

/// For the automatic click check (`click_check.rs`): a cursor position that replaces the real one.
#[derive(Resource, Default)]
pub struct CursorOverride(pub Option<Vec2>);

pub struct OrdersPlugin;

impl Plugin for OrdersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Clicks>().init_resource::<CursorOverride>().add_systems(Update, clicks);
    }
}

type Cam<'a> = (&'a Camera, &'a GlobalTransform);

/// The man nearest the cursor in the 3D view.
fn pick_terrain(link: &SimLink, (camera, cam_tf): Cam, cursor: Vec2) -> Option<u32> {
    let sim = &link.sim;
    let cam_pos = cam_tf.translation();
    let mut best: Option<(u32, f32)> = None;
    for s in &sim.soldiers {
        let (x, y) = link.drawn_position(s.id as usize);
        let ground = to_world(x, y, sim.terrain.height(x, y));
        let centre = ground + Vec3::Y * 0.8 * marker_scale(cam_pos.distance(ground));
        if let Ok(p) = camera.world_to_viewport(cam_tf, centre) {
            let d = p.distance(cursor);
            if d < PICK_PX && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((s.id, d));
            }
        }
    }
    best.map(|(id, _)| id)
}

/// On the map: the nearest man when zoomed in, otherwise the nearest squad flag (giving its first man,
/// so "what this man can see" still has an observer).
fn pick_map(link: &SimLink, map: &MapView, (camera, cam_tf): Cam, cursor: Vec2) -> Option<u32> {
    let sim = &link.sim;
    let mut best: Option<(u32, f32)> = None;
    if map.shows_men() {
        for s in &sim.soldiers {
            let (x, y) = link.drawn_position(s.id as usize);
            if let Ok(p) = camera.world_to_viewport(cam_tf, Vec3::new(x, y, 0.0)) {
                let d = p.distance(cursor);
                if d < PICK_PX && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((s.id, d));
                }
            }
        }
    } else {
        for q in &sim.squads {
            let Some((x, y)) = squad_anchor(link, q.id) else { continue };
            if let Ok(p) = camera.world_to_viewport(cam_tf, Vec3::new(x, y, 0.0)) {
                let d = (p + FLAG_PICK_OFFSET_PX).distance(cursor);
                if d < PICK_PX && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((q.members[0], d));
                }
            }
        }
    }
    best.map(|(id, _)| id)
}

/// The ground under the cursor, in simulation metres.
fn ground_under(link: &SimLink, mode: ViewMode, terrain: Cam, map: Cam, cursor: Vec2) -> Option<(f32, f32)> {
    let t = &link.sim.terrain;
    match mode {
        ViewMode::Terrain => {
            let ray = terrain.0.viewport_to_world(terrain.1, cursor).ok()?;
            ground_hit(t, ray.origin, *ray.direction)
        }
        ViewMode::Map => {
            let p = map.0.viewport_to_world_2d(map.1, cursor).ok()?;
            t.in_bounds(p.x, p.y).then_some((p.x, p.y))
        }
    }
}

#[allow(clippy::too_many_arguments)] // a Bevy system: each argument is a piece of game state it reads
pub fn clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    window: Single<&Window, With<PrimaryWindow>>,
    mode: Res<ViewMode>,
    terrain_cam: Single<Cam, With<TerrainCam>>,
    map_cam: Single<Cam, With<MapCam>>,
    map: Res<MapView>,
    egui: Res<EguiWantsInput>,
    cursor_override: Res<CursorOverride>,
    mut state: ResMut<Clicks>,
    mut link: ResMut<SimLink>,
    mut selection: ResMut<Selection>,
) {
    let cursor = cursor_override.0.or(window.cursor_position());
    let over_ui = egui.wants_any_pointer_input() && cursor_override.0.is_none();
    let (terrain_cam, map_cam) = (*terrain_cam, *map_cam);

    if buttons.just_pressed(MouseButton::Left) && !over_ui {
        state.left_start = cursor;
    }
    if buttons.just_released(MouseButton::Left)
        && let (Some(start), Some(c)) = (state.left_start.take(), cursor)
        && start.distance(c) < 5.0
    {
        let picked = match *mode {
            ViewMode::Terrain => pick_terrain(&link, terrain_cam, c),
            ViewMode::Map => pick_map(&link, &map, map_cam, c),
        };
        selection.soldier = picked;
        selection.squad = picked.map(|id| link.sim.soldiers[id as usize].squad);
    }

    if buttons.just_pressed(MouseButton::Right) {
        state.right_start = if over_ui { None } else { cursor };
        state.right_dragged = 0.0;
    }
    if buttons.pressed(MouseButton::Right) {
        state.right_dragged += motion.delta.length();
    }
    if buttons.just_released(MouseButton::Right) {
        let start = state.right_start.take();
        if let (Some(_), Some(c), Some(squad)) = (start, cursor, selection.squad)
            && state.right_dragged < 5.0
        {
            let side = link.sim.squads[squad as usize].side;
            if side != Side::Austria {
                link.last_message = Some("Italian squads follow their own patrols; select an Austro-Hungarian squad.".into());
            } else {
                match ground_under(&link, *mode, terrain_cam, map_cam, c) {
                    Some((x, y)) => {
                        link.sim.order(Order::Move { squad, x, y });
                        link.last_message = Some(format!(
                            "Squad {squad} ordered to ({x:.0}, {y:.0}), {:.0} m up.",
                            link.sim.terrain.height(x, y)
                        ));
                    }
                    None => link.last_message = Some("That is off the map.".into()),
                }
            }
        }
    }
}
