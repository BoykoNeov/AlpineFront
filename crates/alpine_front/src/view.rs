//! The two views of equal standing (design brief, Presentation): the 3D terrain and the 2D period map.
//! M switches between them. Each view has its own camera; only the active one draws. The side panels
//! have a camera of their own, so they stay on screen whichever view is showing.

use crate::Options;
use bevy::camera::ClearColorConfig;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy_egui::PrimaryEguiContext;
use bevy_egui::input::EguiWantsInput;

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    Terrain,
    Map,
}

impl ViewMode {
    pub fn from_options(o: &Options) -> ViewMode {
        if o.map_view { ViewMode::Map } else { ViewMode::Terrain }
    }
}

/// The 3D view's camera.
#[derive(Component)]
pub struct TerrainCam;

/// The map's camera.
#[derive(Component)]
pub struct MapCam;

/// Everything drawn on the map lives on this layer, which only the map's camera sees.
pub const MAP_LAYER: usize = 1;
/// An empty layer: the panels' camera draws nothing of the world.
const PANEL_LAYER: usize = 31;

pub fn map_layer() -> RenderLayers {
    RenderLayers::layer(MAP_LAYER)
}

pub struct ViewPlugin;

impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_panel_camera).add_systems(Update, (switch_key, activate_cameras).chain());
    }
}

fn spawn_panel_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Camera { order: 10, clear_color: ClearColorConfig::None, ..default() },
        RenderLayers::layer(PANEL_LAYER),
        PrimaryEguiContext,
    ));
}

pub fn switch_key(keys: Res<ButtonInput<KeyCode>>, egui: Res<EguiWantsInput>, mut mode: ResMut<ViewMode>) {
    if !egui.wants_any_keyboard_input() && keys.just_pressed(KeyCode::KeyM) {
        *mode = match *mode {
            ViewMode::Terrain => ViewMode::Map,
            ViewMode::Map => ViewMode::Terrain,
        };
    }
}

fn activate_cameras(
    mode: Res<ViewMode>,
    mut terrain: Query<&mut Camera, (With<TerrainCam>, Without<MapCam>)>,
    mut map: Query<&mut Camera, (With<MapCam>, Without<TerrainCam>)>,
) {
    for mut c in &mut terrain {
        let on = *mode == ViewMode::Terrain;
        if c.is_active != on {
            c.is_active = on;
        }
    }
    for mut c in &mut map {
        let on = *mode == ViewMode::Map;
        if c.is_active != on {
            c.is_active = on;
        }
    }
}
