//! The 2D view: a First World War staff map (design brief, Presentation; decisions 23 to 27).
//!
//! The paper, relief, rock, forest, contour lines, grid and border are painted by `map.wesl` from the
//! simulation's own heights. On top: squads as little flags (like the pins staff officers stuck in
//! wall maps) when zoomed out, each man as a dot up close, lettered place names, and the overlay lines.
//! Map coordinates are simulation metres (x east, y north), so nothing needs converting.
//!
//! W A S D or right-drag move the map, the mouse wheel zooms about the cursor, Q / E zoom out / in.

use crate::Options;
use crate::orders::{Clicks, CursorOverride};
use crate::overlays::{Stroke, StrokeKind, Strokes};
use crate::sim_link::SimLink;
use crate::terrain_view::{FLAG_CONTOURS, TerrainHandles, TerrainSettings, spawn_terrain};
use crate::overlays::OverlaySettings;
use crate::units::{Selection, fallen_shown, seen_by_enemy};
use crate::view::{MapCam, ViewMode, map_layer};
use alpine_sim::sim::Side;
use alpine_sim::terrain::Terrain;
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::camera::ClearColorConfig;
use bevy::image::ImageSampler;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::sprite::Anchor;
use bevy::sprite_render::{Material2d, Material2dPlugin};
use bevy::window::PrimaryWindow;
use bevy_egui::input::EguiWantsInput;

/// Zoomed in closer than this (metres per screen pixel), the map shows each man instead of squad flags.
pub const MEN_BELOW_M_PER_PX: f32 = 1.2;
/// Where a squad's flag cloth sits on screen relative to the foot of its pole (pixels, y down).
pub const FLAG_PICK_OFFSET_PX: Vec2 = Vec2::new(6.0, -14.0);
/// Closest zoom, in metres per screen pixel.
const CLOSEST_M_PER_PX: f32 = 0.08;

/// Places named on the map. Only places the height data itself proves (decisions log, map notes):
/// the summit at the map centre, Monte Sief (610 m north-west along the ridge) and Settsass (the
/// highest point on the map). Positions are simulation metres; heights are read from the terrain.
const PLACES: [(&str, f32, f32); 3] = [("Col di Lana", 0.0, 0.0), ("Monte Sief", -400.0, 460.0), ("Settsass", -140.0, 2480.0)];

const FONT_REGULAR: &[u8] = include_bytes!("fonts/OldStandard-Regular.ttf");
const FONT_ITALIC: &[u8] = include_bytes!("fonts/OldStandard-Italic.ttf");

/// Where the map is looking and how close.
#[derive(Resource, Clone, Copy, Debug)]
pub struct MapView {
    pub centre: Vec2,
    /// Metres per screen pixel.
    pub scale: f32,
}

impl Default for MapView {
    fn default() -> Self {
        MapView { centre: Vec2::ZERO, scale: 7.0 }
    }
}

impl MapView {
    pub fn shows_men(&self) -> bool {
        self.scale < MEN_BELOW_M_PER_PX
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct MapMaterial {
    #[uniform(0)]
    pub settings: TerrainSettings,
    /// The simulation's heights, one texel per grid point, read exactly (no filtering).
    #[texture(1, sample_type = "float", filterable = false)]
    pub heights: Handle<Image>,
    /// Smoothed shading: red = light from the north-west, green = how level the ground is.
    #[texture(2)]
    #[sampler(3)]
    pub relief: Handle<Image>,
    /// Shared with the 3D view: what the selected man can see.
    #[texture(4)]
    #[sampler(5)]
    pub viewshed: Handle<Image>,
    /// Shared with the 3D view: fresh snow and the packed tracks.
    #[texture(6)]
    #[sampler(7)]
    pub snow: Handle<Image>,
}

impl Material2d for MapMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://alpine_front/map.wesl".into()
    }
}

#[derive(Resource)]
pub struct MapHandles {
    pub material: Handle<MapMaterial>,
}

