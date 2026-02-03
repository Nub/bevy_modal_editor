//! Blockout Mode - Keyboard-first tile snapping for rapid prototyping
//!
//! Enter with B key from View mode. Number keys select shapes,
//! WASDQE select faces, R rotates, Enter places.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::commands::TakeSnapshotCommand;
use crate::scene::blockout::*;
use crate::scene::SceneEntity;
use crate::selection::Selected;
use crate::utils::should_process_input;

use super::state::{EditorMode, EditorState};

/// Available shapes in Blockout mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Resource)]
pub enum BlockoutShapeSelection {
    #[default]
    Cube,
    Stairs,
    Ramp,
    Arch,
    LShape,
}

impl BlockoutShapeSelection {
    pub fn display_name(&self) -> &'static str {
        match self {
            BlockoutShapeSelection::Cube => "Cube",
            BlockoutShapeSelection::Stairs => "Stairs",
            BlockoutShapeSelection::Ramp => "Ramp",
            BlockoutShapeSelection::Arch => "Arch",
            BlockoutShapeSelection::LShape => "L-Shape",
        }
    }

    /// Get the approximate AABB half-extents for this shape (for snapping calculations)
    pub fn half_extents(&self) -> Vec3 {
        match self {
            BlockoutShapeSelection::Cube => Vec3::splat(0.5),
            BlockoutShapeSelection::Stairs => {
                let s = StairsMarker::default();
                Vec3::new(s.width / 2.0, s.height / 2.0, s.depth / 2.0)
            }
            BlockoutShapeSelection::Ramp => {
                let r = RampMarker::default();
                Vec3::new(r.width / 2.0, r.height / 2.0, r.length / 2.0)
            }
            BlockoutShapeSelection::Arch => {
                let a = ArchMarker::default();
                Vec3::new(a.wall_width / 2.0, a.wall_height / 2.0, a.thickness / 2.0)
            }
            BlockoutShapeSelection::LShape => {
                let l = LShapeMarker::default();
                Vec3::new(l.arm1_length / 2.0, l.height / 2.0, l.arm2_length / 2.0)
            }
        }
    }
}

/// Face selection for snapping
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Face {
    #[default]
    PosX, // Right (D key)
    NegX, // Left (A key)
    PosY, // Top (E key)
    NegY, // Bottom (Q key)
    PosZ, // Front (S key)
    NegZ, // Back (W key)
}

impl Face {
    pub fn normal(&self) -> Vec3 {
        match self {
            Face::PosX => Vec3::X,
            Face::NegX => Vec3::NEG_X,
            Face::PosY => Vec3::Y,
            Face::NegY => Vec3::NEG_Y,
            Face::PosZ => Vec3::Z,
            Face::NegZ => Vec3::NEG_Z,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Face::PosX => "+X (Right)",
            Face::NegX => "-X (Left)",
            Face::PosY => "+Y (Top)",
            Face::NegY => "-Y (Bottom)",
            Face::PosZ => "+Z (Front)",
            Face::NegZ => "-Z (Back)",
        }
    }
}

/// State for Blockout mode
#[derive(Resource)]
pub struct BlockoutState {
    /// Currently selected shape to place
    pub selected_shape: BlockoutShapeSelection,
    /// Preview entity showing where tile will be placed
    pub preview_entity: Option<Entity>,
    /// Which face of the anchor entity is selected for snapping
    pub selected_face: Face,
    /// Current 90-degree rotation index (0-3)
    pub rotation_index: u8,
    /// The entity we're snapping to (usually last placed or selected)
    pub anchor_entity: Option<Entity>,
}

impl Default for BlockoutState {
    fn default() -> Self {
        Self {
            selected_shape: BlockoutShapeSelection::default(),
            preview_entity: None,
            selected_face: Face::default(),
            rotation_index: 0,
            anchor_entity: None,
        }
    }
}

impl BlockoutState {
    pub fn reset(&mut self) {
        self.selected_shape = BlockoutShapeSelection::default();
        self.preview_entity = None;
        self.selected_face = Face::default();
        self.rotation_index = 0;
        self.anchor_entity = None;
    }
}

/// Marker component for blockout preview entities
#[derive(Component)]
pub struct BlockoutPreview;

