//! Speed-pass frame check (`--frame-check SECONDS`): flies the camera over the map at top game speed
//! with vsync off, records every frame, prints a summary line and exits.
//!
//! Three measures, because graphics drivers often cap the frame rate (for example 119 fps on a
//! 120 Hz screen, or 60 fps when the window is in the background), which hides how much work a
//! frame really takes:
//! - the time between frames (what the player sees, but held up by any cap);
//! - the processor's work per frame on the main thread (from the start to the end of each update);
//! - the graphics card's work per frame (summed timings of Bevy's drawing passes).

use crate::Options;
use crate::sim_link::SimLink;
use crate::terrain_view::to_world;
use bevy::diagnostic::DiagnosticsStore;
use bevy::prelude::*;
use std::time::Instant;

const WARM_UP_S: f32 = 5.0;

#[derive(Resource, Default)]
struct FrameRecord {
    intervals: Vec<f32>,
    cpu: Vec<f32>,
    gpu: Vec<f32>,
    update_started: Option<Instant>,
}

pub struct FrameCheckPlugin;

impl Plugin for FrameCheckPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameRecord>()
            .add_systems(First, start_update)
            .add_systems(Update, run_check)
            .add_systems(Last, end_update);
        if std::env::args().any(|a| a == "--frame-check") {
            app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
        }
    }
}

fn start_update(options: Res<Options>, mut record: ResMut<FrameRecord>) {
    if options.frame_check.is_some() {
        record.update_started = Some(Instant::now());
    }
}

fn end_update(options: Res<Options>, time: Res<Time>, diagnostics: Res<DiagnosticsStore>, mut record: ResMut<FrameRecord>) {
    if options.frame_check.is_none() || time.elapsed_secs() < WARM_UP_S {
        return;
    }
    if let Some(s) = record.update_started.take() {
        record.cpu.push(s.elapsed().as_secs_f32() * 1000.0);
    }
    // Graphics card time: the latest reading of each top-level drawing pass ("render/<pass>/elapsed_gpu").
    let gpu: f64 = diagnostics
        .iter()
        .filter(|d| {
            let parts: Vec<&str> = d.path().components().collect();
            parts.len() == 3 && parts[0] == "render" && parts[2] == "elapsed_gpu"
        })
        .filter_map(|d| d.value())
        .sum();
    if gpu > 0.0 {
        record.gpu.push(gpu as f32);
    }
}

fn percentiles(v: &[f32]) -> (f32, f32, f32) {
    if v.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut s = v.to_vec();
    s.sort_by(f32::total_cmp);
    let pct = |p: f32| s[((s.len() - 1) as f32 * p).round() as usize];
    (pct(0.5), pct(0.99), s[s.len() - 1])
}

fn run_check(
    time: Res<Time>,
    options: Res<Options>,
    link: Res<SimLink>,
    window: Single<&Window>,
    mut record: ResMut<FrameRecord>,
    mut camera: Single<&mut Transform, With<Camera3d>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(seconds) = options.frame_check else { return };
    let t = time.elapsed_secs();
    // A loop around the summit, swinging between 600 m and 2,400 m out and 150 m to 650 m up.
    let a = t * 0.12;
    let r = 1500.0 + 900.0 * (t * 0.21).sin();
    let terrain = &link.sim.terrain;
    let (x, y) = (r * a.cos(), r * a.sin());
    let pos = to_world(x, y, terrain.height(x, y) + 400.0 + 250.0 * (t * 0.17).sin());
    let target = to_world(0.3 * x, 0.3 * y, 2300.0);
    **camera = Transform::from_translation(pos).looking_at(target, Vec3::Y);

    if t < WARM_UP_S {
        return;
    }
    record.intervals.push(time.delta_secs() * 1000.0);
    if t >= WARM_UP_S + seconds {
        let (i50, i99, imax) = percentiles(&record.intervals);
        let (c50, c99, cmax) = percentiles(&record.cpu);
        let (g50, g99, gmax) = percentiles(&record.gpu);
        println!(
            "FRAME-CHECK frames={} p50={i50:.2} p99={i99:.2} max={imax:.2} cpu_p50={c50:.2} cpu_p99={c99:.2} cpu_max={cmax:.2} \
             gpu_p50={g50:.2} gpu_p99={g99:.2} gpu_max={gmax:.2} ms; window {}x{}; simulation tick {:.2} ms (worst {:.2})",
            record.intervals.len(),
            window.physical_width(),
            window.physical_height(),
            link.tick_ms,
            link.tick_ms_worst
        );
        exit.write(AppExit::Success);
    }
}
