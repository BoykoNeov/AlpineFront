//! Debug overlays: contour lines, steep ground, what the selected man can see, spotting lines and
//! squad routes. What to show is worked out once here; each view draws it in its own style (the 3D
//! lines are drawn here, the map's in `map_view.rs`).

use crate::map_view::{MapHandles, MapMaterial};
use crate::sim_link::SimLink;
use crate::terrain_view::{FLAG_CONTOURS, FLAG_STEEP, FLAG_VIEWSHED, TerrainHandles, TerrainMaterial, to_world};
use crate::units::Selection;
use crate::view::ViewMode;
use alpine_sim::los;
use alpine_sim::sim::{Side, SquadState};
use bevy::prelude::*;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotLines {
    Off,
    SelectedSquad,
    Everyone,
}

#[derive(Resource)]
pub struct OverlaySettings {
    pub contours: bool,
    pub steep: bool,
    pub viewshed: bool,
    pub spotting: SpotLines,
    pub routes: bool,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        OverlaySettings { contours: true, steep: false, viewshed: false, spotting: SpotLines::Everyone, routes: true }
    }
}

#[derive(Resource, Default)]
pub struct ViewshedState {
    observer: Option<u32>,
    at: (f32, f32),
    computed_at_tick: u64,
    pub last_ms: f32,
}

#[derive(Resource, Default)]
struct RouteCache {
    squad: Option<u32>,
    tick: u64,
    points: Vec<(f32, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeKind {
    /// From a man to an enemy he can see; the side is the one doing the seeing.
    Spotting(Side),
    /// A squad's destination (a single point).
    Destination(Side),
    /// The selected squad's route.
    Route,
    /// The edge of the selected man's spotting range.
    Range,
}

/// One line to draw, in simulation metres (east, north).
pub struct Stroke {
    pub kind: StrokeKind,
    pub points: Vec<(f32, f32)>,
}

/// The overlay lines for this frame, shared by both views.
#[derive(Resource, Default)]
pub struct Strokes(pub Vec<Stroke>);

pub struct OverlaysPlugin;

impl Plugin for OverlaysPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OverlaySettings>()
            .init_resource::<ViewshedState>()
            .init_resource::<RouteCache>()
            .init_resource::<Strokes>()
            .add_systems(Update, (apply_flags, update_viewshed, (collect_strokes, draw_terrain_strokes).chain()));
    }
}

/// Passes the overlay switches to both views' ground shaders.
fn apply_flags(
    settings: Res<OverlaySettings>,
    handles: Option<Res<TerrainHandles>>,
    map: Option<Res<MapHandles>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
    mut map_materials: ResMut<Assets<MapMaterial>>,
) {
    let (Some(handles), Some(map)) = (handles, map) else { return };
    if !settings.is_changed() && !handles.is_added() && !map.is_added() {
        return;
    }
    let mut flags = 0;
    if settings.contours {
        flags |= FLAG_CONTOURS;
    }
    if settings.steep {
        flags |= FLAG_STEEP;
    }
    if settings.viewshed {
        flags |= FLAG_VIEWSHED;
    }
    if let Some(mut m) = materials.get_mut(&handles.material) {
        m.extension.settings.flags = flags;
    }
    if let Some(mut m) = map_materials.get_mut(&map.material) {
        m.settings.flags = flags;
    }
}

/// Recomputes "what the selected man can see" when he changes or moves.
fn update_viewshed(
    settings: Res<OverlaySettings>,
    selection: Res<Selection>,
    link: Res<SimLink>,
    handles: Option<Res<TerrainHandles>>,
    mut state: ResMut<ViewshedState>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(handles) = handles else { return };
    if !settings.viewshed {
        state.observer = None;
        return;
    }
    let Some(obs) = selection.soldier else { return };
    let s = &link.sim.soldiers[obs as usize];
    let moved = ((s.x - state.at.0).powi(2) + (s.y - state.at.1).powi(2)).sqrt();
    let stale = state.observer != Some(obs) || moved > 5.0 || (moved > 0.5 && link.sim.tick >= state.computed_at_tick + 50);
    if !stale {
        return;
    }
    let sim = &link.sim;
    let t = &sim.terrain;
    let radius = sim.tuning.spotting.spotting_range_m;
    let eye = [s.x, s.y, t.height(s.x, s.y) + sim.tuning.soldier.eye_height_m];
    let start = Instant::now();
    let seen = sim.pool().install(|| los::viewshed(t, &sim.los, eye, radius, sim.tuning.soldier.head_height_m));
    let mut data = vec![0u8; seen.len()];
    for r in 0..t.rows {
        for c in 0..t.cols {
            let (x, y) = t.point_xy(c, r);
            if (x - s.x).powi(2) + (y - s.y).powi(2) <= radius * radius {
                let i = t.idx(c, r);
                data[i] = if seen[i] != 0 { 255 } else { 128 };
            }
        }
    }
    state.last_ms = start.elapsed().as_secs_f32() * 1000.0;
    if let Some(mut img) = images.get_mut(&handles.viewshed) {
        img.data = Some(data);
    }
    state.observer = Some(obs);
    state.at = (s.x, s.y);
    state.computed_at_tick = sim.tick;
}

