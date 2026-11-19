//! The night raid on screen (decisions 71 to 75), in both views: the Italian front trench and our
//! command post with their names, and flares, drawn as a bright light over the ground in the 3D view
//! and as a star with its lit circle on the map. The men themselves (raiders shown only once word
//! has reached the command post, prisoners, the fallen) are drawn with everyone else in `units.rs`
//! and `map_view.rs`.

use crate::map_view::{MapGizmos, MapView};
use crate::overlays::OverlaySettings;
use crate::places_view::FreeArea;
use crate::sim_link::SimLink;
use crate::terrain_view::to_world;
use crate::view::{TerrainCam, ViewMode, map_layer};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};

const FONT_ITALIC: &[u8] = include_bytes!("fonts/OldStandard-Italic.ttf");
const INK: Color = Color::srgb(0.16, 0.12, 0.09);
const INK_ITALIAN: Color = Color::srgb(0.72, 0.1, 0.08);
/// Names show on the map when zoomed in closer than this (metres per pixel).
const NAMES_BELOW_M_PER_PX: f32 = 1.6;
/// In the 3D view, names show within this distance of the camera.
const NAMES_WITHIN_M: f32 = 900.0;

#[derive(Component)]
struct RaidMark;

pub struct RaidViewPlugin;

impl Plugin for RaidViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_map_names)
            .add_systems(Update, (show_map_names, draw_terrain, draw_map))
            .add_systems(EguiPrimaryContextPass, paint_terrain_names.after(crate::ui::panels));
    }
}

/// The trench's points, a little zigzag as trench maps draw it.
fn trench_line(a: (f32, f32), b: (f32, f32)) -> Vec<(f32, f32)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let (nx, ny) = (-dy / len, dx / len);
    let steps = (len / 6.0).ceil() as usize;
    (0..=steps)
        .map(|k| {
            let f = k as f32 / steps as f32;
            let off = if k % 2 == 1 { 2.0 } else { -2.0 };
            (a.0 + dx * f + nx * off, a.1 + dy * f + ny * off)
        })
        .collect()
}

fn spawn_map_names(mut commands: Commands, link: Res<SimLink>, mut fonts: ResMut<Assets<Font>>) {
    let Some(r) = &link.sim.raid else { return };
    let italic = fonts.add(Font::from_bytes(FONT_ITALIC.to_vec()));
    let mid = ((r.trench[0].0 + r.trench[1].0) / 2.0, (r.trench[0].1 + r.trench[1].1) / 2.0);
    for (name, (x, y), colour) in [
        (r.tuning.trench.name.clone(), mid, INK_ITALIAN),
        (r.tuning.command_post.name.clone(), r.command_post, INK),
    ] {
        commands.spawn((
            Text2d::new(name),
            TextFont { font: italic.clone().into(), font_size: FontSize::Px(12.0), ..default() },
            TextColor(colour),
            Anchor::TOP_CENTER,
            Transform::from_xyz(x, y - 4.0, 4.6),
            Visibility::Hidden,
            RaidMark,
            map_layer(),
        ));
    }
}

fn show_map_names(mode: Res<ViewMode>, view: Res<MapView>, settings: Res<OverlaySettings>, mut q: Query<(&mut Transform, &mut Visibility), With<RaidMark>>) {
    let show = *mode == ViewMode::Map && settings.places && view.scale < NAMES_BELOW_M_PER_PX;
    for (mut tf, mut vis) in &mut q {
        vis.set_if_neq(if show { Visibility::Visible } else { Visibility::Hidden });
        if show {
            tf.scale = Vec3::splat(view.scale);
        }
    }
}

