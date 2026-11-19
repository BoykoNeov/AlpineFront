//! Soldiers in the 3D view: one simple shape each, coloured by side, brighter when an enemy can see
//! him, yellow when his squad is selected. Shapes grow with distance so they stay visible from afar.

use crate::overlays::OverlaySettings;
use crate::sim_link::SimLink;
use crate::terrain_view::to_world;
use crate::view::{TerrainCam, ViewMode};
use alpine_sim::sim::Side;
use bevy::prelude::*;

#[derive(Component)]
pub struct SoldierMarker(pub u32);

#[derive(Resource, Default)]
pub struct Selection {
    pub soldier: Option<u32>,
    pub squad: Option<u32>,
}

#[derive(Resource)]
struct UnitMaterials {
    austria: Handle<StandardMaterial>,
    austria_seen: Handle<StandardMaterial>,
    italy: Handle<StandardMaterial>,
    italy_seen: Handle<StandardMaterial>,
    selected: Handle<StandardMaterial>,
    /// A man taken prisoner, being walked away.
    captive: Handle<StandardMaterial>,
    /// Men hit in the raid, lying where they fell until dawn.
    fallen_austria: Handle<StandardMaterial>,
    fallen_italy: Handle<StandardMaterial>,
}

/// A man is 1.6 m tall; seen from further than this he is drawn larger so he stays visible.
const TRUE_SIZE_WITHIN_M: f32 = 150.0;

/// Drawing scale for a soldier this far from the camera.
pub fn marker_scale(distance: f32) -> f32 {
    (distance / TRUE_SIZE_WITHIN_M).max(1.0)
}

/// For each soldier, whether an enemy can see him, as far as the player may know (a raider seeing
/// one of our men does not show; decision 75).
pub fn seen_by_enemy(link: &SimLink) -> Vec<bool> {
    link.sim.seen_by_enemy_shown()
}

/// For each soldier, whether a man hit in the raid lies there now, drawn (once word has reached the
/// command post, or always with the checking overlay).
pub fn fallen_shown(link: &SimLink, everything: bool) -> Vec<bool> {
    let sim = &link.sim;
    let mut out = vec![false; sim.soldiers.len()];
    if let Some(r) = &sim.raid
        && (everything || r.known() || !r.tuning.switches.word_must_reach_command_post)
    {
        for f in &r.fallen {
            out[f.man as usize] = true;
        }
    }
    out
}

pub struct UnitsPlugin;

impl Plugin for UnitsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>()
            .add_systems(Startup, spawn_units)
            .add_systems(PostUpdate, place_units.before(TransformSystems::Propagate));
    }
}

fn flat(materials: &mut Assets<StandardMaterial>, c: Color) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial { base_color: c, unlit: true, ..default() })
}

fn spawn_units(
    mut commands: Commands,
    link: Res<SimLink>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mats = UnitMaterials {
        austria: flat(&mut materials, Color::srgb(0.25, 0.42, 0.85)),
        austria_seen: flat(&mut materials, Color::srgb(0.6, 0.85, 1.0)),
        italy: flat(&mut materials, Color::srgb(0.8, 0.12, 0.1)),
        italy_seen: flat(&mut materials, Color::srgb(1.0, 0.55, 0.5)),
        selected: flat(&mut materials, Color::srgb(1.0, 1.0, 0.2)),
        captive: flat(&mut materials, Color::srgb(0.95, 0.95, 0.95)),
        fallen_austria: flat(&mut materials, Color::srgb(0.12, 0.2, 0.4)),
        fallen_italy: flat(&mut materials, Color::srgb(0.4, 0.08, 0.06)),
    };
    // A capsule 1.6 m tall, centred 0.8 m above the ground.
    let mesh = meshes.add(Capsule3d::new(0.3, 1.0));
    for s in &link.sim.soldiers {
        let m = if s.side == Side::Austria { mats.austria.clone() } else { mats.italy.clone() };
        commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(m), Transform::default(), SoldierMarker(s.id)));
    }
    commands.insert_resource(mats);
}

fn place_units(
    link: Res<SimLink>,
    selection: Res<Selection>,
    mats: Res<UnitMaterials>,
    mode: Res<ViewMode>,
    settings: Res<OverlaySettings>,
    camera: Single<&Transform, (With<TerrainCam>, Without<SoldierMarker>)>,
    mut q: Query<(&SoldierMarker, &mut Transform, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    if *mode != ViewMode::Terrain {
        return;
    }
    let sim = &link.sim;
    let seen = seen_by_enemy(&link);
    // Men inside a shelter are out of sight (decision 39); raiders show only as the player may know
    // of them (decision 75).
    let drawn = sim.drawn_men(settings.everything);
    let fallen = fallen_shown(&link, settings.everything);
    let captive = |i: usize| sim.raid.as_ref().is_some_and(|r| r.captive(i as u32));
    let cam = camera.translation;
    for (m, mut tf, mut vis, mut mat) in &mut q {
        let i = m.0 as usize;
        let s = &sim.soldiers[i];
        let show = drawn[i] || fallen[i];
        vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
        if !show {
            continue;
        }
        let (x, y) = link.drawn_position(i);
        let ground = to_world(x, y, sim.terrain.height(x, y));
        let scale = marker_scale(cam.distance(ground));
        tf.scale = Vec3::splat(scale);
        if fallen[i] {
            // Lying on the ground.
            tf.translation = ground + Vec3::Y * 0.3 * scale;
            tf.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
            let want = if s.side == Side::Austria { &mats.fallen_austria } else { &mats.fallen_italy };
            if mat.0 != *want {
                mat.0 = want.clone();
            }
            continue;
        }
        tf.rotation = Quat::IDENTITY;
        tf.translation = ground + Vec3::Y * 0.8 * scale;
        let want = if captive(i) {
            &mats.captive
        } else if selection.squad == Some(s.squad) {
            &mats.selected
        } else {
            match (s.side, seen[i]) {
                (Side::Austria, false) => &mats.austria,
                (Side::Austria, true) => &mats.austria_seen,
                (Side::Italy, false) => &mats.italy,
                (Side::Italy, true) => &mats.italy_seen,
            }
        };
        if mat.0 != *want {
            mat.0 = want.clone();
        }
    }
}
