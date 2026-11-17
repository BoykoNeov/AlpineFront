//! The terrain on screen: the simulation's exact triangles, split into chunks, drawn with a shader
//! that colours the ground and draws the overlays (contours, steep ground, what an observer sees).

use crate::sim_link::SimLink;
use alpine_sim::terrain::Terrain;
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::image::ImageSampler;
use bevy::mesh::Indices;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, PrimitiveTopology, ShaderType, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;

/// Simulation metres (x east, y north, z up) to Bevy's world (x right, y up, z towards viewer).
pub fn to_world(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, z, -y)
}

/// Bevy world position to simulation (x east, y north).
pub fn to_sim(p: Vec3) -> (f32, f32) {
    (p.x, -p.z)
}

pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExt>;

pub const FLAG_CONTOURS: u32 = 1;
pub const FLAG_STEEP: u32 = 2;
pub const FLAG_VIEWSHED: u32 = 4;

#[derive(Clone, Copy, Debug, Default, Reflect, ShaderType)]
pub struct TerrainSettings {
    pub flags: u32,
    pub contour_interval: f32,
    /// Cosine of the steepest walkable slope: triangles whose upward normal is below this are steep.
    pub steep_cos: f32,
    pub cols: f32,
    pub rows: f32,
    pub min_x: f32,
    pub min_y: f32,
    pub spacing: f32,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct TerrainExt {
    #[uniform(100)]
    pub settings: TerrainSettings,
    /// One texel per grid point: 0 = beyond the observer's range, 0.5 = hidden, 1 = in view.
    #[texture(101)]
    #[sampler(102)]
    pub viewshed: Handle<Image>,
}

impl MaterialExtension for TerrainExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://alpine_front/terrain.wesl".into()
    }
}

#[derive(Resource)]
pub struct TerrainHandles {
    pub material: Handle<TerrainMaterial>,
    pub viewshed: Handle<Image>,
}

pub struct TerrainViewPlugin;

impl Plugin for TerrainViewPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "terrain.wesl");
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default())
            .add_systems(Startup, spawn_terrain);
    }
}

const CHUNK: usize = 60;

fn chunk_mesh(t: &Terrain, c0: usize, r0: usize) -> Mesh {
    let c1 = (c0 + CHUNK).min(t.cols - 1);
    let r1 = (r0 + CHUNK).min(t.rows - 1);
    let w = c1 - c0 + 1;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    for r in r0..=r1 {
        for c in c0..=c1 {
            let (x, y) = t.point_xy(c, r);
            positions.push(to_world(x, y, t.h(c, r)).to_array());
            // Smooth shading normal from the neighbouring heights.
            let hx0 = t.h(c.saturating_sub(1), r);
            let hx1 = t.h((c + 1).min(t.cols - 1), r);
            let hy0 = t.h(c, r.saturating_sub(1));
            let hy1 = t.h(c, (r + 1).min(t.rows - 1));
            let gx = (hx1 - hx0) / (2.0 * t.spacing);
            let gy = (hy1 - hy0) / (2.0 * t.spacing);
            normals.push(Vec3::new(-gx, 1.0, gy).normalize().to_array());
        }
    }
    let mut indices = Vec::new();
    for r in 0..(r1 - r0) {
        for c in 0..(c1 - c0) {
            let a = (r * w + c) as u32;
            let b = a + 1;
            let cc = a + w as u32;
            let d = cc + 1;
            // The same split as the simulation: lower triangle (a, b, d), upper (a, d, c).
            indices.extend_from_slice(&[a, b, d, a, d, cc]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(indices))
}

fn spawn_terrain(
    mut commands: Commands,
    link: Res<SimLink>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let t = &link.sim.terrain;
    let mut image = Image::new(
        Extent3d { width: t.cols as u32, height: t.rows as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0u8; t.cols * t.rows],
        TextureFormat::R8Unorm,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    let viewshed = images.add(image);
    let material = materials.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.95,
            reflectance: 0.2,
            ..default()
        },
        extension: TerrainExt {
            settings: TerrainSettings {
                flags: FLAG_CONTOURS,
                contour_interval: 20.0,
                steep_cos: link.sim.tuning.walking.steepest_walkable_deg.to_radians().cos(),
                cols: t.cols as f32,
                rows: t.rows as f32,
                min_x: t.min_x(),
                min_y: t.min_y(),
                spacing: t.spacing,
            },
            viewshed: viewshed.clone(),
        },
    });
    for r0 in (0..t.rows - 1).step_by(CHUNK) {
        for c0 in (0..t.cols - 1).step_by(CHUNK) {
            commands.spawn((
                Mesh3d(meshes.add(chunk_mesh(t, c0, r0))),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
            ));
        }
    }
    commands.insert_resource(TerrainHandles { material, viewshed });
}

/// Where a ray from the camera first meets the ground, in simulation metres.
pub fn ground_hit(t: &Terrain, origin: Vec3, dir: Vec3) -> Option<(f32, f32)> {
    let below = |p: Vec3| {
        let (x, y) = to_sim(p);
        t.in_bounds(x, y) && p.y <= t.height(x, y)
    };
    let step = 4.0;
    let mut prev = origin;
    for i in 1..5000 {
        let p = origin + dir * (i as f32 * step);
        if below(p) {
            let (mut lo, mut hi) = (prev, p);
            for _ in 0..24 {
                let mid = (lo + hi) * 0.5;
                if below(mid) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            return Some(to_sim(hi));
        }
        prev = p;
    }
    None
}