/// Marker component for entities just placed in blockout mode (for anchor chaining)
#[derive(Component)]
pub struct JustPlacedBlockout;

pub struct BlockoutPlugin;

impl Plugin for BlockoutPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlockoutState>()
            .add_systems(OnEnter(EditorMode::Blockout), on_enter_blockout_mode)
            .add_systems(OnExit(EditorMode::Blockout), on_exit_blockout_mode)
            .add_systems(
                Update,
                (
                    handle_blockout_input,
                    update_blockout_preview,
                    draw_blockout_face_highlight,
                    draw_blockout_hud,
                )
                    .run_if(in_state(EditorMode::Blockout)),
            );
    }
}

/// Initialize blockout mode when entering
fn on_enter_blockout_mode(
    mut blockout_state: ResMut<BlockoutState>,
    selected: Query<Entity, With<Selected>>,
) {
    // Use selected entity as initial anchor
    blockout_state.anchor_entity = selected.iter().next();
    blockout_state.rotation_index = 0;
    info!("Entered Blockout Mode - Shape: {}", blockout_state.selected_shape.display_name());
}

/// Cleanup when exiting blockout mode
fn on_exit_blockout_mode(
    mut commands: Commands,
    mut blockout_state: ResMut<BlockoutState>,
    preview_query: Query<Entity, With<BlockoutPreview>>,
) {
    // Despawn preview entity
    for entity in preview_query.iter() {
        commands.entity(entity).despawn();
    }
    blockout_state.preview_entity = None;
    info!("Exited Blockout Mode");
}

/// Handle keyboard input in blockout mode
fn handle_blockout_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut blockout_state: ResMut<BlockoutState>,
    mut next_mode: ResMut<NextState<EditorMode>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GridMat>>,
    preview_query: Query<&Transform, With<BlockoutPreview>>,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Escape exits blockout mode
    if keyboard.just_pressed(KeyCode::Escape) {
        next_mode.set(EditorMode::View);
        return;
    }

    // Number keys select shape
    if keyboard.just_pressed(KeyCode::Digit1) {
        blockout_state.selected_shape = BlockoutShapeSelection::Cube;
        info!("Shape: Cube");
    } else if keyboard.just_pressed(KeyCode::Digit2) {
        blockout_state.selected_shape = BlockoutShapeSelection::Stairs;
        info!("Shape: Stairs");
    } else if keyboard.just_pressed(KeyCode::Digit3) {
        blockout_state.selected_shape = BlockoutShapeSelection::Ramp;
        info!("Shape: Ramp");
    } else if keyboard.just_pressed(KeyCode::Digit4) {
        blockout_state.selected_shape = BlockoutShapeSelection::Arch;
        info!("Shape: Arch");
    } else if keyboard.just_pressed(KeyCode::Digit5) {
        blockout_state.selected_shape = BlockoutShapeSelection::LShape;
        info!("Shape: L-Shape");
    }

    // WASDQE for face selection
    if keyboard.just_pressed(KeyCode::KeyA) {
        blockout_state.selected_face = Face::NegX;
        info!("Face: -X (Left)");
    } else if keyboard.just_pressed(KeyCode::KeyD) {
        blockout_state.selected_face = Face::PosX;
        info!("Face: +X (Right)");
    } else if keyboard.just_pressed(KeyCode::KeyW) {
        blockout_state.selected_face = Face::NegZ;
        info!("Face: -Z (Back)");
    } else if keyboard.just_pressed(KeyCode::KeyS) {
        blockout_state.selected_face = Face::PosZ;
        info!("Face: +Z (Front)");
    } else if keyboard.just_pressed(KeyCode::KeyQ) {
        blockout_state.selected_face = Face::NegY;
        info!("Face: -Y (Bottom)");
    } else if keyboard.just_pressed(KeyCode::KeyE) {
        blockout_state.selected_face = Face::PosY;
        info!("Face: +Y (Top)");
    }

    // R rotates preview 90 degrees
    if keyboard.just_pressed(KeyCode::KeyR) {
        blockout_state.rotation_index = (blockout_state.rotation_index + 1) % 4;
        let angle = blockout_state.rotation_index as f32 * 90.0;
        info!("Rotation: {}°", angle);
    }

    // Enter places the tile
    if keyboard.just_pressed(KeyCode::Enter) {
        let (position, rotation) = if let Some(preview_entity) = blockout_state.preview_entity {
            if let Ok(preview_transform) = preview_query.get(preview_entity) {
                (preview_transform.translation, preview_transform.rotation)
            } else {
                return;
            }
        } else if blockout_state.anchor_entity.is_none() {
            // No anchor - place at origin
            (Vec3::ZERO, Quat::IDENTITY)
        } else {
            return;
        };

        // Take snapshot for undo
        commands.queue(TakeSnapshotCommand {
            description: format!("Place {}", blockout_state.selected_shape.display_name()),
        });

        // Spawn the actual entity directly to get the entity ID for anchor chaining
        let new_entity = match blockout_state.selected_shape {
            BlockoutShapeSelection::Cube => {
                use bevy::pbr::ExtendedMaterial;
                use bevy_grid_shader::GridMaterial;
                use crate::scene::PrimitiveShape;
                let shape = PrimitiveShape::Cube;
                commands
                    .spawn((
                        SceneEntity,
                        Name::new("Cube"),
                        Mesh3d(meshes.add(shape.create_mesh())),
                        MeshMaterial3d(materials.add(ExtendedMaterial {
                            base: shape.create_material(),
                            extension: GridMaterial::default(),
                        })),
                        Transform::from_translation(position).with_rotation(rotation),
                        RigidBody::Static,
                        shape.create_collider(),
                        crate::scene::PrimitiveMarker { shape },
                    ))
                    .id()
            }
            BlockoutShapeSelection::Stairs => {
                spawn_stairs(&mut commands, &mut meshes, &mut materials, position, rotation, "Stairs")
            }
            BlockoutShapeSelection::Ramp => {
                spawn_ramp(&mut commands, &mut meshes, &mut materials, position, rotation, "Ramp")
            }
            BlockoutShapeSelection::Arch => {
                spawn_arch(&mut commands, &mut meshes, &mut materials, position, rotation, "Arch")
            }
            BlockoutShapeSelection::LShape => {
                spawn_lshape(&mut commands, &mut meshes, &mut materials, position, rotation, "L-Shape")
            }
        };

        // Set new entity as anchor for chaining
        blockout_state.anchor_entity = Some(new_entity);
        info!("Placed {} at {:?} (now anchor)", blockout_state.selected_shape.display_name(), position);
    }
}