/// Lines drawn on the map (only the map's camera sees them).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct MapGizmos;

/// Kept at a constant size on screen: scaled by metres-per-pixel every frame.
#[derive(Component)]
struct ScreenSized;

#[derive(Component)]
struct ManDot(u32);

#[derive(Component)]
struct SquadFlag(u32);

#[derive(Component)]
struct FlagCloth(u32);

#[derive(Resource)]
struct Inks {
    austria: Handle<ColorMaterial>,
    austria_seen: Handle<ColorMaterial>,
    italy: Handle<ColorMaterial>,
    italy_seen: Handle<ColorMaterial>,
    selected: Handle<ColorMaterial>,
    /// A man taken prisoner; men hit in the raid, lying where they fell.
    captive: Handle<ColorMaterial>,
    fallen: Handle<ColorMaterial>,
}

impl Inks {
    fn pick(&self, side: Side, seen: bool, selected: bool) -> &Handle<ColorMaterial> {
        if selected {
            return &self.selected;
        }
        match (side, seen) {
            (Side::Austria, false) => &self.austria,
            (Side::Austria, true) => &self.austria_seen,
            (Side::Italy, false) => &self.italy,
            (Side::Italy, true) => &self.italy_seen,
        }
    }
}

/// Ink colours on paper: own troops blue, Italians red (decision 27); lighter when an enemy sees them.
pub fn ink(side: Side) -> Color {
    match side {
        Side::Austria => Color::srgb(0.12, 0.25, 0.62),
        Side::Italy => Color::srgb(0.72, 0.1, 0.08),
    }
}
const INK_DARK: Color = Color::srgb(0.16, 0.12, 0.09);
/// The violet of an indelible copying pencil, for the selected squad's route.
const INK_ROUTE: Color = Color::srgb(0.42, 0.16, 0.55);
const INK_RANGE: Color = Color::srgb(0.7, 0.5, 0.08);

pub struct MapViewPlugin;

impl Plugin for MapViewPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "map.wesl");
        let gizmos = GizmoConfig {
            render_layers: map_layer(),
            line: GizmoLineConfig { width: 1.8, ..default() },
            ..default()
        };
        app.add_plugins(Material2dPlugin::<MapMaterial>::default())
            .insert_gizmo_config(MapGizmos, gizmos)
            .init_resource::<MapView>()
            .add_systems(Startup, spawn_map.after(spawn_terrain))
            .add_systems(
                Update,
                (
                    map_controls,
                    (place_camera, place_men, place_flags, keep_screen_size, draw_map_strokes),
                )
                    .chain(),
            );
    }
}

