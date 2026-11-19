//! Falling snow on screen (decisions 57 to 67): the fresh snow and its packed tracks, passed to both
//! views' ground shaders as one shared texture, and flakes around the camera in the 3D view only
//! (decision 63; the map says in words how hard it snows, see `ui::snow_note`).

use crate::sim_link::SimLink;
use crate::terrain_view::TerrainHandles;
use crate::view::{TerrainCam, ViewMode};
use bevy::prelude::*;
use rayon::prelude::*;

pub struct SnowViewPlugin;

impl Plugin for SnowViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SnowTexture>().add_systems(Update, (update_snow_texture, draw_flakes));
    }
}

/// When the snow texture was last filled.
#[derive(Resource, Default)]
struct SnowTexture {
    /// The game minute it shows, and the real time it was filled.
    minute: Option<u64>,
    at: f32,
}

/// Centimetres of fresh snow shown as full white (red = 1).
const FULL_CM: f32 = 80.0;
/// The texture is filled at most this often, in real seconds (it changes once a game minute).
const REFILL_S: f32 = 0.25;

/// Fills the snow texture from the simulation once a game minute, at most every quarter second.
fn update_snow_texture(
    link: Res<SimLink>,
    handles: Option<Res<TerrainHandles>>,
    time: Res<Time>,
    mut state: ResMut<SnowTexture>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(handles), Some(sn)) = (handles, link.sim.snow.as_ref()) else { return };
    let minute = link.sim.tick / link.sim.tuning.seconds_to_ticks(60.0);
    let now = time.elapsed_secs();
    if state.minute == Some(minute) || (state.minute.is_some() && now - state.at < REFILL_S) {
        return;
    }
    let mut data = vec![0u8; sn.loose.len() * 2];
    link.sim.pool().install(|| {
        data.par_chunks_mut(2).enumerate().for_each(|(i, px)| {
            px[0] = ((sn.loose[i] / FULL_CM).clamp(0.0, 1.0) * 255.0) as u8;
            px[1] = (sn.track(i) * 255.0) as u8;
        });
    });
    if let Some(mut img) = images.get_mut(&handles.snow) {
        img.data = Some(data);
    }
    state.minute = Some(minute);
    state.at = now;
}

/// Flakes in the box around the camera, metres.
const BOX: Vec3 = Vec3::new(70.0, 36.0, 70.0);
/// Flakes at the heaviest snowfall, and the rate (cm an hour) that counts as heaviest.
const MOST_FLAKES: f32 = 3000.0;
const HEAVIEST_CM_PER_HOUR: f32 = 4.0;
/// How fast flakes fall, metres per real second (they are drawn in real time, not game time).
const FALL_MPS: f32 = 1.4;

/// A fixed pseudo-random number in [0, 1) for flake `i`, part `k`.
fn hash(i: u32, k: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ k.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// Draws falling flakes as short white strokes in a box that moves with the 3D camera. Each flake
/// is fixed in the world and wraps round the box, so moving the camera does not drag the snow along.
fn draw_flakes(mut gizmos: Gizmos, mode: Res<ViewMode>, link: Res<SimLink>, time: Res<Time>, cam: Query<&GlobalTransform, With<TerrainCam>>) {
    if *mode != ViewMode::Terrain {
        return;
    }
    let Some(sn) = link.sim.snow.as_ref() else { return };
    if sn.falling < 0.05 {
        return;
    }
    let Ok(cam) = cam.single() else { return };
    let centre = cam.translation();
    let n = ((sn.falling / HEAVIEST_CM_PER_HOUR).min(1.0) * MOST_FLAKES).max(150.0) as u32;
    let t = time.elapsed_secs();
    let colour = Color::srgba(0.95, 0.96, 1.0, 0.85);
    for i in 0..n {
        let seed = Vec3::new(hash(i, 1), hash(i, 2), hash(i, 3)) * BOX;
        let sway = Vec3::new((t * 0.7 + hash(i, 4) * 6.3).sin() * 0.8, -FALL_MPS * t * (0.8 + 0.4 * hash(i, 5)), (t * 0.5 + hash(i, 6) * 6.3).cos() * 0.8);
        let p = seed + sway - centre;
        let wrapped = Vec3::new(p.x.rem_euclid(BOX.x), p.y.rem_euclid(BOX.y), p.z.rem_euclid(BOX.z)) - BOX * 0.5 + centre;
        gizmos.line(wrapped, wrapped - Vec3::Y * 0.12, colour);
    }
}