/// Calculate the snap position for placing a new shape adjacent to an anchor
fn calculate_snap_transform(
    anchor_transform: &Transform,
    anchor_collider: Option<&Collider>,
    target_face: Face,
    shape: BlockoutShapeSelection,
    rotation_index: u8,
) -> Transform {
    // Get anchor AABB (from collider or estimate)
    let anchor_half_extents = if let Some(collider) = anchor_collider {
        let he = collider.shape_scaled().compute_local_aabb().half_extents();
        Vec3::new(he.x, he.y, he.z)
    } else {
        Vec3::splat(0.5)
    };

    // Get shape half extents
    let shape_half_extents = shape.half_extents();

    // Face normal in world space
    let face_normal = anchor_transform.rotation * target_face.normal();

    // Calculate the center of the anchor's face
    let local_normal = target_face.normal();
    let face_offset = anchor_half_extents * local_normal.abs();
    let face_center = anchor_transform.translation + anchor_transform.rotation * (local_normal * face_offset.length());

    // Calculate how far the new shape needs to be offset from face center
    // We need to offset by the shape's half-extent along the face normal direction
    let shape_offset = shape_half_extents.dot(local_normal.abs());

    // Final position
    let position = face_center + face_normal * shape_offset;

    // Base rotation aligns the shape to face the same direction
    // For most shapes, no additional base rotation needed
    let base_rotation = anchor_transform.rotation;

    // Apply 90-degree rotation increments around the face normal
    let angle = (rotation_index as f32) * std::f32::consts::FRAC_PI_2;
    let rotation = base_rotation * Quat::from_axis_angle(local_normal, angle);

    Transform::from_translation(position).with_rotation(rotation)
}