/// Works out this frame's overlay lines.
fn collect_strokes(
    settings: Res<OverlaySettings>,
    selection: Res<Selection>,
    link: Res<SimLink>,
    mut routes: ResMut<RouteCache>,
    mut strokes: ResMut<Strokes>,
) {
    let sim = &link.sim;
    let t = &sim.terrain;
    let out = &mut strokes.0;
    out.clear();

    if settings.spotting != SpotLines::Off {
        for s in &sim.soldiers {
            if settings.spotting == SpotLines::SelectedSquad && selection.squad != Some(s.squad) {
                continue;
            }
            for &e in &s.sees {
                out.push(Stroke {
                    kind: StrokeKind::Spotting(s.side),
                    points: vec![link.drawn_position(s.id as usize), link.drawn_position(e as usize)],
                });
            }
        }
    }

    if settings.routes {
        for q in &sim.squads {
            if !matches!(q.state, SquadState::Moving | SquadState::Planning { .. }) {
                continue;
            }
            if let Some(d) = q.dest {
                out.push(Stroke { kind: StrokeKind::Destination(q.side), points: vec![d] });
            }
        }
        // Route of the selected squad, refreshed every couple of game seconds.
        match selection.squad.map(|q| &sim.squads[q as usize]) {
            Some(q) if q.state == SquadState::Moving => {
                if routes.squad != Some(q.id) || sim.tick >= routes.tick + 20 {
                    let lead = &sim.soldiers[q.members[0] as usize];
                    let field = q.field.as_ref().unwrap();
                    routes.points = field.route_from(t, &sim.tuning.walking, lead.x, lead.y, 2000);
                    routes.squad = Some(q.id);
                    routes.tick = sim.tick;
                }
                out.push(Stroke { kind: StrokeKind::Route, points: routes.points.clone() });
            }
            _ => routes.squad = None,
        }
    }

    // Spotting range around the selected man.
    if settings.viewshed
        && let Some(obs) = selection.soldier
    {
        let (cx, cy) = link.drawn_position(obs as usize);
        let r = sim.tuning.spotting.spotting_range_m;
        let ring = (0..=180)
            .map(|k| {
                let a = k as f32 / 180.0 * std::f32::consts::TAU;
                let (x, y) = (cx + r * a.cos(), cy + r * a.sin());
                (x.clamp(t.min_x(), t.max_x()), y.clamp(t.min_y(), t.max_y()))
            })
            .collect();
        out.push(Stroke { kind: StrokeKind::Range, points: ring });
    }
}

/// Side colours in the 3D view: own troops blue, Italians red (decision 27).
pub fn terrain_side_colour(side: Side) -> Color {
    match side {
        Side::Austria => Color::srgb(0.4, 0.7, 1.0),
        Side::Italy => Color::srgb(1.0, 0.35, 0.3),
    }
}

/// Draws the overlay lines in the 3D view, following the ground.
fn draw_terrain_strokes(mut gizmos: Gizmos, mode: Res<ViewMode>, link: Res<SimLink>, strokes: Res<Strokes>) {
    if *mode != ViewMode::Terrain {
        return;
    }
    let sim = &link.sim;
    let t = &sim.terrain;
    let eye = sim.tuning.soldier.eye_height_m;
    let at = |(x, y): (f32, f32), lift: f32| to_world(x, y, t.height(x, y) + lift);
    for s in &strokes.0 {
        match s.kind {
            StrokeKind::Spotting(side) => {
                gizmos.line(at(s.points[0], eye), at(s.points[1], eye), terrain_side_colour(side).with_alpha(0.8));
            }
            StrokeKind::Destination(side) => {
                let g = at(s.points[0], 0.0);
                gizmos.line(g, g + Vec3::Y * 40.0, terrain_side_colour(side));
            }
            StrokeKind::Route => {
                gizmos.linestrip(s.points.iter().map(|&p| at(p, 2.0)), Color::srgb(1.0, 1.0, 0.3));
            }
            StrokeKind::Range => {
                gizmos.linestrip(s.points.iter().map(|&p| at(p, 3.0)), Color::srgb(1.0, 0.85, 0.3));
            }
        }
    }
}
