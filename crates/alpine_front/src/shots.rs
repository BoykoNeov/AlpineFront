//! Automated visual checks: apply the overlay and selection options at start, then save a
//! screenshot after the scene has settled and exit.

use crate::Options;
use crate::overlays::{OverlaySettings, SpotLines};
use crate::sim_link::SimLink;
use crate::units::Selection;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

const SETTLE_S: f32 = 4.0;
const EXIT_AFTER_S: f32 = 6.0;

pub struct ShotsPlugin;

impl Plugin for ShotsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, apply_options).add_systems(Update, take_shot);
    }
}

fn apply_options(options: Res<Options>, link: Res<SimLink>, mut overlays: ResMut<OverlaySettings>, mut selection: ResMut<Selection>) {
    if let Some(list) = &options.overlays {
        for item in list.split(',').map(|s| s.trim()) {
            match item {
                "contours" => overlays.contours = true,
                "nocontours" => overlays.contours = false,
                "steep" => overlays.steep = true,
                "viewshed" => overlays.viewshed = true,
                "routes" => overlays.routes = true,
                "noplaces" => overlays.places = false,
                "nospotting" => overlays.spotting = SpotLines::Off,
                "squadspotting" => overlays.spotting = SpotLines::SelectedSquad,
                _ => warn!("unknown overlay '{item}'"),
            }
        }
    }
    if let Some(id) = options.select
        && let Some(s) = link.sim.soldiers.get(id as usize)
    {
        selection.soldier = Some(id);
        selection.squad = Some(s.squad);
    }
}

fn take_shot(
    mut commands: Commands,
    time: Res<Time>,
    options: Res<Options>,
    mut taken: Local<bool>,
    mut link: ResMut<SimLink>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = &options.screenshot else { return };
    // Hold the simulation still so the picture shows the fast-forwarded moment.
    link.paused = true;
    let t = time.elapsed_secs();
    if !*taken && t >= SETTLE_S {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
        *taken = true;
    }
    if t >= EXIT_AFTER_S {
        exit.write(AppExit::Success);
    }
}
