//! Screens (egui): time controls, the view switch, overlay switches, the selected squad, replays,
//! help, and the map's scale bar.

use crate::map_view::MapView;
use crate::overlays::{OverlaySettings, SpotLines, ViewshedState};
use crate::sim_link::{ReplayStatus, SPEEDS, SimLink};
use crate::units::Selection;
use crate::view::ViewMode;
use alpine_sim::sim::{Side, SquadState};
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(Update, shortcuts)
            .add_systems(EguiPrimaryContextPass, panels);
    }
}

fn clock(seconds: f64) -> String {
    let s = seconds as u64;
    format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

fn shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    egui: Res<EguiWantsInput>,
    mut link: ResMut<SimLink>,
    mut selection: ResMut<Selection>,
) {
    if egui.wants_any_keyboard_input() {
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        link.paused = !link.paused;
    }
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
    ];
    for (k, s) in digits.iter().zip(SPEEDS) {
        if keys.just_pressed(*k) {
            link.speed = s;
            link.paused = false;
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        selection.soldier = None;
        selection.squad = None;
    }
    if keys.just_pressed(KeyCode::F5) {
        save(&mut link);
    }
}

fn save(link: &mut SimLink) {
    link.last_message = Some(match link.save_replay() {
        Ok(p) => format!("Replay saved: {}", p.display()),
        Err(e) => format!("Could not save the replay: {e}"),
    });
}

/// A scale bar like those on printed maps: alternating black and white quarters, labelled in metres.
fn scale_bar(ctx: &egui::Context, metres_per_px: f32) {
    // The longest round length that fits in about 220 pixels.
    let length = [50.0f32, 100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0]
        .into_iter()
        .rev()
        .find(|m| m / metres_per_px <= 220.0)
        .unwrap_or(50.0);
    let width = length / metres_per_px;
    egui::Area::new("scale_bar".into())
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-24.0, -24.0))
        .interactable(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width + 40.0, 34.0), egui::Sense::hover());
            let p = ui.painter();
            let ink = egui::Color32::from_rgb(40, 30, 22);
            let paper = egui::Color32::from_rgb(236, 226, 202);
            p.rect_filled(rect, 3.0, paper.gamma_multiply(0.9));
            let left = rect.left() + 20.0;
            let top = rect.top() + 18.0;
            for k in 0..4 {
                let x0 = left + width * k as f32 / 4.0;
                let r = egui::Rect::from_min_size(egui::pos2(x0, top), egui::vec2(width / 4.0, 6.0));
                p.rect_filled(r, 0.0, if k % 2 == 0 { ink } else { paper });
            }
            p.rect_stroke(
                egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, 6.0)),
                0.0,
                egui::Stroke::new(1.0, ink),
                egui::StrokeKind::Middle,
            );
            let font = egui::FontId::proportional(12.0);
            p.text(egui::pos2(left, top - 3.0), egui::Align2::CENTER_BOTTOM, "0", font.clone(), ink);
            let label = if length >= 1000.0 { format!("{} km", length / 1000.0) } else { format!("{length} m") };
            p.text(egui::pos2(left + width, top - 3.0), egui::Align2::CENTER_BOTTOM, label, font, ink);
        });
}

