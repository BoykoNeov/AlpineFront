//! Free camera and mouse orders.
//!
//! WASD move, Q/E down/up, Shift faster, mouse wheel changes speed, hold the right button and drag to
//! look around. Left-click a soldier to select his squad. Right-click the ground (without dragging)
//! to send the selected Austro-Hungarian squad there.

use crate::Options;
use crate::sim_link::SimLink;
use crate::terrain_view::{ground_hit, to_sim, to_world};
use crate::units::{Selection, marker_scale};
use alpine_sim::sim::{Order, Side};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::light::CascadeShadowConfigBuilder;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::input::EguiWantsInput;

#[derive(Component)]
pub struct FlyCam {
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
}

#[derive(Resource, Default)]
struct Clicks {
    left_start: Option<Vec2>,
    right_start: Option<Vec2>,
    right_dragged: f32,
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Clicks>()
            .insert_resource(ClearColor(Color::srgb(0.55, 0.68, 0.85)))
            .insert_resource(GlobalAmbientLight { brightness: 350.0, ..default() })
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, (fly_camera, clicks));
    }
}

fn spawn_camera(mut commands: Commands, link: Res<SimLink>, options: Res<Options>) {
    let t = &link.sim.terrain;
    let (x, y) = (-900.0, -1500.0);
    let (pos, target) = match options.camera {
        Some([x, y, z, tx, ty, tz]) => (to_world(x, y, z), to_world(tx, ty, tz)),
        None => (to_world(x, y, t.height(x, y) + 350.0), to_world(0.0, 0.0, 2400.0)),
    };
    let dir = (target - pos).normalize();
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { far: 30_000.0, near: 0.3, ..default() }),
        Transform::from_translation(pos).looking_to(dir, Vec3::Y),
        FlyCam { yaw: (-dir.x).atan2(-dir.z), pitch: dir.y.asin(), speed: 120.0 },
        DistanceFog {
            color: Color::srgba(0.62, 0.72, 0.85, 1.0),
            falloff: FogFalloff::Linear { start: 6_000.0, end: 22_000.0 },
            ..default()
        },
    ));
    // Afternoon sun from the south-west.
    commands.spawn((
        DirectionalLight { illuminance: 7_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_translation(Vec3::ZERO).looking_to(to_world(0.6, 0.8, -0.6), Vec3::Y),
        CascadeShadowConfigBuilder { first_cascade_far_bound: 400.0, maximum_distance: 8_000.0, ..default() }.build(),
    ));
}

#[allow(clippy::too_many_arguments)] // a Bevy system: each argument is a piece of game state it reads
fn fly_camera(
    time: Res<Time>,
    options: Res<Options>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    egui: Res<EguiWantsInput>,
    clicks: Res<Clicks>,
    link: Res<SimLink>,
    cam: Single<(&mut Transform, &mut FlyCam)>,
) {
    if options.frame_check.is_some() || options.screenshot.is_some() {
        return;
    }
    let (mut tf, mut cam) = cam.into_inner();
    if buttons.pressed(MouseButton::Right) && clicks.right_start.is_some() {
        cam.yaw -= motion.delta.x * 0.0025;
        cam.pitch = (cam.pitch - motion.delta.y * 0.0025).clamp(-1.55, 1.55);
    }
    if !egui.wants_any_pointer_input() && scroll.delta.y != 0.0 {
        let notches = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        cam.speed = (cam.speed * 1.25f32.powf(notches)).clamp(5.0, 3000.0);
    }
    let mut v = Vec3::ZERO;
    if !egui.wants_any_keyboard_input() {
        let forward = Vec3::new(-cam.yaw.sin(), 0.0, -cam.yaw.cos());
        let right = Vec3::new(cam.yaw.cos(), 0.0, -cam.yaw.sin());
        for (key, dir) in [
            (KeyCode::KeyW, forward),
            (KeyCode::KeyS, -forward),
            (KeyCode::KeyD, right),
            (KeyCode::KeyA, -right),
            (KeyCode::KeyE, Vec3::Y),
            (KeyCode::KeyQ, -Vec3::Y),
        ] {
            if keys.pressed(key) {
                v += dir;
            }
        }
    }
    let boost = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { 4.0 } else { 1.0 };
    tf.translation += v.normalize_or_zero() * cam.speed * boost * time.delta_secs();
    let (x, y) = to_sim(tf.translation);
    let t = &link.sim.terrain;
    let floor = t.height(x.clamp(t.min_x(), t.max_x()), y.clamp(t.min_y(), t.max_y())) + 3.0;
    tf.translation.y = tf.translation.y.max(floor);
    tf.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
}

#[allow(clippy::too_many_arguments)]
fn clicks(
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    egui: Res<EguiWantsInput>,
    mut state: ResMut<Clicks>,
    mut link: ResMut<SimLink>,
    mut selection: ResMut<Selection>,
) {
    let cursor = window.cursor_position();
    let (camera, cam_tf) = *camera;
    let over_ui = egui.wants_any_pointer_input();

    if buttons.just_pressed(MouseButton::Left) && !over_ui {
        state.left_start = cursor;
    }
    if buttons.just_released(MouseButton::Left)
        && let (Some(start), Some(c)) = (state.left_start.take(), cursor)
        && start.distance(c) < 5.0
    {
        // Select the soldier drawn nearest the cursor.
        let sim = &link.sim;
        let cam_pos = cam_tf.translation();
        let mut best: Option<(u32, f32)> = None;
        for s in &sim.soldiers {
            let (x, y) = link.drawn_position(s.id as usize);
            let ground = to_world(x, y, sim.terrain.height(x, y));
            let centre = ground + Vec3::Y * 0.8 * marker_scale(cam_pos.distance(ground));
            if let Ok(p) = camera.world_to_viewport(cam_tf, centre) {
                let d = p.distance(c);
                if d < 18.0 && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((s.id, d));
                }
            }
        }
        match best {
            Some((id, _)) => {
                selection.soldier = Some(id);
                selection.squad = Some(sim.soldiers[id as usize].squad);
            }
            None => {
                selection.soldier = None;
                selection.squad = None;
            }
        }
    }

    if buttons.just_pressed(MouseButton::Right) {
        state.right_start = if over_ui { None } else { cursor };
        state.right_dragged = 0.0;
    }
    if buttons.pressed(MouseButton::Right) {
        state.right_dragged += motion.delta.length();
    }
    if buttons.just_released(MouseButton::Right) {
        let start = state.right_start.take();
        if let (Some(_), Some(c), Some(squad)) = (start, cursor, selection.squad)
            && state.right_dragged < 5.0
        {
            let side = link.sim.squads[squad as usize].side;
            if side != Side::Austria {
                link.last_message = Some("Italian squads follow their own patrols; select an Austro-Hungarian squad.".into());
            } else if let Ok(ray) = camera.viewport_to_world(cam_tf, c) {
                match ground_hit(&link.sim.terrain, ray.origin, *ray.direction) {
                    Some((x, y)) => {
                        link.sim.order(Order::Move { squad, x, y });
                        link.last_message = Some(format!(
                            "Squad {squad} ordered to ({x:.0}, {y:.0}), {:.0} m up.",
                            link.sim.terrain.height(x, y)
                        ));
                    }
                    None => link.last_message = Some("That is off the map.".into()),
                }
            }
        }
    }
}