/// Update the preview ghost entity
fn update_blockout_preview(
    mut commands: Commands,
    mut blockout_state: ResMut<BlockoutState>,
    anchor_query: Query<(&Transform, Option<&Collider>), With<SceneEntity>>,
    mut preview_query: Query<(&mut Transform, &mut Mesh3d), (With<BlockoutPreview>, Without<SceneEntity>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(anchor_entity) = blockout_state.anchor_entity else {
        // No anchor - despawn preview if exists
        if let Some(preview_entity) = blockout_state.preview_entity {
            commands.entity(preview_entity).despawn();
            blockout_state.preview_entity = None;
        }
        return;
    };

    let Ok((anchor_transform, anchor_collider)) = anchor_query.get(anchor_entity) else {
        // Anchor no longer valid
        blockout_state.anchor_entity = None;
        if let Some(preview_entity) = blockout_state.preview_entity {
            commands.entity(preview_entity).despawn();
            blockout_state.preview_entity = None;
        }
        return;
    };

    // Calculate snap transform
    let snap_transform = calculate_snap_transform(
        anchor_transform,
        anchor_collider,
        blockout_state.selected_face,
        blockout_state.selected_shape,
        blockout_state.rotation_index,
    );

    // Copy shape selection to avoid borrow issues
    let selected_shape = blockout_state.selected_shape;

    // Helper to create preview mesh for current shape
    let create_preview_mesh = || -> Mesh {
        match selected_shape {
            BlockoutShapeSelection::Cube => Cuboid::new(1.0, 1.0, 1.0).into(),
            BlockoutShapeSelection::Stairs => generate_stairs_mesh(&StairsMarker::default()),
            BlockoutShapeSelection::Ramp => generate_ramp_mesh(&RampMarker::default()),
            BlockoutShapeSelection::Arch => generate_arch_mesh(&ArchMarker::default()),
            BlockoutShapeSelection::LShape => generate_lshape_mesh(&LShapeMarker::default()),
        }
    };

    if let Some(preview_entity) = blockout_state.preview_entity {
        // Update existing preview
        if let Ok((mut transform, mut mesh_handle)) = preview_query.get_mut(preview_entity) {
            *transform = snap_transform;
            mesh_handle.0 = meshes.add(create_preview_mesh());
        } else {
            // Preview entity invalid, recreate
            commands.entity(preview_entity).despawn();
            blockout_state.preview_entity = None;
        }
    }

    // Create preview if needed
    if blockout_state.preview_entity.is_none() {
        let preview_entity = commands
            .spawn((
                BlockoutPreview,
                Name::new("Blockout Preview"),
                Mesh3d(meshes.add(create_preview_mesh())),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgba(0.2, 0.6, 1.0, 0.4),
                    alpha_mode: AlphaMode::Blend,
                    unlit: true,
                    ..default()
                })),
                snap_transform,
            ))
            .id();
        blockout_state.preview_entity = Some(preview_entity);
    }
}

/// Draw a highlight on the selected face of the anchor entity
fn draw_blockout_face_highlight(
    mut gizmos: Gizmos,
    blockout_state: Res<BlockoutState>,
    anchor_query: Query<(&Transform, Option<&Collider>), With<SceneEntity>>,
) {
    let Some(anchor_entity) = blockout_state.anchor_entity else {
        return;
    };

    let Ok((anchor_transform, anchor_collider)) = anchor_query.get(anchor_entity) else {
        return;
    };

    // Get anchor AABB
    let half_extents = if let Some(collider) = anchor_collider {
        let he = collider.shape_scaled().compute_local_aabb().half_extents();
        Vec3::new(he.x, he.y, he.z)
    } else {
        Vec3::splat(0.5)
    };

    // Calculate face center and corners in world space
    let local_normal = blockout_state.selected_face.normal();
    let face_offset = half_extents * local_normal.abs();
    let face_center = anchor_transform.translation + anchor_transform.rotation * (local_normal * face_offset.length());

    // Calculate the two tangent axes for the face
    let (tangent1, tangent2) = match blockout_state.selected_face {
        Face::PosX | Face::NegX => (Vec3::Y, Vec3::Z),
        Face::PosY | Face::NegY => (Vec3::X, Vec3::Z),
        Face::PosZ | Face::NegZ => (Vec3::X, Vec3::Y),
    };

    let t1_extent = half_extents.dot(tangent1.abs());
    let t2_extent = half_extents.dot(tangent2.abs());

    // Transform tangents to world space
    let world_t1 = anchor_transform.rotation * tangent1;
    let world_t2 = anchor_transform.rotation * tangent2;

    // Calculate face corners
    let corner1 = face_center - world_t1 * t1_extent - world_t2 * t2_extent;
    let corner2 = face_center + world_t1 * t1_extent - world_t2 * t2_extent;
    let corner3 = face_center + world_t1 * t1_extent + world_t2 * t2_extent;
    let corner4 = face_center - world_t1 * t1_extent + world_t2 * t2_extent;

    // Draw face outline
    let highlight_color = Color::srgb(1.0, 0.8, 0.0);
    gizmos.line(corner1, corner2, highlight_color);
    gizmos.line(corner2, corner3, highlight_color);
    gizmos.line(corner3, corner4, highlight_color);
    gizmos.line(corner4, corner1, highlight_color);

    // Draw normal arrow from face center
    let arrow_end = face_center + anchor_transform.rotation * local_normal * 0.5;
    gizmos.arrow(face_center, arrow_end, highlight_color);
}