/// Shading from smoothed slopes, one texel per grid point: light from the north-west at 45 degrees,
/// as map makers light their relief, and how level the ground is.
fn relief_image(t: &Terrain) -> Image {
    let mut data = Vec::with_capacity(t.cols * t.rows * 2);
    let light = Vec3::new(-1.0, 1.0, std::f32::consts::SQRT_2).normalize();
    for r in 0..t.rows {
        for c in 0..t.cols {
            let gx = (t.h((c + 1).min(t.cols - 1), r) - t.h(c.saturating_sub(1), r)) / (2.0 * t.spacing);
            let gy = (t.h(c, (r + 1).min(t.rows - 1)) - t.h(c, r.saturating_sub(1))) / (2.0 * t.spacing);
            let n = Vec3::new(-gx, -gy, 1.0).normalize();
            data.push((n.dot(light).clamp(0.0, 1.0) * 255.0).round() as u8);
            data.push((n.z * 255.0).round() as u8);
        }
    }
    let mut image = Image::new(
        Extent3d { width: t.cols as u32, height: t.rows as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rg8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

fn heights_image(t: &Terrain) -> Image {
    let data: Vec<u8> = t.heights.iter().flat_map(|h| h.to_le_bytes()).collect();
    let mut image = Image::new(
        Extent3d { width: t.cols as u32, height: t.rows as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::R32Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

#[allow(clippy::too_many_arguments)] // a Bevy startup system: each argument is a kind of asset it creates
fn spawn_map(
    mut commands: Commands,
    link: Res<SimLink>,
    options: Res<Options>,
    window: Single<&Window, With<PrimaryWindow>>,
    terrain: Res<TerrainHandles>,
    mut view: ResMut<MapView>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<MapMaterial>>,
    mut colours: ResMut<Assets<ColorMaterial>>,
    mut fonts: ResMut<Assets<Font>>,
) {
    let sim = &link.sim;
    let t = &sim.terrain;
    let (w, h) = (t.max_x() - t.min_x(), t.max_y() - t.min_y());

    // Start with the whole map in view, or where the command line asks.
    *view = match options.map {
        Some([x, y, width]) => MapView { centre: Vec2::new(x, y), scale: width / window.width() },
        None => MapView { centre: Vec2::ZERO, scale: h * 1.04 / window.height() },
    };

    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            is_active: false,
            // The table the map lies on.
            clear_color: ClearColorConfig::Custom(Color::srgb(0.2, 0.17, 0.14)),
            ..default()
        },
        MapCam,
        map_layer(),
        Transform::from_xyz(view.centre.x, view.centre.y, 0.0),
    ));

    let material = materials.add(MapMaterial {
        settings: TerrainSettings {
            flags: FLAG_CONTOURS,
            contour_interval: 20.0,
            steep_cos: sim.tuning.walking.steepest_walkable_deg.to_radians().cos(),
            cols: t.cols as f32,
            rows: t.rows as f32,
            min_x: t.min_x(),
            min_y: t.min_y(),
            spacing: t.spacing,
        },
        heights: images.add(heights_image(t)),
        relief: images.add(relief_image(t)),
        viewshed: terrain.viewshed.clone(),
        snow: terrain.snow.clone(),
    });
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(w, h))),
        MeshMaterial2d(material.clone()),
        Transform::from_xyz(t.min_x() + w / 2.0, t.min_y() + h / 2.0, 0.0),
        map_layer(),
    ));
    commands.insert_resource(MapHandles { material });

    let lighter = |c: Color| Color::from(c.to_srgba().mix(&Srgba::WHITE, 0.45));
    let inks = Inks {
        austria: colours.add(ink(Side::Austria)),
        austria_seen: colours.add(lighter(ink(Side::Austria))),
        italy: colours.add(ink(Side::Italy)),
        italy_seen: colours.add(lighter(ink(Side::Italy))),
        selected: colours.add(Color::srgb(0.9, 0.62, 0.0)),
        captive: colours.add(Color::srgb(0.98, 0.97, 0.92)),
        fallen: colours.add(Color::srgb(0.35, 0.3, 0.26)),
    };
    let dark = colours.add(INK_DARK);

    // Men: a dot each, sized in `place_men`.
    let dot = meshes.add(Circle::new(1.0));
    for s in &sim.soldiers {
        commands.spawn((
            Mesh2d(dot.clone()),
            MeshMaterial2d(inks.pick(s.side, false, false).clone()),
            Transform::from_xyz(s.x, s.y, 2.0),
            Visibility::Hidden,
            ManDot(s.id),
            map_layer(),
        ));
    }

    let regular = fonts.add(Font::from_bytes(FONT_REGULAR.to_vec()));
    let italic = fonts.add(Font::from_bytes(FONT_ITALIC.to_vec()));

    // Squads: a little flag on a pole, the squad's number beside it. Children are sized in pixels.
    let pole = meshes.add(Rectangle::new(1.6, 18.0));
    let cloth = meshes.add(Rectangle::new(11.0, 7.5));
    let foot = meshes.add(Circle::new(1.8));
    for q in &sim.squads {
        commands
            .spawn((Transform::from_xyz(0.0, 0.0, 3.0), Visibility::Hidden, SquadFlag(q.id), ScreenSized, map_layer()))
            .with_children(|p| {
                p.spawn((Mesh2d(pole.clone()), MeshMaterial2d(dark.clone()), Transform::from_xyz(0.0, 9.0, 0.0), map_layer()));
                p.spawn((Mesh2d(foot.clone()), MeshMaterial2d(dark.clone()), Transform::from_xyz(0.0, 0.0, 0.1), map_layer()));
                p.spawn((
                    Mesh2d(cloth.clone()),
                    MeshMaterial2d(inks.pick(q.side, false, false).clone()),
                    Transform::from_xyz(6.3, 14.2, 0.1),
                    FlagCloth(q.id),
                    map_layer(),
                ));
                p.spawn((
                    Text2d::new(q.id.to_string()),
                    TextFont { font: regular.clone().into(), font_size: FontSize::Px(12.0), ..default() },
                    TextColor(INK_DARK),
                    Anchor::CENTER_LEFT,
                    Transform::from_xyz(13.5, 14.2, 0.2),
                    map_layer(),
                ));
            });
    }
    commands.insert_resource(inks);

    // Place names: a peak mark, the name in italic and the height, as on the maps of the time.
    let peak = meshes.add(Triangle2d::new(Vec2::new(0.0, 4.5), Vec2::new(-4.0, -2.5), Vec2::new(4.0, -2.5)));
    for (name, x, y) in PLACES {
        let height = t.height(x, y);
        commands
            .spawn((Transform::from_xyz(x, y, 4.0), Visibility::Visible, ScreenSized, map_layer()))
            .with_children(|p| {
                p.spawn((Mesh2d(peak.clone()), MeshMaterial2d(dark.clone()), Transform::default(), map_layer()));
                p.spawn((
                    Text2d::new(name),
                    TextFont { font: italic.clone().into(), font_size: FontSize::Px(17.0), ..default() },
                    TextColor(INK_DARK),
                    Anchor::CENTER_LEFT,
                    Transform::from_xyz(8.0, 4.0, 0.1),
                    map_layer(),
                ));
                p.spawn((
                    Text2d::new(format!("{height:.0}")),
                    TextFont { font: regular.clone().into(), font_size: FontSize::Px(12.0), ..default() },
                    TextColor(INK_DARK),
                    Anchor::CENTER_LEFT,
                    Transform::from_xyz(8.0, -10.0, 0.1),
                    map_layer(),
                ));
            });
    }
}

