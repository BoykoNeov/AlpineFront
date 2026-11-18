//! The work roster's lookout posts and shelters on the ground, in both views (decisions 39 to 43):
//! a mark and the place's name, with how many squads are inside each shelter (their men are not
//! drawn while inside). The 3D view draws marks on the ground and names painted over the picture;
//! the map draws them in ink like its other names.

use crate::overlays::OverlaySettings;
use crate::sim_link::SimLink;
use crate::terrain_view::to_world;
use crate::view::{TerrainCam, ViewMode, map_layer};
use crate::map_view::MapView;
use alpine_sim::roster::PlaceKind;
use alpine_sim::sim::Sim;
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};

const FONT_ITALIC: &[u8] = include_bytes!("fonts/OldStandard-Italic.ttf");
const INK: Color = Color::srgb(0.16, 0.12, 0.09);
/// Names show on the map when zoomed in closer than this (metres per pixel); further out they
/// would sit on top of each other.
const NAMES_BELOW_M_PER_PX: f32 = 1.6;
/// In the 3D view, names show within this distance of the camera.
const NAMES_WITHIN_M: f32 = 900.0;

/// The part of the window the side and top panels leave free, for painting names over the 3D view.
#[derive(Resource, Default)]
pub struct FreeArea(pub Option<egui::Rect>);

#[derive(Component)]
struct PlaceMark;

#[derive(Component)]
struct PlaceLabel(usize);

pub struct PlacesViewPlugin;

impl Plugin for PlacesViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FreeArea>()
            .add_systems(Startup, spawn_map_marks)
            .add_systems(Update, (show_map_marks, draw_terrain_marks))
            .add_systems(EguiPrimaryContextPass, paint_terrain_names.after(crate::ui::panels));
    }
}

/// "Great cavern (3 squads inside)" for a shelter with squads in it; the bare name otherwise.
pub fn place_label(sim: &Sim, p: usize) -> String {
    let place = &sim.places[p];
    if place.kind != PlaceKind::Shelter {
        return place.name.clone();
    }
    let inside = sim.squads.iter().filter(|q| q.inside && q.post.map(|x| x.0) == Some(p)).count();
    match inside {
        0 => place.name.clone(),
        1 => format!("{} (1 squad inside)", place.name),
        n => format!("{} ({n} squads inside)", place.name),
    }
}

fn spawn_map_marks(
    mut commands: Commands,
    link: Res<SimLink>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut colours: ResMut<Assets<ColorMaterial>>,
    mut fonts: ResMut<Assets<Font>>,
) {
    let sim = &link.sim;
    if sim.places.is_empty() {
        return;
    }
    let italic = fonts.add(Font::from_bytes(FONT_ITALIC.to_vec()));
    let ink = colours.add(INK);
    // A post: a small ring of a dot; a shelter: a small block, as on trench maps.
    let post = meshes.add(Circle::new(3.0));
    let shelter = meshes.add(Rectangle::new(8.0, 5.0));
    // The dump and the kitchen store: a square.
    let store = meshes.add(Rectangle::new(7.0, 7.0));
    for (i, p) in sim.places.iter().enumerate() {
        let mesh = match p.kind {
            PlaceKind::Post => post.clone(),
            PlaceKind::Shelter => shelter.clone(),
            PlaceKind::Dump | PlaceKind::Store => store.clone(),
        };
        commands
            .spawn((Transform::from_xyz(p.x, p.y, 4.5), Visibility::Hidden, PlaceMark, map_layer()))
            .with_children(|c| {
                c.spawn((Mesh2d(mesh), MeshMaterial2d(ink.clone()), Transform::default(), map_layer()));
                c.spawn((
                    Text2d::new(p.name.clone()),
                    TextFont { font: italic.clone().into(), font_size: FontSize::Px(12.0), ..default() },
                    TextColor(INK),
                    Anchor::BOTTOM_LEFT,
                    // Above and to the right, clear of the dots of the men standing there.
                    Transform::from_xyz(10.0, 10.0, 0.1),
                    PlaceLabel(i),
                    map_layer(),
                ));
            });
    }
}

