//! Screens (egui): the clock and time controls (with the skip-ahead), the dawn orders screen with
//! the work roster, the view switch, overlay switches, the selected squad, replays, help, and the
//! map's scale bar.

use crate::map_view::MapView;
use crate::overlays::{OverlaySettings, SpotLines, ViewshedState};
use crate::places_view::FreeArea;
use crate::sim_link::{OnEvent, ReplayStatus, SPEEDS, SimLink};
use crate::units::Selection;
use crate::view::ViewMode;
use alpine_sim::raid::Instruction;
use alpine_sim::report::morning_report;
use alpine_sim::roster::{self, Duty, PlaceKind};
use alpine_sim::supply::{CarryLeg, Stock};
use alpine_sim::sim::{Order, PLAYER_SIDE, Side, Sim, SquadState};
use alpine_sim::sky;
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

pub fn shortcuts(
    keys: Res<ButtonInput<KeyCode>>,
    egui: Res<EguiWantsInput>,
    mut link: ResMut<SimLink>,
    mut selection: ResMut<Selection>,
) {
    if egui.wants_any_keyboard_input() {
        return;
    }
    if link.dawn_screen {
        // The orders screen waits for "Carry on"; time stays stopped until then.
        if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
            link.carry_on();
        }
        if keys.just_pressed(KeyCode::F5) {
            save(&mut link);
        }
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        link.paused = !link.paused;
    }
    if keys.just_pressed(KeyCode::KeyN) {
        link.toggle_skip();
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
            link.skipping = false;
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

/// On the map, in its top right corner, how hard it is snowing, in words (decision 63: the flakes
/// are drawn in the 3D view only).
fn snow_note(ctx: &egui::Context, sim: &Sim, free: Option<egui::Rect>) {
    let Some(sn) = &sim.snow else { return };
    let word = sn.tuning.falling_word(sn.falling);
    if word.is_empty() {
        return;
    }
    let Some(free) = free else { return };
    egui::Area::new("snow_note".into()).fixed_pos(egui::pos2(free.max.x - 190.0, free.min.y + 12.0)).interactable(false).show(ctx, |ui| {
        ui.label(egui::RichText::new(format!("Snowing: {word}")).size(16.0).italics().color(egui::Color32::from_rgb(40, 32, 24)));
    });
}

/// "Mon 6 Mar 1916 06:42, night, overcast, seeing 15 m" with a calendar; the game time without.
fn clock_text(sim: &Sim) -> String {
    match (sim.now(), &sim.clock) {
        (Some(now), Some(c)) => {
            let sun = c.sky.sun_altitude_deg;
            let light = if sun >= 0.0 {
                "day"
            } else if sun >= sky::DUSK_ALTITUDE_DEG {
                "twilight"
            } else {
                "night"
            };
            let snowing = match &sim.snow {
                Some(sn) if !sn.tuning.falling_word(sn.falling).is_empty() => format!(", snowing ({})", sn.tuning.falling_word(sn.falling)),
                _ => String::new(),
            };
            format!(
                "{} {}, {light}, {}{snowing}, seeing {:.0} m",
                now.short_date(),
                now.hhmm(),
                sky::cloud_words(c.sky.cloud),
                sim.spotting_range
            )
        }
        _ => format!("Game time {}", clock(sim.game_seconds())),
    }
}

/// A squad's men's average tiredness (0 fresh, 1 exhausted).
fn squad_tiredness(sim: &Sim, q: u32) -> f32 {
    let m = &sim.squads[q as usize].members;
    m.iter().map(|&i| sim.soldiers[i as usize].tiredness).sum::<f32>() / m.len().max(1) as f32
}

/// Where a squad on the roster is and what it is doing, in a few words.
fn squad_now(sim: &Sim, q: u32) -> String {
    let sq = &sim.squads[q as usize];
    if sq.detached {
        return if sim.back_to_duty_pending(q as usize) { "returning to duty".into() } else { "sent away by you".into() };
    }
    match sq.post {
        Some((p, _)) => {
            let place = &sim.places[p];
            let there = matches!(sq.state, SquadState::Arrived | SquadState::Idle);
            if let Some(leg) = sq.carry {
                return match leg {
                    CarryLeg::ToDump => format!("going to fetch from the {}", place.name),
                    CarryLeg::Loading { .. } => format!("loading at the {}", place.name),
                    CarryLeg::ToStore { stock } => format!("carrying {} to the {}", stock.word(), place.name),
                    CarryLeg::Unloading { .. } => format!("unloading at the {}", place.name),
                    CarryLeg::Done if sq.inside => format!("asleep, {} (store full)", place.name),
                    CarryLeg::Done => format!("store full, going to the {}", place.name),
                };
            }
            match (place.kind, sq.inside, there) {
                (PlaceKind::Shelter, true, _) => format!("asleep, {}", place.name),
                (PlaceKind::Post, _, true) => format!("on guard, {}", place.name),
                _ => format!("going to the {}", place.name),
            }
        }
        None if sq.members.is_empty() => "all men lost".into(),
        None => "-".into(),
    }
}

/// The work roster as a table (decisions 39 to 43): each of our squads with its day and night duty
/// (click to change), what it is doing now and how tired its men are. Changes are orders, so they
/// are recorded for the replay; given while time stands still they show at once and take effect
/// when it moves on.
fn roster_table(ui: &mut egui::Ui, link: &mut SimLink, selection: &mut Selection, id: &str) {
    let sim = &link.sim;
    let ours: Vec<u32> =
        sim.squads.iter().filter(|q| q.side == PLAYER_SIDE && q.roster.is_some() && !q.members.is_empty()).map(|q| q.id).collect();
    // The carry duty exists only with carrying parties, cold and hunger on (decision 46).
    let duties: &[Duty] = if sim.carrying_on() { &[Duty::Guard, Duty::Rest, Duty::Carry] } else { &[Duty::Guard, Duty::Rest] };
    if ours.is_empty() {
        return;
    }
    let mut orders = Vec::new();
    let mut select = None;
    egui::Grid::new(id).striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
        ui.strong("Squad");
        ui.strong("By day");
        ui.strong("By night");
        ui.strong("Now");
        ui.strong("Men");
        ui.end_row();
        for &q in &ours {
            let Some(r) = sim.roster_shown(q as usize) else { continue };
            let shoes = sim.squads[q as usize].snowshoes;
            let label = if shoes { format!("{q} (snowshoes)") } else { format!("{q}") };
            let hover = if shoes { "Select this squad. It has snowshoes: fresh snow slows it less, so it is the one to break a trail." } else { "Select this squad" };
            if ui.selectable_label(selection.squad == Some(q), label).on_hover_text(hover).clicked() {
                select = Some(q);
            }
            for night in [false, true] {
                let current = r.duty(night);
                ui.horizontal(|ui| {
                    for &duty in duties {
                        if ui.selectable_label(current == duty, duty.word()).clicked() && current != duty {
                            let mut nr = r;
                            if night {
                                nr.night = duty;
                            } else {
                                nr.day = duty;
                            }
                            orders.push(Order::SetRoster { squad: q, day: nr.day, night: nr.night });
                        }
                    }
                });
            }
            ui.label(squad_now(sim, q));
            ui.label(roster::tiredness_word(squad_tiredness(sim, q)));
            ui.end_row();
        }
    });
    for o in orders {
        link.sim.order(o);
    }
    if let Some(q) = select {
        selection.squad = Some(q);
        selection.soldier = link.sim.squads[q as usize].members.first().copied();
    }
}

