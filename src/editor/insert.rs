use avian3d::prelude::*;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::EguiContexts;

use super::camera::EditorCamera;
use super::state::{EditorMode, EditorState, InsertObjectType, InsertPreview, InsertState, SnapSubMode, StartInsertEvent};
use crate::commands::TakeSnapshotCommand;
use bevy_spline_3d::prelude::SplineType;

use crate::scene::{
    blockout::{generate_stairs_mesh, generate_ramp_mesh, generate_arch_mesh, generate_lshape_mesh,
               StairsMarker, RampMarker, ArchMarker, LShapeMarker, blockout_colors},
    GroupMarker, GltfSource, PrimitiveMarker, PrimitiveShape, SceneSource, SpawnEntityEvent,
    SpawnEntityKind, SpawnGltfEvent, SpawnSceneSourceEvent,
};
use crate::utils::{get_half_height_along_normal, rotation_from_normal};

pub struct InsertModePlugin;

impl Plugin for InsertModePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                handle_start_insert,
                handle_insert_submode_keys,
                update_preview_position,
                handle_insert_click,
                cleanup_on_mode_exit,
            )
                .run_if(in_state(EditorMode::Insert)),
        )
        .add_systems(OnExit(EditorMode::Insert), cleanup_preview);
    }
}

/// Handle the StartInsertEvent to create a preview entity
fn handle_start_insert(
    mut events: MessageReader<StartInsertEvent>,
    mut insert_state: ResMut<InsertState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for event in events.read() {
        // Clean up any existing preview
        if let Some(old_preview) = insert_state.preview_entity.take() {
            commands.entity(old_preview).despawn();
        }

        // Create new preview entity
        let preview_entity = spawn_preview_entity(
            &mut commands,
            &mut meshes,
            &mut materials,
            event.object_type,
            insert_state.gltf_path.as_deref(),
            insert_state.scene_path.as_deref(),
        );

        insert_state.object_type = Some(event.object_type);
        insert_state.preview_entity = Some(preview_entity);

        info!("Insert mode: placing {:?}", event.object_type);
    }
}

/// Handle scroll wheel to cycle snap sub-mode in Insert mode
fn handle_insert_submode_keys(
    scroll: Res<AccumulatedMouseScroll>,
    editor_state: Res<EditorState>,
    mut snap_submode: ResMut<SnapSubMode>,
    mut contexts: EguiContexts,
) {
    // Don't handle when editor is disabled
    if !editor_state.editor_active {
        return;
    }

    // Don't handle when UI wants pointer input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }

    let scroll_y = scroll.delta.y;
    if scroll_y == 0.0 {
        return;
    }

    // Scroll up = next mode, scroll down = previous mode
    let new_mode = if scroll_y > 0.0 {
        match *snap_submode {
            SnapSubMode::Surface => SnapSubMode::Center,
            SnapSubMode::Center => SnapSubMode::Aligned,
            SnapSubMode::Aligned => SnapSubMode::Vertex,
            SnapSubMode::Vertex => SnapSubMode::Surface,
        }
    } else {
        match *snap_submode {
            SnapSubMode::Surface => SnapSubMode::Vertex,
            SnapSubMode::Center => SnapSubMode::Surface,
            SnapSubMode::Aligned => SnapSubMode::Center,
            SnapSubMode::Vertex => SnapSubMode::Aligned,
        }
    };

    *snap_submode = new_mode;
    let mode_name = match new_mode {
        SnapSubMode::Surface => "Surface",
        SnapSubMode::Center => "Center",
        SnapSubMode::Aligned => "Aligned",
        SnapSubMode::Vertex => "Vertex",
    };
    info!("Insert mode: {}", mode_name);
}