/// The furthest zoom: the whole map with a margin.
fn furthest_scale(t: &Terrain, window: &Window) -> f32 {
    let (w, h) = (t.max_x() - t.min_x(), t.max_y() - t.min_y());
    (w / window.width()).max(h / window.height()) * 1.3
}

#[allow(clippy::too_many_arguments)] // a Bevy system: each argument is a piece of game state it reads
pub fn map_controls(
    time: Res<Time>,
    options: Res<Options>,
    mode: Res<ViewMode>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    egui: Res<EguiWantsInput>,
    clicks: Res<Clicks>,
    cursor_override: Res<CursorOverride>,
    link: Res<SimLink>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut view: ResMut<MapView>,
) {
    if *mode != ViewMode::Map || options.frame_check.is_some() || options.screenshot.is_some() {
        return;
    }
    let t = &link.sim.terrain;
    let dt = time.delta_secs();
    let mut v = *view;

    // Right-drag moves the map with the cursor.
    if buttons.pressed(MouseButton::Right) && clicks.right_start.is_some() {
        v.centre += Vec2::new(-motion.delta.x, motion.delta.y) * v.scale;
    }
    if !egui.wants_any_keyboard_input() {
        let mut dir = Vec2::ZERO;
        for (key, d) in [
            (KeyCode::KeyW, Vec2::Y),
            (KeyCode::KeyS, -Vec2::Y),
            (KeyCode::KeyD, Vec2::X),
            (KeyCode::KeyA, -Vec2::X),
            (KeyCode::ArrowUp, Vec2::Y),
            (KeyCode::ArrowDown, -Vec2::Y),
            (KeyCode::ArrowRight, Vec2::X),
            (KeyCode::ArrowLeft, -Vec2::X),
        ] {
            if keys.pressed(key) {
                dir += d;
            }
        }
        let boost = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { 3.0 } else { 1.0 };
        v.centre += dir.normalize_or_zero() * 600.0 * v.scale * boost * dt;
        if keys.pressed(KeyCode::KeyQ) {
            v.scale *= (1.6 * dt).exp();
        }
        if keys.pressed(KeyCode::KeyE) {
            v.scale *= (-1.6 * dt).exp();
        }
    }
    // The mouse wheel zooms about the point under the cursor.
    if !egui.wants_any_pointer_input() && scroll.delta.y != 0.0 {
        let notches = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        let new_scale = (v.scale * 0.8f32.powf(notches)).clamp(CLOSEST_M_PER_PX, furthest_scale(t, &window));
        if let Some(c) = cursor_override.0.or(window.cursor_position()) {
            let from_centre = Vec2::new(c.x - window.width() / 2.0, window.height() / 2.0 - c.y);
            let under = v.centre + from_centre * v.scale;
            v.centre = under - from_centre * new_scale;
        }
        v.scale = new_scale;
    }
    v.scale = v.scale.clamp(CLOSEST_M_PER_PX, furthest_scale(t, &window));
    v.centre = v.centre.clamp(Vec2::new(t.min_x(), t.min_y()), Vec2::new(t.max_x(), t.max_y()));
    if v.centre != view.centre || v.scale != view.scale {
        *view = v;
    }
}