/// The standing instruction for the night posts (decision 73): one choice for all of them. Given
/// as an order, so it is recorded for the replay.
fn instruction_choice(ui: &mut egui::Ui, link: &mut SimLink) {
    let Some(current) = link.sim.instruction_shown() else { return };
    ui.strong("Night posts, if men come at them");
    ui.small("Only men coming at a post (within about 150 m and getting closer). Anyone else in sight is watched and reported.");
    let mut chosen = None;
    for i in Instruction::ALL {
        if ui.radio(current == i, i.words()).on_hover_text(i.explain()).clicked() && current != i {
            chosen = Some(i);
        }
    }
    ui.small(current.explain());
    if let Some(instruction) = chosen {
        link.sim.order(Order::SetInstruction { instruction });
    }
}

/// The dawn orders screen (decision 32): the morning report, the work roster and "Carry on".
fn dawn_window(ctx: &egui::Context, link: &mut SimLink, selection: &mut Selection) {
    let Some(report) = morning_report(&link.sim) else {
        link.carry_on();
        return;
    };
    let mut carry_on = false;
    egui::Window::new("Orders at dawn")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .default_width(560.0)
        .show(ctx, |ui| {
            ui.heading(&report.title);
            ui.add_space(4.0);
            for line in &report.weather {
                ui.label(line);
            }
            ui.separator();
            ui.strong("Morning report");
            for line in &report.events {
                ui.label(line);
            }
            if !link.sim.places.is_empty() {
                ui.separator();
                ui.strong("Work roster");
                ui.small("Guards watch from the lookout posts; resting men sleep in the shelters, out of sight. Squads change over by themselves at dusk and dawn.");
                egui::ScrollArea::vertical().max_height(330.0).show(ui, |ui| {
                    roster_table(ui, link, selection, "dawn_roster");
                });
            }
            if link.sim.instruction_shown().is_some() {
                ui.separator();
                instruction_choice(ui, link);
                ui.small("Same as yesterday unless you change it.");
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Carry on (Enter)").clicked() {
                    carry_on = true;
                }
                ui.small("Time stays stopped until you carry on.");
            });
        });
    if carry_on {
        link.carry_on();
    }
}

