//! Editor ground grid (spec §4 `Space t` toggles; v1-parity spatial anchor):
//! a line-list mesh on y≈0 — minor lines every meter, brighter majors every 10,
//! colored X/Z axis lines through the origin. Editor-only visibility, never
//! pickable, toggled by `view.toggle-grid`.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Mesh, PrimitiveTopology};
use bevy::prelude::*;
use editor_core::prelude::*;

const EXTENT: i32 = 60;
const MAJOR_EVERY: i32 = 10;

#[derive(Resource)]
pub(crate) struct GridVisible(pub bool);

impl Default for GridVisible {
    fn default() -> Self {
        Self(true)
    }
}

#[derive(Component)]
pub(crate) struct EditorGrid;

fn line_mesh(lines: &[(Vec3, Vec3)]) -> Mesh {
    let mut positions = Vec::with_capacity(lines.len() * 2);
    for (a, b) in lines {
        positions.push([a.x, a.y, a.z]);
        positions.push([b.x, b.y, b.z]);
    }
    Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
}

pub(crate) fn spawn_grid(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut minor = Vec::new();
    let mut major = Vec::new();
    let extent = EXTENT as f32;
    for i in -EXTENT..=EXTENT {
        if i == 0 {
            continue; // axis lines drawn separately
        }
        let offset = i as f32;
        let bucket = if i % MAJOR_EVERY == 0 {
            &mut major
        } else {
            &mut minor
        };
        bucket.push((
            Vec3::new(offset, 0.0, -extent),
            Vec3::new(offset, 0.0, extent),
        ));
        bucket.push((
            Vec3::new(-extent, 0.0, offset),
            Vec3::new(extent, 0.0, offset),
        ));
    }

    let unlit = |color: Color, materials: &mut Assets<StandardMaterial>| {
        materials.add(StandardMaterial {
            base_color: color,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })
    };
    // Slightly above the ground plane so co-planar geometry never z-fights.
    let layer = |y: f32| Transform::from_xyz(0.0, y, 0.0);

    commands.spawn((
        EditorGrid,
        Mesh3d(meshes.add(line_mesh(&minor))),
        MeshMaterial3d(unlit(Color::srgba(1.0, 1.0, 1.0, 0.08), &mut materials)),
        layer(0.002),
        bevy::picking::Pickable::IGNORE,
        Visibility::Hidden,
    ));
    commands.spawn((
        EditorGrid,
        Mesh3d(meshes.add(line_mesh(&major))),
        MeshMaterial3d(unlit(Color::srgba(1.0, 1.0, 1.0, 0.20), &mut materials)),
        layer(0.003),
        bevy::picking::Pickable::IGNORE,
        Visibility::Hidden,
    ));
    // Origin axes: X red, Z blue (the world's compass).
    let axes = [
        (
            Vec3::new(-extent, 0.0, 0.0),
            Vec3::new(extent, 0.0, 0.0),
            Color::srgba(0.9, 0.35, 0.35, 0.35),
        ),
        (
            Vec3::new(0.0, 0.0, -extent),
            Vec3::new(0.0, 0.0, extent),
            Color::srgba(0.35, 0.55, 0.95, 0.35),
        ),
    ];
    for (a, b, color) in axes {
        commands.spawn((
            EditorGrid,
            Mesh3d(meshes.add(line_mesh(&[(a, b)]))),
            MeshMaterial3d(unlit(color, &mut materials)),
            layer(0.004),
            bevy::picking::Pickable::IGNORE,
            Visibility::Hidden,
        ));
    }
}

pub(crate) fn handle_grid_actions(
    mut reader: MessageReader<ActionInvoked>,
    mut visible: ResMut<GridVisible>,
) {
    for invoked in reader.read() {
        if invoked.action.as_str() == "view.toggle-grid" {
            visible.0 = !visible.0;
        }
    }
}

/// Grid shows in the editor, never in the game — and respects the toggle.
pub(crate) fn sync_grid(
    state: Res<EditorState>,
    visible: Res<GridVisible>,
    mut grid: Query<&mut Visibility, With<EditorGrid>>,
) {
    let show = state.active && visible.0;
    for mut visibility in &mut grid {
        let target = if show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != target {
            *visibility = target;
        }
    }
}