fn place_camera(view: Res<MapView>, camera: Single<(&mut Transform, &mut Projection), With<MapCam>>) {
    let (mut tf, mut projection) = camera.into_inner();
    tf.translation = view.centre.extend(0.0);
    if let Projection::Orthographic(o) = &mut *projection
        && o.scale != view.scale
    {
        o.scale = view.scale;
    }
}

/// A squad's flag stands at the middle of its men.
pub fn squad_anchor(link: &SimLink, squad: u32) -> Option<(f32, f32)> {
    squad_anchor_of(link, squad, None)
}

/// The middle of the squad's men that are drawn (`drawn`, from `Sim::drawn_men`; all when None).
/// None when none of them is.
pub fn squad_anchor_of(link: &SimLink, squad: u32, drawn: Option<&[bool]>) -> Option<(f32, f32)> {
    let q = link.sim.squads.get(squad as usize)?;
    let (mut x, mut y, mut n) = (0.0, 0.0, 0.0);
    for &m in q.members.iter().filter(|&&m| drawn.is_none_or(|d| d[m as usize])) {
        let (mx, my) = link.drawn_position(m as usize);
        x += mx;
        y += my;
        n += 1.0;
    }
    (n > 0.0).then(|| (x / n, y / n))
}

fn place_men(
    mode: Res<ViewMode>,
    view: Res<MapView>,
    link: Res<SimLink>,
    selection: Res<Selection>,
    inks: Res<Inks>,
    settings: Res<OverlaySettings>,
    mut q: Query<(&ManDot, &mut Transform, &mut Visibility, &mut MeshMaterial2d<ColorMaterial>)>,
) {
    let show = *mode == ViewMode::Map && view.shows_men();
    if !show {
        for (_, _, mut vis, _) in &mut q {
            vis.set_if_neq(Visibility::Hidden);
        }
        return;
    }
    let sim = &link.sim;
    let seen = seen_by_enemy(&link);
    let drawn = sim.drawn_men(settings.everything);
    let fallen = fallen_shown(&link, settings.everything);
    // At least 2.5 pixels across, or a man's true width when zoomed right in.
    let radius = (2.5 * view.scale).max(0.35);
    for (m, mut tf, mut vis, mut mat) in &mut q {
        let i = m.0 as usize;
        let s = &sim.soldiers[i];
        if !drawn[i] && !fallen[i] {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        let (x, y) = link.drawn_position(i);
        tf.translation = Vec3::new(x, y, 2.0);
        tf.scale = Vec3::splat(radius);
        vis.set_if_neq(Visibility::Visible);
        let want = if fallen[i] {
            &inks.fallen
        } else if sim.raid.as_ref().is_some_and(|r| r.captive(i as u32)) {
            &inks.captive
        } else {
            inks.pick(s.side, seen[i], selection.squad == Some(s.squad))
        };
        if mat.0 != *want {
            mat.0 = want.clone();
        }
    }
}

#[allow(clippy::too_many_arguments)] // a Bevy system: each argument is a piece of game state it reads
fn place_flags(
    mode: Res<ViewMode>,
    view: Res<MapView>,
    link: Res<SimLink>,
    selection: Res<Selection>,
    inks: Res<Inks>,
    settings: Res<OverlaySettings>,
    mut flags: Query<(&SquadFlag, &mut Transform, &mut Visibility)>,
    mut cloths: Query<(&FlagCloth, &mut MeshMaterial2d<ColorMaterial>)>,
) {
    let show = *mode == ViewMode::Map && !view.shows_men();
    let drawn = if show { link.sim.drawn_men(settings.everything) } else { Vec::new() };
    for (f, mut tf, mut vis) in &mut flags {
        // A squad inside a shelter shows only in the shelter's name ("3 squads inside").
        if !show || link.sim.squads[f.0 as usize].inside {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        match squad_anchor_of(&link, f.0, Some(&drawn)) {
            Some((x, y)) => {
                tf.translation.x = x;
                tf.translation.y = y;
                vis.set_if_neq(Visibility::Visible);
            }
            // A squad whose men are all lost has no flag.
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
    if !show {
        return;
    }
    let sim = &link.sim;
    let seen = seen_by_enemy(&link);
    for (c, mut mat) in &mut cloths {
        let q = &sim.squads[c.0 as usize];
        let any_seen = q.members.iter().any(|&m| seen[m as usize]);
        let want = inks.pick(q.side, any_seen, selection.squad == Some(q.id));
        if mat.0 != *want {
            mat.0 = want.clone();
        }
    }
}

fn keep_screen_size(view: Res<MapView>, mut q: Query<&mut Transform, With<ScreenSized>>) {
    for mut tf in &mut q {
        tf.scale = Vec3::splat(view.scale);
    }
}

/// Draws the overlay lines on the map in pencil colours.
fn draw_map_strokes(mut gizmos: Gizmos<MapGizmos>, mode: Res<ViewMode>, view: Res<MapView>, strokes: Res<Strokes>) {
    if *mode != ViewMode::Map {
        return;
    }
    let p = |(x, y): (f32, f32)| Vec2::new(x, y);
    let px = view.scale;
    for Stroke { kind, points } in &strokes.0 {
        match *kind {
            StrokeKind::Spotting(side) => gizmos.line_2d(p(points[0]), p(points[1]), ink(side).with_alpha(0.55)),
            StrokeKind::Destination(side) => {
                // A ringed cross, 7 pixels across.
                let c = p(points[0]);
                let r = 7.0 * px;
                gizmos.line_2d(c - Vec2::splat(r * 0.7), c + Vec2::splat(r * 0.7), ink(side));
                gizmos.line_2d(c + Vec2::new(-r * 0.7, r * 0.7), c + Vec2::new(r * 0.7, -r * 0.7), ink(side));
                gizmos.circle_2d(c, r, ink(side));
            }
            StrokeKind::Route => gizmos.linestrip_2d(points.iter().map(|&q| p(q)), INK_ROUTE),
            StrokeKind::Range => gizmos.linestrip_2d(points.iter().map(|&q| p(q)), INK_RANGE),
        }
    }
}
