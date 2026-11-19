//! Alpine Front: the playable view. Bevy draws the simulation's state and turns clicks into orders;
//! the simulation itself lives in `alpine_sim` and never depends on anything here.
//!
//! Usage:
//!   alpine_front [--seed N] [--step1] [--autoplay] [--replay FILE] [--view terrain|map] [--frame-check SECONDS]
//!                [--advance-minutes M] [--camera X,Y,Z,TX,TY,TZ] [--map X,Y,WIDTH] [--overlays a,b]
//!                [--select ID] [--screenshot FILE]
//!
//! The game is step 2: a week on the summit from dawn on Monday 6 March 1916. `--step1` plays the
//! step-1 scenario instead (no calendar, always daylight).
//!
//! `--view map` starts on the 2D map instead of the 3D view (M switches while playing); with
//! `--frame-check` it measures the map's frames instead of the 3D view's.
//!
//! The last group is for automated visual checks: fast-forward the simulation, place the 3D camera
//! (simulation metres: east, north, height, then the point to look at) or the map (the point at the
//! centre, then how many metres the window spans), switch overlays on (contours, steep, viewshed,
//! routes, nospotting, noplaces), select a soldier, save a screenshot and exit.

mod camera;
mod click_check;
mod frame_check;
mod map_view;
mod orders;
mod overlays;
mod places_view;
mod shots;
mod snow_view;
mod sim_link;
mod terrain_view;
mod ui;
mod units;
mod view;

use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};
use std::path::PathBuf;

/// Command-line options.
#[derive(Resource, Clone, Debug)]
pub struct Options {
    pub seed: u64,
    /// Play the step-1 scenario (no calendar) instead of step 2.
    pub step1: bool,
    pub autoplay: bool,
    pub replay: Option<PathBuf>,
    /// Start on the 2D map.
    pub map_view: bool,
    /// Speed-pass mode: move the camera over the map for this many seconds, report frame times, exit.
    pub frame_check: Option<f32>,
    pub advance_minutes: f32,
    pub camera: Option<[f32; 6]>,
    /// Map centre (east, north) and the width in metres the window spans.
    pub map: Option<[f32; 3]>,
    pub overlays: Option<String>,
    pub select: Option<u32>,
    pub screenshot: Option<PathBuf>,
    /// Checks only: heavy snow falling all the time, and this many centimetres of fresh snow lying
    /// from the start (the speed pass measures the flakes and the snow this way). Not recorded.
    pub snowing: Option<f32>,
}

const USAGE: &str = "usage: alpine_front [--seed N] [--step1] [--autoplay] [--replay FILE] [--view terrain|map] \
    [--frame-check SECONDS] [--advance-minutes M] [--camera X,Y,Z,TX,TY,TZ] [--map X,Y,WIDTH] \
    [--overlays a,b] [--select ID] [--screenshot FILE] [--snowing CM]";

/// `N` comma-separated numbers.
fn numbers<const N: usize>(text: Option<String>, what: &str) -> Result<[f32; N], String> {
    let bad = || format!("{what} needs {N} numbers separated by commas");
    let v: Vec<f32> = text
        .ok_or_else(bad)?
        .split(',')
        .map(|p| p.trim().parse::<f32>())
        .collect::<Result<_, _>>()
        .map_err(|_| bad())?;
    v.try_into().map_err(|_| bad())
}

fn parse_options() -> Result<Options, String> {
    let mut o = Options {
        seed: 1916,
        step1: false,
        autoplay: false,
        replay: None,
        map_view: false,
        frame_check: None,
        advance_minutes: 0.0,
        camera: None,
        map: None,
        overlays: None,
        select: None,
        screenshot: None,
        snowing: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seed" => o.seed = args.next().and_then(|v| v.parse().ok()).ok_or("--seed needs a number")?,
            "--autoplay" => o.autoplay = true,
            "--step1" => o.step1 = true,
            "--replay" => o.replay = Some(args.next().ok_or("--replay needs a file")?.into()),
            "--view" => {
                o.map_view = match args.next().as_deref() {
                    Some("map") => true,
                    Some("terrain") => false,
                    _ => return Err("--view needs 'terrain' or 'map'".into()),
                }
            }
            "--frame-check" => {
                o.frame_check = Some(args.next().and_then(|v| v.parse().ok()).ok_or("--frame-check needs seconds")?)
            }
            "--advance-minutes" => {
                o.advance_minutes = args.next().and_then(|v| v.parse().ok()).ok_or("--advance-minutes needs a number")?
            }
            "--camera" => o.camera = Some(numbers::<6>(args.next(), "--camera")?),
            "--map" => o.map = Some(numbers::<3>(args.next(), "--map")?),
            "--overlays" => o.overlays = Some(args.next().ok_or("--overlays needs a list")?),
            "--select" => o.select = Some(args.next().and_then(|v| v.parse().ok()).ok_or("--select needs a soldier id")?),
            "--screenshot" => o.screenshot = Some(args.next().ok_or("--screenshot needs a file")?.into()),
            "--snowing" => o.snowing = Some(args.next().and_then(|v| v.parse().ok()).ok_or("--snowing needs centimetres")?),
            "--help" | "-h" => return Err(USAGE.into()),
            other => return Err(format!("unknown option '{other}'")),
        }
    }
    Ok(o)
}

fn main() {
    let options = match parse_options() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let link = match sim_link::SimLink::start(&options) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("could not start: {e}");
            std::process::exit(1);
        }
    };
    let checking = options.frame_check.is_some();
    let window = Window {
        title: "Alpine Front - Col di Lana (build step 2)".into(),
        // The frame check uses exactly 2560x1440 pixels whatever the desktop scaling, with vsync off
        // (falling back to vsync where the machine cannot turn it off).
        resolution: if checking {
            WindowResolution::new(2560, 1440).with_scale_factor_override(1.0)
        } else {
            (1600, 900).into()
        },
        present_mode: if checking { PresentMode::AutoNoVsync } else { PresentMode::AutoVsync },
        ..default()
    };

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin { primary_window: Some(window), ..default() }))
        .add_plugins(bevy_egui::EguiPlugin::default())
        // The panels get a camera of their own (view.rs), so they show in both views.
        .insert_resource(bevy_egui::EguiGlobalSettings { auto_create_primary_context: false, ..default() })
        .insert_resource(view::ViewMode::from_options(&options))
        .insert_resource(options)
        .insert_resource(link)
        .add_plugins((
            sim_link::SimLinkPlugin,
            view::ViewPlugin,
            terrain_view::TerrainViewPlugin,
            map_view::MapViewPlugin,
            units::UnitsPlugin,
            camera::CameraPlugin,
            orders::OrdersPlugin,
            overlays::OverlaysPlugin,
            places_view::PlacesViewPlugin,
            ui::UiPlugin,
            frame_check::FrameCheckPlugin,
            shots::ShotsPlugin,
            snow_view::SnowViewPlugin,
            click_check::ClickCheckPlugin,
        ))
        .run();
}