/// Create a preview entity for the given object type
pub fn spawn_preview_entity(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    object_type: InsertObjectType,
    gltf_path: Option<&str>,
    scene_path: Option<&str>,
) -> Entity {
    // Semi-transparent material for preview
    let preview_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.3, 0.7, 1.0, 0.5),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    match object_type {
        InsertObjectType::Primitive(shape) => {
            let mesh = match shape {
                PrimitiveShape::Cube => meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
                PrimitiveShape::Sphere => meshes.add(Sphere::new(0.5)),
                PrimitiveShape::Cylinder => meshes.add(Cylinder::new(0.5, 1.0)),
                PrimitiveShape::Capsule => meshes.add(Capsule3d::new(0.25, 0.5)),
                PrimitiveShape::Plane => meshes.add(Plane3d::default().mesh().size(2.0, 2.0)),
            };

            commands
                .spawn((
                    InsertPreview,
                    PrimitiveMarker { shape },
                    Mesh3d(mesh),
                    MeshMaterial3d(preview_material),
                    Transform::from_translation(Vec3::ZERO),
                ))
                .id()
        }
        InsertObjectType::PointLight => {
            // For point lights, show a small sphere as preview
            commands
                .spawn((
                    InsertPreview,
                    Mesh3d(meshes.add(Sphere::new(0.3))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgba(1.0, 0.9, 0.5, 0.7),
                        alpha_mode: AlphaMode::Blend,
                        emissive: bevy::color::LinearRgba::new(1.0, 0.9, 0.5, 1.0) * 5.0,
                        ..default()
                    })),
                    Transform::from_translation(Vec3::ZERO),
                ))
                .id()
        }
        InsertObjectType::DirectionalLight => {
            // For directional lights (sun), show a larger glowing sphere
            commands
                .spawn((
                    InsertPreview,
                    Mesh3d(meshes.add(Sphere::new(0.5))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgba(1.0, 0.95, 0.4, 0.7),
                        alpha_mode: AlphaMode::Blend,
                        emissive: bevy::color::LinearRgba::new(1.0, 0.95, 0.4, 1.0) * 8.0,
                        ..default()
                    })),
                    Transform::from_translation(Vec3::ZERO),
                ))
                .id()
        }
        InsertObjectType::Group => {
            // For groups, show a wireframe-ish cube indicator
            commands
                .spawn((
                    InsertPreview,
                    GroupMarker,
                    Mesh3d(meshes.add(Cuboid::new(0.5, 0.5, 0.5))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgba(0.5, 1.0, 0.5, 0.3),
                        alpha_mode: AlphaMode::Blend,
                        ..default()
                    })),
                    Transform::from_translation(Vec3::ZERO),
                ))
                .id()
        }
        InsertObjectType::Gltf => {
            // For GLTF files, load the actual model as preview
            if let Some(path) = gltf_path {
                commands
                    .spawn((
                        InsertPreview,
                        GltfSource {
                            path: path.to_string(),
                            scene_index: 0,
                        },
                        Transform::from_translation(Vec3::ZERO),
                    ))
                    .id()
            } else {
                // Fallback to placeholder if no path (shouldn't happen)
                commands
                    .spawn((
                        InsertPreview,
                        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color: Color::srgba(1.0, 0.6, 0.2, 0.5),
                            alpha_mode: AlphaMode::Blend,
                            ..default()
                        })),
                        Transform::from_translation(Vec3::ZERO),
                    ))
                    .id()
            }
        }
        InsertObjectType::Scene => {
            // For scene files, load the actual scene as preview
            if let Some(path) = scene_path {
                commands
                    .spawn((
                        InsertPreview,
                        GroupMarker,
                        SceneSource {
                            path: path.to_string(),
                        },
                        Transform::from_translation(Vec3::ZERO),
                    ))
                    .id()
            } else {
                // Fallback to placeholder if no path (shouldn't happen)
                commands
                    .spawn((
                        InsertPreview,
                        GroupMarker,
                        Mesh3d(meshes.add(Cuboid::new(0.5, 0.5, 0.5))),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color: Color::srgba(0.2, 0.8, 0.4, 0.5),
                            alpha_mode: AlphaMode::Blend,
                            ..default()
                        })),
                        Transform::from_translation(Vec3::ZERO),
                    ))
                    .id()
            }
        }
        InsertObjectType::Spline(_spline_type) => {
            // For splines, show a simple line indicator as preview
            // The actual spline gizmos will be handled by the library
            commands
                .spawn((
                    InsertPreview,
                    Mesh3d(meshes.add(Capsule3d::new(0.1, 2.0))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgba(0.2, 0.6, 1.0, 0.5),
                        alpha_mode: AlphaMode::Blend,
                        ..default()
                    })),
                    Transform::from_translation(Vec3::ZERO),
                ))
                .id()
        }
        InsertObjectType::FogVolume => {
            // For fog volumes, show a semi-transparent cube representing the volume bounds
            commands
                .spawn((
                    InsertPreview,
                    Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::srgba(0.5, 0.7, 1.0, 0.3),
                        alpha_mode: AlphaMode::Blend,
                        ..default()
                    })),
                    // Default fog volume size of 10 units
                    Transform::from_translation(Vec3::ZERO).with_scale(Vec3::splat(10.0)),
                ))
                .id()
        }
        InsertObjectType::Stairs => {
            let marker = StairsMarker::default();
            let mesh = generate_stairs_mesh(&marker);
            commands
                .spawn((
                    InsertPreview,
                    marker,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: blockout_colors::STAIRS.with_alpha(0.5),
                        alpha_mode: AlphaMode::Blend,
                        ..default()
                    })),
                    Transform::from_translation(Vec3::ZERO),
                ))
                .id()
        }
        InsertObjectType::Ramp => {
            let marker = RampMarker::default();
            let mesh = generate_ramp_mesh(&marker);
            commands
                .spawn((
                    InsertPreview,
                    marker,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: blockout_colors::RAMP.with_alpha(0.5),
                        alpha_mode: AlphaMode::Blend,
                        ..default()
                    })),
                    Transform::from_translation(Vec3::ZERO),
                ))
                .id()
        }
        InsertObjectType::Arch => {
            let marker = ArchMarker::default();
            let mesh = generate_arch_mesh(&marker);
            commands
                .spawn((
                    InsertPreview,
                    marker,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: blockout_colors::ARCH.with_alpha(0.5),
                        alpha_mode: AlphaMode::Blend,
                        ..default()
                    })),
                    Transform::from_translation(Vec3::ZERO),
                ))
                .id()
        }
        InsertObjectType::LShape => {
            let marker = LShapeMarker::default();
            let mesh = generate_lshape_mesh(&marker);
            commands
                .spawn((
                    InsertPreview,
                    marker,
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: blockout_colors::LSHAPE.with_alpha(0.5),
                        alpha_mode: AlphaMode::Blend,
                        ..default()
                    })),
                    Transform::from_translation(Vec3::ZERO),
                ))
                .id()
        }
    }
}

