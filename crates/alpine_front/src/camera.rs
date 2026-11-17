//! The 3D view's free camera.
//!
//! WASD move, Q/E down/up, Shift faster, mouse wheel changes speed, hold the right button and drag to
//! look around. Clicks and orders are in `orders.rs`, shared with the map.

use crate::Options;
use crate::orders::Clicks;
use crate::sim_link::SimLink;
use crate::terrain_view::{to_sim, to_world};
use crate::view::{TerrainCam, ViewMode};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::light::CascadeShadowConfigBuilder;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;

#[derive(Component)]
pub struct FlyCam {
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.55, 0.68, 0.85)))
            .insert_resource(GlobalAmbientLight { brightness: 350.0, ..default() })
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, fly_camera);
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
        TerrainCam,
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
    mode: Res<ViewMode>,
    cam: Single<(&mut Transform, &mut FlyCam)>,
) {
    if options.frame_check.is_some() || options.screenshot.is_some() || *mode != ViewMode::Terrain {
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