#[allow(clippy::too_many_arguments)] // a Bevy system: each argument is a piece of game state it reads
fn panels(
    mut contexts: EguiContexts,
    mut mode: ResMut<ViewMode>,
    map: Res<MapView>,
    mut link: ResMut<SimLink>,
    mut overlays: ResMut<OverlaySettings>,
    selection: Res<Selection>,
    viewshed: Res<ViewshedState>,
    diagnostics: Res<DiagnosticsStore>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    // Panels attach to a full-screen base area (as in bevy_egui's side_panel example).
    let mut screen = egui::Ui::new(
        ctx.clone(),
        "screen".into(),
        egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(ctx.viewport_rect()),
    );
    let fps = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);
    let frame_ms = diagnostics.get(&FrameTimeDiagnosticsPlugin::FRAME_TIME).and_then(|d| d.smoothed()).unwrap_or(0.0);

    egui::Panel::top("top").show(&mut screen, |ui| {
        ui.horizontal(|ui| {
            ui.strong("Col di Lana");
            ui.separator();
            if ui.selectable_label(*mode == ViewMode::Terrain, "3D view").clicked() {
                *mode = ViewMode::Terrain;
            }
            if ui.selectable_label(*mode == ViewMode::Map, "Map").clicked() {
                *mode = ViewMode::Map;
            }
            ui.label("(M)");
            ui.separator();
            ui.label(format!("Game time {}", clock(link.sim.game_seconds())));
            ui.separator();
            let paused = link.paused;
            if ui.selectable_label(paused, "Pause").clicked() {
                link.paused = !paused;
            }
            for s in SPEEDS {
                if ui.selectable_label(!paused && link.speed == s, format!("{s}x")).clicked() {
                    link.speed = s;
                    link.paused = false;
                }
            }
            ui.separator();
            ui.label(format!(
                "Simulation {:.0}% of each frame (worst {:.0}%; limit 50%); update {:.2} ms (worst {:.2})",
                link.frame_share * 100.0,
                link.frame_share_worst * 100.0,
                link.tick_ms,
                link.tick_ms_worst,
            ));
            ui.separator();
            ui.label(format!("{fps:.0} fps, {frame_ms:.1} ms per frame"));
        });
    });

    egui::Panel::left("side").default_size(320.0).resizable(true).show(&mut screen, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Overlays");
            let o = &mut *overlays;
            ui.checkbox(&mut o.contours, "Contour lines (every 20 m, bold every 100 m)");
            ui.checkbox(&mut o.steep, format!("Ground too steep to walk (over {:.0} degrees)", link.sim.tuning.walking.steepest_walkable_deg));
            ui.checkbox(&mut o.viewshed, format!("What the selected man can see (within {:.0} m)", link.sim.tuning.spotting.spotting_range_m));
            if o.viewshed {
                ui.small(if selection.soldier.is_some() {
                    format!("Gold = a standing man there would be in view; blue = hidden. Took {:.0} ms.", viewshed.last_ms)
                } else {
                    "Select a soldier to see what he can see.".to_string()
                });
            }
            ui.label("Spotting lines (who can see whom):");
            ui.horizontal(|ui| {
                ui.radio_value(&mut o.spotting, SpotLines::Off, "off");
                ui.radio_value(&mut o.spotting, SpotLines::SelectedSquad, "selected squad");
                ui.radio_value(&mut o.spotting, SpotLines::Everyone, "everyone");
            });
            ui.checkbox(&mut o.routes, "Squad destinations and the selected squad's route");

            ui.separator();
            ui.heading("Spotting");
            let sim = &link.sim;
            let mut seen = [0usize; 2];
            let mut seen_flags = vec![false; sim.soldiers.len()];
            for s in &sim.soldiers {
                for &e in &s.sees {
                    seen_flags[e as usize] = true;
                }
            }
            for s in &sim.soldiers {
                if seen_flags[s.id as usize] {
                    seen[s.side.index()] += 1;
                }
            }
            ui.label(format!("Austro-Hungarians in Italian view: {}", seen[Side::Austria.index()]));
            ui.label(format!("Italians in Austro-Hungarian view: {}", seen[Side::Italy.index()]));

            ui.separator();
            ui.heading("Selected");
            match selection.squad {
                Some(q) => {
                    let sq = &sim.squads[q as usize];
                    ui.label(format!("{} squad {}, {} men", sq.side.name(), sq.id, sq.members.len()));
                    if let Some(s) = selection.soldier {
                        let sd = &sim.soldiers[s as usize];
                        ui.label(format!(
                            "Soldier {} at ({:.0}, {:.0}), {:.0} m up, on {:.0} degree ground",
                            s,
                            sd.x,
                            sd.y,
                            sim.terrain.height(sd.x, sd.y),
                            sim.terrain.slope_deg(sd.x, sd.y)
                        ));
                    }
                    let state = match sq.state {
                        SquadState::Idle => "waiting".to_string(),
                        SquadState::Planning { .. } => "working out the route".to_string(),
                        SquadState::Moving => {
                            let (x, y) = sq.dest.unwrap_or((0.0, 0.0));
                            let walking = &sim.tuning.walking;
                            let lead = &sim.soldiers[sq.members[0] as usize];
                            let left = sq.field.as_ref().map(|f| f.time_from(&sim.terrain, walking, lead.x, lead.y)).unwrap_or(0.0);
                            format!("moving to ({x:.0}, {y:.0}), about {} to go", clock(left as f64))
                        }
                        SquadState::Arrived => "arrived".to_string(),
                        SquadState::Unreachable => "cannot get there".to_string(),
                    };
                    ui.label(format!("Status: {state}"));
                    let sees: std::collections::BTreeSet<u32> =
                        sq.members.iter().flat_map(|&m| sim.soldiers[m as usize].sees.iter().copied()).collect();
                    ui.label(format!("Enemies in sight of the squad: {}", sees.len()));
                    if sq.side == Side::Austria {
                        ui.small("Right-click the ground to send this squad there.");
                    } else {
                        ui.small("Italian squads follow their own patrols.");
                    }
                }
                None => {
                    ui.label(match *mode {
                        ViewMode::Terrain => "Nothing. Left-click a soldier to select his squad.",
                        ViewMode::Map => "Nothing. Left-click a squad's flag (or, zoomed in, a man) to select the squad.",
                    });
                }
            }

            ui.separator();
            ui.heading("Replay");
            match &link.playback {
                Some(p) => {
                    ui.label(format!("Playing back {}", p.file.display()));
                    ui.label(match p.status {
                        ReplayStatus::Matching => format!("Matches the recording so far (tick {} of {}).", link.sim.tick, p.end_tick),
                        ReplayStatus::Diverged { tick } => format!("DIFFERS from the recording from tick {tick}."),
                        ReplayStatus::Finished => "Finished: matched the recording exactly.".to_string(),
                    });
                }
                None => {
                    ui.label(format!("Scenario {}, seed {}. Everything you do is recorded.", link.scenario, link.sim.seed));
                    if ui.button("Save replay (F5)").clicked() {
                        save(&mut link);
                    }
                }
            }
            if let Some(m) = &link.last_message {
                ui.separator();
                ui.label(m);
            }

            ui.separator();
            egui::CollapsingHeader::new("Controls").default_open(true).show(ui, |ui| {
                match *mode {
                    ViewMode::Terrain => {
                        ui.label("W A S D: move    Q / E: down / up    Shift: faster");
                        ui.label("Mouse wheel: camera speed");
                        ui.label("Hold right button and drag: look around");
                        ui.label("Left-click a soldier: select his squad (Esc clears)");
                    }
                    ViewMode::Map => {
                        ui.label("W A S D or arrows: move the map    Shift: faster");
                        ui.label("Mouse wheel: zoom at the cursor    Q / E: zoom out / in");
                        ui.label("Hold right button and drag: move the map");
                        ui.label("Left-click a squad's flag, or zoomed in a man: select the squad (Esc clears)");
                        ui.label("Zoomed out squads show as flags; zoomed in, each man is a dot");
                    }
                }
                ui.label("Right-click the ground: send the selected squad there");
                ui.label("Space: pause    1 to 7: speed 1x to 64x    F5: save replay");
                ui.label("M: switch between the 3D view and the map");
            });
        });
    });
    if *mode == ViewMode::Map {
        scale_bar(ctx, map.scale);
    }
    Ok(())
}
