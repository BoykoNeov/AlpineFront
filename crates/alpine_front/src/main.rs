//! Alpine Front: the playable view. Bevy draws the simulation's state and turns clicks into orders;
//! the simulation itself lives in `alpine_sim` and never depends on anything here.
//!
//! Usage:
//!   alpine_front [--seed N] [--autoplay] [--replay FILE] [--frame-check SECONDS]
//!                [--advance-minutes M] [--camera X,Y,Z,TX,TY,TZ] [--overlays a,b] [--select ID]
//!                [--screenshot FILE]
//!
//! The last group is for automated visual checks: fast-forward the simulation, place the camera
//! (simulation metres: east, north, height, then the point to look at), switch overlays on
//! (contours, steep, viewshed, routes, nospotting), select a soldier, save a screenshot and exit.

mod camera;
mod frame_check;
mod shots;
mod overlays;
mod sim_link;
mod terrain_view;
mod ui;
mod units;

use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};
use std::path::PathBuf;

/// Command-line options.
#[derive(Resource, Clone, Debug)]
pub struct Options {
    pub seed: u64,
    pub autoplay: bool,
    pub replay: Option<PathBuf>,
    /// Speed-pass mode: fly the camera over the map for this many seconds, report frame times, exit.
    pub frame_check: Option<f32>,
    pub advance_minutes: f32,
    pub camera: Option<[f32; 6]>,
    pub overlays: Option<String>,
    pub select: Option<u32>,
    pub screenshot: Option<PathBuf>,
}

const USAGE: &str = "usage: alpine_front [--seed N] [--autoplay] [--replay FILE] [--frame-check SECONDS] \
    [--advance-minutes M] [--camera X,Y,Z,TX,TY,TZ] [--overlays a,b] [--select ID] [--screenshot FILE]";

fn parse_options() -> Result<Options, String> {
    let mut o = Options {
        seed: 1916,
        autoplay: false,
        replay: None,
        frame_check: None,
        advance_minutes: 0.0,
        camera: None,
        overlays: None,
        select: None,
        screenshot: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seed" => o.seed = args.next().and_then(|v| v.parse().ok()).ok_or("--seed needs a number")?,
            "--autoplay" => o.autoplay = true,
            "--replay" => o.replay = Some(args.next().ok_or("--replay needs a file")?.into()),
            "--frame-check" => {
                o.frame_check = Some(args.next().and_then(|v| v.parse().ok()).ok_or("--frame-check needs seconds")?)
            }
            "--advance-minutes" => {
                o.advance_minutes = args.next().and_then(|v| v.parse().ok()).ok_or("--advance-minutes needs a number")?
            }
            "--camera" => {
                let v: Vec<f32> = args
                    .next()
                    .ok_or("--camera needs X,Y,Z,TX,TY,TZ")?
                    .split(',')
                    .map(|p| p.trim().parse::<f32>())
                    .collect::<Result<_, _>>()
                    .map_err(|_| "--camera needs six numbers")?;
                o.camera = Some(v.try_into().map_err(|_| "--camera needs six numbers")?);
            }
            "--overlays" => o.overlays = Some(args.next().ok_or("--overlays needs a list")?),
            "--select" => o.select = Some(args.next().and_then(|v| v.parse().ok()).ok_or("--select needs a soldier id")?),
            "--screenshot" => o.screenshot = Some(args.next().ok_or("--screenshot needs a file")?.into()),
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
        title: "Alpine Front - Col di Lana (build step 1)".into(),
        // The frame check uses exactly 2560x1440 pixels whatever the desktop scaling, and presents
        // frames as soon as they are ready so nothing caps the frame rate.
        resolution: if checking {
            WindowResolution::new(2560, 1440).with_scale_factor_override(1.0)
        } else {
            (1600, 900).into()
        },
        present_mode: if checking { PresentMode::Immediate } else { PresentMode::AutoVsync },
        ..default()
    };

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin { primary_window: Some(window), ..default() }))
        .add_plugins(bevy_egui::EguiPlugin::default())
        .insert_resource(options)
        .insert_resource(link)
        .add_plugins((
            sim_link::SimLinkPlugin,
            terrain_view::TerrainViewPlugin,
            units::UnitsPlugin,
            camera::CameraPlugin,
            overlays::OverlaysPlugin,
            ui::UiPlugin,
            frame_check::FrameCheckPlugin,
            shots::ShotsPlugin,
        ))
        .run();
}