/// Update the preview entity position based on camera raycast
fn update_preview_position(
    insert_state: Res<InsertState>,
    snap_submode: Res<SnapSubMode>,
    camera_query: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    spatial_query: SpatialQuery,
    mut preview_query: Query<(&mut Transform, Option<&Collider>), With<InsertPreview>>,
    target_query: Query<(&Transform, Option<&Collider>), Without<InsertPreview>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
) {
    let Some(preview_entity) = insert_state.preview_entity else {
        return;
    };

    let Ok((mut preview_transform, preview_collider)) = preview_query.get_mut(preview_entity) else {
        return;
    };

    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };

    let Ok(window) = window_query.single() else {
        return;
    };

    // Get the cursor position (use center as fallback if cursor not in window)
    let cursor_position = window
        .cursor_position()
        .unwrap_or_else(|| Vec2::new(window.width() / 2.0, window.height() / 2.0));

    // Create ray from camera through cursor position
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else {
        return;
    };

    // Cast ray against physics colliders (exclude preview entity)
    let filter = SpatialQueryFilter::default().with_excluded_entities([preview_entity]);

    let Some(hit) = spatial_query.cast_ray(
        ray.origin,
        ray.direction,
        100.0,
        true,
        &filter,
    ) else {
        // No hit - position at default distance from camera
        preview_transform.translation = ray.origin + ray.direction * insert_state.default_distance;
        preview_transform.rotation = Quat::IDENTITY;
        return;
    };

    let hit_point = ray.origin + ray.direction * hit.distance;
    let surface_normal = hit.normal.normalize();

    // Get preview object half-height from collider AABB
    let half_height = get_half_height_along_normal(preview_collider, surface_normal);

    match *snap_submode {
        SnapSubMode::Surface => {
            // Surface mode: align Y axis with surface normal
            let rotation = rotation_from_normal(surface_normal);
            let position = hit_point + surface_normal * half_height;
            preview_transform.translation = position;
            preview_transform.rotation = rotation;
        }
        SnapSubMode::Center => {
            // Center mode: align centers through AABBs (world-axis aligned)
            let Ok((target_transform, target_collider)) = target_query.get(hit.entity) else {
                // Fallback to surface mode
                let position = hit_point + surface_normal * half_height;
                preview_transform.translation = position;
                return;
            };

            let target_half_extents = target_collider
                .map(|c| c.aabb(Vec3::ZERO, Quat::IDENTITY).size() * 0.5)
                .unwrap_or(Vec3::splat(0.5));

            let target_center = target_transform.translation;

            // Determine which axis the surface normal is most aligned with
            let abs_normal = surface_normal.abs();
            let (primary_axis, axis_idx) = if abs_normal.x >= abs_normal.y && abs_normal.x >= abs_normal.z {
                (Vec3::X, 0)
            } else if abs_normal.y >= abs_normal.x && abs_normal.y >= abs_normal.z {
                (Vec3::Y, 1)
            } else {
                (Vec3::Z, 2)
            };

            let axis_offset = if surface_normal.dot(primary_axis) > 0.0 {
                primary_axis
            } else {
                -primary_axis
            };

            let target_extent = match axis_idx {
                0 => target_half_extents.x,
                1 => target_half_extents.y,
                _ => target_half_extents.z,
            };

            let position = target_center + axis_offset * (target_extent + half_height);
            preview_transform.translation = position;
            // Don't change rotation in center mode
        }
        SnapSubMode::Aligned => {
            // Aligned mode: use target's rotation for off-axis objects
            let Ok((target_transform, target_collider)) = target_query.get(hit.entity) else {
                // Fallback to surface mode
                let rotation = rotation_from_normal(surface_normal);
                let position = hit_point + surface_normal * half_height;
                preview_transform.translation = position;
                preview_transform.rotation = rotation;
                return;
            };

            let target_half_extents = target_collider
                .map(|c| c.aabb(Vec3::ZERO, Quat::IDENTITY).size() * 0.5)
                .unwrap_or(Vec3::splat(0.5));

            let target_center = target_transform.translation;
            let target_rotation = target_transform.rotation;

            // Transform surface normal into target's local space
            let local_normal = target_rotation.inverse() * surface_normal;

            // Determine which local axis the surface normal is most aligned with
            let abs_local_normal = local_normal.abs();
            let (local_axis, axis_idx) = if abs_local_normal.x >= abs_local_normal.y && abs_local_normal.x >= abs_local_normal.z {
                (Vec3::X, 0)
            } else if abs_local_normal.y >= abs_local_normal.x && abs_local_normal.y >= abs_local_normal.z {
                (Vec3::Y, 1)
            } else {
                (Vec3::Z, 2)
            };

            // Get the world-space direction for this local axis
            let world_axis = target_rotation * local_axis;
            let axis_sign = if local_normal.dot(local_axis) > 0.0 { 1.0 } else { -1.0 };

            let target_extent = match axis_idx {
                0 => target_half_extents.x,
                1 => target_half_extents.y,
                _ => target_half_extents.z,
            };

            let position = target_center + world_axis * axis_sign * (target_extent + half_height);
            preview_transform.translation = position;
            preview_transform.rotation = target_rotation;
        }
        SnapSubMode::Vertex => {
            // Vertex mode: snap to exact hit point (vertex snapping simplified)
            // This places the object center at the hit point
            preview_transform.translation = hit_point;
            // Keep current rotation in vertex mode
        }
    }
}

