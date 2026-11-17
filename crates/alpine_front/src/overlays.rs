//! Debug overlays: contour lines, steep ground, what the selected man can see, spotting lines and
//! squad routes.

use crate::sim_link::SimLink;
use crate::terrain_view::{FLAG_CONTOURS, FLAG_STEEP, FLAG_VIEWSHED, TerrainHandles, TerrainMaterial, to_world};
use crate::units::Selection;
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
    points: Vec<Vec3>,
}

pub struct OverlaysPlugin;

impl Plugin for OverlaysPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OverlaySettings>()
            .init_resource::<ViewshedState>()
            .init_resource::<RouteCache>()
            .add_systems(Update, (apply_flags, update_viewshed, draw_lines));
    }
}

fn apply_flags(
    settings: Res<OverlaySettings>,
    handles: Option<Res<TerrainHandles>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    let Some(handles) = handles else { return };
    if !settings.is_changed() && !handles.is_added() {
        return;
    }
    if let Some(mut m) = materials.get_mut(&handles.material) {
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
        m.extension.settings.flags = flags;
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

fn draw_lines(
    mut gizmos: Gizmos,
    settings: Res<OverlaySettings>,
    selection: Res<Selection>,
    link: Res<SimLink>,
    mut routes: ResMut<RouteCache>,
) {
    let sim = &link.sim;
    let t = &sim.terrain;
    let eye = sim.tuning.soldier.eye_height_m;
    let at = |i: usize| {
        let (x, y) = link.drawn_position(i);
        to_world(x, y, t.height(x, y) + eye)
    };

    if settings.spotting != SpotLines::Off {
        for s in &sim.soldiers {
            if settings.spotting == SpotLines::SelectedSquad && selection.squad != Some(s.squad) {
                continue;
            }
            let colour = match s.side {
                Side::Austria => Color::srgba(0.5, 0.8, 1.0, 0.8),
                Side::Italy => Color::srgba(1.0, 0.6, 0.2, 0.8),
            };
            for &e in &s.sees {
                gizmos.line(at(s.id as usize), at(e as usize), colour);
            }
        }
    }

    if settings.routes {
        for q in &sim.squads {
            if !matches!(q.state, SquadState::Moving | SquadState::Planning { .. }) {
                continue;
            }
            if let Some((x, y)) = q.dest {
                let g = to_world(x, y, t.height(x, y));
                let colour = if q.side == Side::Austria { Color::srgb(0.4, 0.7, 1.0) } else { Color::srgb(1.0, 0.55, 0.2) };
                gizmos.line(g, g + Vec3::Y * 40.0, colour);
            }
        }
        // Route of the selected squad, refreshed every couple of game seconds.
        match selection.squad.map(|q| &sim.squads[q as usize]) {
            Some(q) if q.state == SquadState::Moving => {
                if routes.squad != Some(q.id) || sim.tick >= routes.tick + 20 {
                    let lead = &sim.soldiers[q.members[0] as usize];
                    let field = q.field.as_ref().unwrap();
                    routes.points = field
                        .route_from(t, &sim.tuning.walking, lead.x, lead.y, 2000)
                        .into_iter()
                        .map(|(x, y)| to_world(x, y, t.height(x, y) + 2.0))
                        .collect();
                    routes.squad = Some(q.id);
                    routes.tick = sim.tick;
                }
                gizmos.linestrip(routes.points.iter().copied(), Color::srgb(1.0, 1.0, 0.3));
            }
            _ => routes.squad = None,
        }
    }

    // Spotting range around the selected man, following the ground.
    if settings.viewshed
        && let Some(obs) = selection.soldier
    {
        let (cx, cy) = link.drawn_position(obs as usize);
        let r = sim.tuning.spotting.spotting_range_m;
        let ring = (0..=180).map(|k| {
            let a = k as f32 / 180.0 * std::f32::consts::TAU;
            let (x, y) = (cx + r * a.cos(), cy + r * a.sin());
            let (x, y) = (x.clamp(t.min_x(), t.max_x()), y.clamp(t.min_y(), t.max_y()));
            to_world(x, y, t.height(x, y) + 3.0)
        });
        gizmos.linestrip(ring, Color::srgb(1.0, 0.85, 0.3));
    }
}