/// The trench, the command post and the flares in the 3D view.
fn draw_terrain(mut gizmos: Gizmos, mode: Res<ViewMode>, settings: Res<OverlaySettings>, link: Res<SimLink>) {
    if *mode != ViewMode::Terrain {
        return;
    }
    let sim = &link.sim;
    let Some(r) = &sim.raid else { return };
    let t = &sim.terrain;
    let at = |(x, y): (f32, f32), lift: f32| to_world(x, y, t.height(x, y) + lift);
    if settings.places {
        gizmos.linestrip(trench_line(r.trench[0], r.trench[1]).into_iter().map(|p| at(p, 0.3)), Color::srgb(0.85, 0.3, 0.25));
        let c = r.command_post;
        let colour = Color::srgb(0.95, 0.85, 0.55);
        let foot = at(c, 0.0);
        gizmos.line(foot, foot + Vec3::Y * 8.0, colour);
        gizmos.linestrip([foot + Vec3::Y * 8.0, foot + Vec3::new(3.0, 7.0, 0.0), foot + Vec3::Y * 6.0], colour);
    }
    // A flare: a bright ball hanging over the ground, and the edge of the ground it lights.
    let radius = r.tuning.flares.radius_m;
    for f in &r.flares {
        let ground = at((f.x, f.y), 0.0);
        let ball = ground + Vec3::Y * 60.0;
        let white = Color::srgb(1.0, 1.0, 0.9);
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::TAU / 8.0;
            gizmos.line(ball, ball + Vec3::new(a.cos(), (a * 0.7).sin(), a.sin()) * 2.5, white);
        }
        gizmos.sphere(Isometry3d::from_translation(ball), 1.2, white);
        let ring: Vec<Vec3> = (0..=48)
            .map(|k| k as f32 * std::f32::consts::TAU / 48.0)
            .map(|a| at((f.x + radius * a.cos(), f.y + radius * a.sin()), 0.5))
            .collect();
        gizmos.linestrip(ring, Color::srgba(1.0, 0.95, 0.6, 0.7));
    }
}

/// The trench, the command post and the flares on the map.
fn draw_map(mut gizmos: Gizmos<MapGizmos>, mode: Res<ViewMode>, view: Res<MapView>, settings: Res<OverlaySettings>, link: Res<SimLink>) {
    if *mode != ViewMode::Map {
        return;
    }
    let Some(r) = &link.sim.raid else { return };
    let p = |(x, y): (f32, f32)| Vec2::new(x, y);
    if settings.places {
        gizmos.linestrip_2d(trench_line(r.trench[0], r.trench[1]).into_iter().map(p), INK_ITALIAN);
        // The command post: a small flag.
        let c = p(r.command_post);
        let px = view.scale;
        gizmos.line_2d(c, c + Vec2::Y * 12.0 * px, INK);
        gizmos.linestrip_2d([c + Vec2::Y * 12.0 * px, c + Vec2::new(7.0, 9.5) * px, c + Vec2::Y * 7.0 * px], INK);
    }
    let star = Color::srgb(0.85, 0.55, 0.05);
    for f in &r.flares {
        let c = p((f.x, f.y));
        let s = (8.0 * view.scale).max(3.0);
        for k in 0..4 {
            let a = k as f32 * std::f32::consts::PI / 4.0;
            let d = Vec2::new(a.cos(), a.sin()) * s;
            gizmos.line_2d(c - d, c + d, star);
        }
        gizmos.circle_2d(c, r.tuning.flares.radius_m, star.with_alpha(0.6));
    }
}

/// The trench's and the command post's names painted over the 3D view.
fn paint_terrain_names(
    mut contexts: EguiContexts,
    mode: Res<ViewMode>,
    settings: Res<OverlaySettings>,
    link: Res<SimLink>,
    free: Res<FreeArea>,
    camera: Single<(&Camera, &GlobalTransform), With<TerrainCam>>,
) -> Result {
    let Some(r) = &link.sim.raid else { return Ok(()) };
    if *mode != ViewMode::Terrain || !settings.places {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let (camera, cam_tf) = *camera;
    let t = &link.sim.terrain;
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, "raid_names".into()));
    let painter = match free.0 {
        Some(rect) => painter.with_clip_rect(rect),
        None => painter,
    };
    let font = egui::FontId::proportional(13.0);
    let mid = ((r.trench[0].0 + r.trench[1].0) / 2.0, (r.trench[0].1 + r.trench[1].1) / 2.0);
    for (name, (x, y)) in [(&r.tuning.trench.name, mid), (&r.tuning.command_post.name, r.command_post)] {
        let top = to_world(x, y, t.height(x, y) + 6.0);
        if cam_tf.translation().distance(top) > NAMES_WITHIN_M {
            continue;
        }
        let Ok(at) = camera.world_to_viewport(cam_tf, top) else { continue };
        let pos = egui::pos2(at.x + 4.0, at.y);
        painter.text(pos + egui::vec2(1.0, 1.0), egui::Align2::LEFT_CENTER, name, font.clone(), egui::Color32::from_black_alpha(200));
        painter.text(pos, egui::Align2::LEFT_CENTER, name, font.clone(), egui::Color32::from_rgb(250, 235, 190));
    }
    Ok(())
}