/// Handle click to confirm placement
fn handle_insert_click(
    mouse_button: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut insert_state: ResMut<InsertState>,
    mut next_mode: ResMut<NextState<EditorMode>>,
    preview_query: Query<&Transform, With<InsertPreview>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut spawn_entity_events: MessageWriter<SpawnEntityEvent>,
    mut spawn_gltf_events: MessageWriter<SpawnGltfEvent>,
    mut spawn_scene_events: MessageWriter<SpawnSceneSourceEvent>,
    mut contexts: EguiContexts,
) {
    // Only confirm on left click
    if !mouse_button.just_pressed(MouseButton::Left) {
        return;
    }

    // Don't process if clicking on UI
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }

    let Some(object_type) = insert_state.object_type else {
        return;
    };

    let Some(preview_entity) = insert_state.preview_entity else {
        return;
    };

    let Ok(preview_transform) = preview_query.get(preview_entity) else {
        return;
    };

    let position = preview_transform.translation;
    let rotation = preview_transform.rotation;

    // Take snapshot before inserting
    let object_name = match &object_type {
        InsertObjectType::Primitive(shape) => shape.display_name().to_string(),
        InsertObjectType::PointLight => "Point Light".to_string(),
        InsertObjectType::DirectionalLight => "Directional Light".to_string(),
        InsertObjectType::Group => "Group".to_string(),
        InsertObjectType::Gltf => {
            insert_state.gltf_path.as_ref()
                .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
                .unwrap_or_else(|| "GLTF".to_string())
        }
        InsertObjectType::Scene => {
            insert_state.scene_path.as_ref()
                .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
                .unwrap_or_else(|| "Scene".to_string())
        }
        InsertObjectType::Spline(spline_type) => match spline_type {
            SplineType::CubicBezier => "Bezier Spline".to_string(),
            SplineType::CatmullRom => "Catmull-Rom Spline".to_string(),
            SplineType::BSpline => "B-Spline".to_string(),
        },
        InsertObjectType::FogVolume => "Fog Volume".to_string(),
        InsertObjectType::Stairs => "Stairs".to_string(),
        InsertObjectType::Ramp => "Ramp".to_string(),
        InsertObjectType::Arch => "Arch".to_string(),
        InsertObjectType::LShape => "L-Shape".to_string(),
    };
    commands.queue(TakeSnapshotCommand {
        description: format!("Insert {}", object_name),
    });

    // Spawn the actual object
    match object_type {
        InsertObjectType::Primitive(shape) => {
            spawn_entity_events.write(SpawnEntityEvent {
                kind: SpawnEntityKind::Primitive(shape),
                position,
                rotation,
            });
        }
        InsertObjectType::PointLight => {
            spawn_entity_events.write(SpawnEntityEvent {
                kind: SpawnEntityKind::PointLight,
                position,
                rotation,
            });
        }
        InsertObjectType::DirectionalLight => {
            spawn_entity_events.write(SpawnEntityEvent {
                kind: SpawnEntityKind::DirectionalLight,
                position,
                rotation,
            });
        }
        InsertObjectType::Group => {
            spawn_entity_events.write(SpawnEntityEvent {
                kind: SpawnEntityKind::Group,
                position,
                rotation,
            });
        }
        InsertObjectType::Gltf => {
            if let Some(gltf_path) = insert_state.gltf_path.clone() {
                spawn_gltf_events.write(SpawnGltfEvent {
                    path: gltf_path,
                    position,
                    rotation,
                });
            }
        }
        InsertObjectType::Scene => {
            if let Some(scene_path) = insert_state.scene_path.clone() {
                spawn_scene_events.write(SpawnSceneSourceEvent {
                    path: scene_path,
                    position,
                    rotation,
                });
            }
        }
        InsertObjectType::Spline(spline_type) => {
            spawn_entity_events.write(SpawnEntityEvent {
                kind: SpawnEntityKind::Spline(spline_type),
                position,
                rotation,
            });
        }
        InsertObjectType::FogVolume => {
            spawn_entity_events.write(SpawnEntityEvent {
                kind: SpawnEntityKind::FogVolume,
                position,
                rotation,
            });
        }
        InsertObjectType::Stairs => {
            spawn_entity_events.write(SpawnEntityEvent {
                kind: SpawnEntityKind::Stairs,
                position,
                rotation,
            });
        }
        InsertObjectType::Ramp => {
            spawn_entity_events.write(SpawnEntityEvent {
                kind: SpawnEntityKind::Ramp,
                position,
                rotation,
            });
        }
        InsertObjectType::Arch => {
            spawn_entity_events.write(SpawnEntityEvent {
                kind: SpawnEntityKind::Arch,
                position,
                rotation,
            });
        }
        InsertObjectType::LShape => {
            spawn_entity_events.write(SpawnEntityEvent {
                kind: SpawnEntityKind::LShape,
                position,
                rotation,
            });
        }
    }

    // Remove old preview entity
    commands.entity(preview_entity).despawn();

    // Check if shift is held for multi-place mode
    let shift_held = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);

    if shift_held {
        // Create a new preview entity to continue placing
        let new_preview = spawn_preview_entity(
            &mut commands,
            &mut meshes,
            &mut materials,
            object_type,
            insert_state.gltf_path.as_deref(),
            insert_state.scene_path.as_deref(),
        );
        insert_state.preview_entity = Some(new_preview);
        info!("Placed {:?} at {:?} (shift-placing)", object_type, position);
    } else {
        // Clear insert state and return to View mode
        insert_state.object_type = None;
        insert_state.preview_entity = None;
        insert_state.gltf_path = None;
        insert_state.scene_path = None;
        next_mode.set(EditorMode::View);
        info!("Placed {:?} at {:?}", object_type, position);
    }
}

/// Clean up preview if mode changes while inserting
fn cleanup_on_mode_exit(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut insert_state: ResMut<InsertState>,
    mut commands: Commands,
) {
    // Cancel on Escape (handled in input.rs for mode change,
    // but we need to clean up preview here)
    if keyboard.just_pressed(KeyCode::Escape) {
        if let Some(preview_entity) = insert_state.preview_entity.take() {
            commands.entity(preview_entity).despawn();
        }
        insert_state.object_type = None;
        insert_state.gltf_path = None;
        insert_state.scene_path = None;
    }
}

/// Clean up preview entity when exiting Insert mode
fn cleanup_preview(
    mut insert_state: ResMut<InsertState>,
    mut commands: Commands,
    preview_query: Query<Entity, With<InsertPreview>>,
) {
    // Remove all preview entities
    for entity in preview_query.iter() {
        commands.entity(entity).despawn();
    }

    insert_state.object_type = None;
    insert_state.preview_entity = None;
    insert_state.gltf_path = None;
    insert_state.scene_path = None;
}