/// Draw HUD overlay showing current shape, face, and rotation
fn draw_blockout_hud(
    mut contexts: EguiContexts,
    blockout_state: Res<BlockoutState>,
) {
    use bevy_egui::egui;

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    // Draw HUD in bottom-left corner
    egui::Area::new(egui::Id::new("blockout_hud"))
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(10.0, -60.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(egui::Color32::from_rgba_unmultiplied(30, 30, 30, 220))
                .corner_radius(4)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_min_width(150.0);

                    // Title
                    ui.label(
                        egui::RichText::new("BLOCKOUT MODE")
                            .strong()
                            .color(egui::Color32::from_rgb(206, 145, 87)),
                    );
                    ui.separator();

                    // Shape
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Shape:").color(egui::Color32::GRAY));
                        let shape_color = match blockout_state.selected_shape {
                            BlockoutShapeSelection::Cube => egui::Color32::from_rgb(150, 150, 160),
                            BlockoutShapeSelection::Stairs => egui::Color32::from_rgb(165, 165, 178),
                            BlockoutShapeSelection::Ramp => egui::Color32::from_rgb(178, 165, 165),
                            BlockoutShapeSelection::Arch => egui::Color32::from_rgb(165, 178, 165),
                            BlockoutShapeSelection::LShape => egui::Color32::from_rgb(178, 178, 165),
                        };
                        ui.label(
                            egui::RichText::new(blockout_state.selected_shape.display_name())
                                .strong()
                                .color(shape_color),
                        );
                    });

                    // Face (only show if we have an anchor)
                    if blockout_state.anchor_entity.is_some() {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Face:").color(egui::Color32::GRAY));
                            let face_color = match blockout_state.selected_face {
                                Face::PosX | Face::NegX => egui::Color32::from_rgb(230, 100, 100),
                                Face::PosY | Face::NegY => egui::Color32::from_rgb(100, 200, 100),
                                Face::PosZ | Face::NegZ => egui::Color32::from_rgb(100, 150, 230),
                            };
                            ui.label(
                                egui::RichText::new(blockout_state.selected_face.display_name())
                                    .strong()
                                    .color(face_color),
                            );
                        });

                        // Rotation
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Rotation:").color(egui::Color32::GRAY));
                            let angle = blockout_state.rotation_index as u32 * 90;
                            ui.label(
                                egui::RichText::new(format!("{}°", angle))
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            );
                        });
                    } else {
                        ui.label(
                            egui::RichText::new("No anchor selected")
                                .italics()
                                .color(egui::Color32::GRAY),
                        );
                        ui.label(
                            egui::RichText::new("Press Enter to place at origin")
                                .small()
                                .color(egui::Color32::DARK_GRAY),
                        );
                    }

                    // Controls hint
                    ui.add_space(4.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("1-5").small().color(egui::Color32::GRAY));
                        ui.label(egui::RichText::new("shape").small().color(egui::Color32::DARK_GRAY));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("R").small().color(egui::Color32::GRAY));
                        ui.label(egui::RichText::new("rotate").small().color(egui::Color32::DARK_GRAY));
                    });
                });
        });
}