#[allow(clippy::too_many_arguments)] // a Bevy system: each argument is a piece of game state it reads
pub fn panels(
    mut contexts: EguiContexts,
    mut mode: ResMut<ViewMode>,
    map: Res<MapView>,
    mut link: ResMut<SimLink>,
    mut overlays: ResMut<OverlaySettings>,
    mut selection: ResMut<Selection>,
    viewshed: Res<ViewshedState>,
    diagnostics: Res<DiagnosticsStore>,
    mut free: ResMut<FreeArea>,
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

    let top = egui::Panel::top("top").show(&mut screen, |ui| {
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
            ui.label(clock_text(&link.sim));
            ui.separator();
            if let Some(sup) = &link.sim.supply {
                ui.label(format!("Store: food {:.1} days, fuel {:.1} days", sup.days_left(Stock::Food), sup.days_left(Stock::Fuel)))
                    .on_hover_text("What the kitchen cavern holds, in days for the company. Set squads to carry by day or night to fill it from the Sief dump.");
                ui.separator();
            }
            if let Some(sn) = &link.sim.snow
                && sn.untrodden >= 0.5
            {
                ui.label(format!("Fresh snow {:.0} cm", sn.untrodden))
                    .on_hover_text("Fresh snow lying where nobody has walked. It slows men wading through it (less on snowshoes); paths walked by a squad or more are packed and quicker, and routes use them.");
                ui.separator();
            }
            let paused = link.paused;
            let skipping = link.skipping;
            let waiting = link.dawn_screen;
            ui.add_enabled_ui(!waiting, |ui| {
                if ui.selectable_label(paused, "Pause").clicked() {
                    link.paused = !paused;
                }
                for s in SPEEDS {
                    if ui.selectable_label(!paused && !skipping && link.speed == s, format!("{s}x")).clicked() {
                        link.speed = s;
                        link.paused = false;
                        link.skipping = false;
                    }
                }
                if link.sim.clock.is_some()
                    && ui
                        .selectable_label(skipping && !paused, "Skip ahead (N)")
                        .on_hover_text("Run as fast as the computer can until dawn, or until something happens")
                        .clicked()
                {
                    link.toggle_skip();
                }
            });
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

    let side = egui::Panel::left("side").default_size(320.0).resizable(true).show(&mut screen, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            if !link.sim.places.is_empty() {
                egui::CollapsingHeader::new("Work roster").default_open(false).show(ui, |ui| {
                    ui.small("Change any squad's duties at any time; squads change over by themselves at dusk and dawn.");
                    roster_table(ui, &mut link, &mut selection, "side_roster");
                    if link.sim.instruction_shown().is_some() {
                        ui.separator();
                        instruction_choice(ui, &mut link);
                    }
                });
                ui.separator();
            }
            if link.sim.clock.is_some() {
                ui.heading("When something happens");
                ui.small("A new enemy in sight, or a squad you sent arriving or stuck. The skip-ahead always stops.");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut link.on_event, OnEvent::Message, "message only");
                    ui.radio_value(&mut link.on_event, OnEvent::Slow8, "slow to 8x");
                    ui.radio_value(&mut link.on_event, OnEvent::Slow1, "slow to 1x");
                });
                ui.separator();
            }
            ui.heading("Overlays");
            let o = &mut *overlays;
            ui.checkbox(&mut o.contours, "Contour lines (every 20 m, bold every 100 m)");
            ui.checkbox(&mut o.steep, format!("Ground too steep to walk (over {:.0} degrees)", link.sim.tuning.walking.steepest_walkable_deg));
            ui.checkbox(&mut o.viewshed, format!("What the selected man can see (within {:.0} m in this light)", link.sim.spotting_range));
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
            if !link.sim.places.is_empty() {
                ui.checkbox(&mut o.places, "Lookout posts and shelters");
            }
            if link.sim.snow.is_some() {
                ui.checkbox(&mut o.snow, "Fresh snow and the tracks packed in it");
            }
            if link.sim.raid.is_some() {
                ui.checkbox(&mut o.everything, "Show every man, hidden raiders too (for checking only)")
                    .on_hover_text("Raiders are normally shown only once word of the raid reaches the command post, and then only while our men can see them.");
            }

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
                    if let (Some(sn), Some(&lead)) = (&sim.snow, sq.members.first()) {
                        let s = &sim.soldiers[lead as usize];
                        let (c, r) = sim.terrain.nearest_point(s.x, s.y);
                        let cm = sn.loose[sim.terrain.idx(c, r)];
                        let shoes = sq.snowshoes && sn.tuning.switches.snowshoes;
                        let feet = if shoes { "on snowshoes" } else { "on foot" };
                        ui.label(format!(
                            "They go {feet}; fresh snow where they stand: {:.0} cm (they walk at {:.0}% of their speed in it).",
                            cm,
                            sn.tuning.walking_factor(cm, shoes) * 100.0
                        ));
                    }
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
                    let mut back = false;
                    if let (Some(r), Some(&lead)) = (sim.roster_shown(q as usize), sq.members.first()) {
                        ui.label(format!("Duties: {} by day, {} by night. Now: {}.", r.day.word(), r.night.word(), squad_now(sim, q)));
                        let t = squad_tiredness(sim, q);
                        ui.label(format!(
                            "The men are {} ({:.0}% of the way to exhausted); they spot a man at up to {:.0} m now.",
                            roster::tiredness_word(t),
                            t * 100.0,
                            sim.spotting_range_of(lead)
                        ));
                        if let Some(sup) = &sim.supply {
                            let n = sq.members.len().max(1) as f32;
                            let chill = sq.members.iter().map(|&m| sim.soldiers[m as usize].chill).sum::<f32>() / n;
                            let meal = sq.members.iter().map(|&m| sim.soldiers[m as usize].minutes_since_meal).max().unwrap_or(0);
                            ui.label(format!("They are {} and {}.", sup.tuning.cold.word(chill), sup.tuning.hunger.word(meal)));
                        }
                        if sq.detached && !sim.back_to_duty_pending(q as usize) {
                            ui.small("Sent away by you: it takes up its duty again at the next dusk or dawn.");
                            back = ui.button("Back to duty").clicked();
                        }
                    }
                    let sees: std::collections::BTreeSet<u32> =
                        sq.members.iter().flat_map(|&m| sim.soldiers[m as usize].sees.iter().copied()).collect();
                    ui.label(format!("Enemies in sight of the squad: {}", sees.len()));
                    if sq.side == Side::Austria {
                        ui.small(if sq.roster.is_some() {
                            "Right-click the ground to send this squad there; it leaves its duty until the next dusk or dawn."
                        } else {
                            "Right-click the ground to send this squad there."
                        });
                    } else {
                        ui.small("Italian squads follow their own patrols.");
                    }
                    if back {
                        link.sim.order(Order::BackToDuty { squad: q });
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
                if link.sim.clock.is_some() {
                    ui.label("N: skip ahead to dawn or the next event    Enter: carry on at dawn");
                }
                ui.label("M: switch between the 3D view and the map");
            });
        });
    });
    let full = ctx.viewport_rect();
    free.0 = Some(egui::Rect::from_min_max(egui::pos2(side.response.rect.max.x, top.response.rect.max.y), full.max));
    if *mode == ViewMode::Map {
        scale_bar(ctx, map.scale);
        snow_note(ctx, &link.sim, free.0);
    }
    if link.dawn_screen {
        dawn_window(ctx, &mut link, &mut selection);
    }
    Ok(())
}