/// Shows the map's marks when the overlay is on and the map is zoomed in, keeps them a constant
/// size on screen, and keeps the shelters' counts up to date.
fn show_map_marks(
    mode: Res<ViewMode>,
    view: Res<MapView>,
    settings: Res<OverlaySettings>,
    link: Res<SimLink>,
    mut marks: Query<(&mut Transform, &mut Visibility), With<PlaceMark>>,
    mut labels: Query<(&PlaceLabel, &mut Text2d)>,
) {
    let show = *mode == ViewMode::Map && settings.places && view.scale < NAMES_BELOW_M_PER_PX;
    for (mut tf, mut vis) in &mut marks {
        vis.set_if_neq(if show { Visibility::Visible } else { Visibility::Hidden });
        if show {
            tf.scale = Vec3::splat(view.scale);
        }
    }
    if show {
        for (l, mut text) in &mut labels {
            let want = place_label(&link.sim, l.0);
            if text.0 != want {
                text.0 = want;
            }
        }
    }
}

/// Marks on the ground in the 3D view: a ring for a post, a square for a shelter.
fn draw_terrain_marks(mut gizmos: Gizmos, mode: Res<ViewMode>, settings: Res<OverlaySettings>, link: Res<SimLink>) {
    if *mode != ViewMode::Terrain || !settings.places {
        return;
    }
    let sim = &link.sim;
    let t = &sim.terrain;
    let colour = Color::srgb(0.95, 0.85, 0.55);
    for p in &sim.places {
        let ground = |dx: f32, dy: f32| to_world(p.x + dx, p.y + dy, t.height(p.x + dx, p.y + dy) + 0.4);
        let r = if p.kind == PlaceKind::Post { 4.0 } else { 5.0 };
        let corners: Vec<Vec3> = match p.kind {
            PlaceKind::Post => (0..=16).map(|k| k as f32 * std::f32::consts::TAU / 16.0).map(|a| ground(r * a.cos(), r * a.sin())).collect(),
            PlaceKind::Shelter => [(-r, -r), (r, -r), (r, r), (-r, r), (-r, -r)].iter().map(|&(dx, dy)| ground(dx, dy)).collect(),
            // The dump and the store: a diamond.
            PlaceKind::Dump | PlaceKind::Store => {
                [(0.0, -r), (r, 0.0), (0.0, r), (-r, 0.0), (0.0, -r)].iter().map(|&(dx, dy)| ground(dx, dy)).collect()
            }
        };
        gizmos.linestrip(corners, colour);
        let foot = ground(0.0, 0.0);
        gizmos.line(foot, foot + Vec3::Y * 6.0, colour);
    }
}

/// The places' names painted over the 3D view, beside their marks.
fn paint_terrain_names(
    mut contexts: EguiContexts,
    mode: Res<ViewMode>,
    settings: Res<OverlaySettings>,
    link: Res<SimLink>,
    free: Res<FreeArea>,
    camera: Single<(&Camera, &GlobalTransform), With<TerrainCam>>,
) -> Result {
    if *mode != ViewMode::Terrain || !settings.places || link.sim.places.is_empty() {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let (camera, cam_tf) = *camera;
    let sim = &link.sim;
    let t = &sim.terrain;
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, "place_names".into()));
    let painter = match free.0 {
        Some(r) => painter.with_clip_rect(r),
        None => painter,
    };
    let font = egui::FontId::proportional(13.0);
    for (i, p) in sim.places.iter().enumerate() {
        let top = to_world(p.x, p.y, t.height(p.x, p.y) + 6.0);
        if cam_tf.translation().distance(top) > NAMES_WITHIN_M {
            continue;
        }
        let Ok(at) = camera.world_to_viewport(cam_tf, top) else { continue };
        let pos = egui::pos2(at.x + 4.0, at.y);
        let text = place_label(sim, i);
        // Dark text on a light shadow, readable on snow and rock alike.
        painter.text(pos + egui::vec2(1.0, 1.0), egui::Align2::LEFT_CENTER, &text, font.clone(), egui::Color32::from_black_alpha(200));
        painter.text(pos, egui::Align2::LEFT_CENTER, &text, font.clone(), egui::Color32::from_rgb(250, 235, 190));
    }
    Ok(())
}
