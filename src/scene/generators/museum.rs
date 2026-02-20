use std::path::Path;

use avian3d::prelude::*;
use bevy::core_pipeline::Skybox;
use bevy::prelude::*;

use bevy_editor_game::{MaterialDefinition, MaterialLibrary, MaterialRef};

use super::{MuseumGrid, MuseumGridSection};
use crate::editor::EditorCamera;
use bevy_vfx::VfxLibrary;
use crate::scene::gltf_source::GltfSource;
use crate::scene::primitives::PrimitiveMarker;
use crate::scene::{regenerate_runtime_components, DirectionalLightMarker, PrimitiveShape, SceneEntity};

const COLUMNS: usize = 6;
const SPACING: f32 = 4.0;
const ITEM_Y: f32 = 0.5;
/// Z gap between sections (in world units).
const SECTION_GAP: f32 = 3.0;

/// An item to be placed in a museum grid section.
struct MuseumItem {
    display_name: String,
    kind: MuseumItemKind,
}

enum MuseumItemKind {
    Primitive(PrimitiveShape),
    Gltf { relative_path: String },
    /// A cube displaying a named material preset.
    MaterialPreset { preset_name: String },
    /// A particle effect from a named preset.
    ParticlePreset { preset_name: String },
}

/// A logical section of museum items.
struct MuseumSection {
    title: String,
    items: Vec<MuseumItem>,
}

/// Generate the museum scene: multiple grid sections with floor, skybox, and lighting.
pub fn generate_museum(world: &mut World) {
    let sections = collect_sections(world);

    let mut grid = MuseumGrid::default();
    let mut z_cursor: f32 = 0.0;
    let mut total_items = 0;

    for section in &sections {
        let count = section.items.len();
        if count == 0 {
            continue;
        }
        let rows = (count + COLUMNS - 1) / COLUMNS;
        let cols = COLUMNS.min(count);

        let origin = Vec3::new(0.0, 0.0, z_cursor);

        // Spawn items in this section
        for (index, item) in section.items.iter().enumerate() {
            let row = index / COLUMNS;
            let col = index % COLUMNS;
            let grid_ref = format!("{}{:03}", (b'A' + row as u8) as char, col + 1);
            let name = format!("{} [{}]", item.display_name, grid_ref);
            let position = origin + Vec3::new(col as f32 * SPACING, ITEM_Y, row as f32 * SPACING);

            match &item.kind {
                MuseumItemKind::Primitive(shape) => {
                    spawn_museum_primitive(world, *shape, position, &name);
                }
                MuseumItemKind::Gltf { relative_path } => {
                    spawn_museum_gltf(world, relative_path, position, &name);
                }
                MuseumItemKind::MaterialPreset { preset_name } => {
                    spawn_museum_material_cube(world, preset_name, position, &name);
                }
                MuseumItemKind::ParticlePreset { preset_name } => {
                    spawn_museum_particle(world, preset_name, position, &name);
                }
            }
        }

        grid.sections.push(MuseumGridSection {
            title: section.title.clone(),
            rows,
            cols,
            spacing: SPACING,
            origin,
        });

        total_items += count;
        // Advance cursor past this section's rows + gap
        z_cursor += rows as f32 * SPACING + SECTION_GAP;
    }

    world.insert_resource(grid);

    // Regenerate runtime components for material cubes (spawned without Mesh3d)
    regenerate_runtime_components(world);

    // Spawn ground floor encompassing all sections
    spawn_ground(world);

    // Add skybox + environment map to editor camera
    add_skybox(world);

    // Spawn directional light fill
    spawn_directional_light(world);

    let section_count = sections.iter().filter(|s| !s.items.is_empty()).count();
    info!(
        "Museum generated: {} items in {} sections",
        total_items, section_count
    );
}

fn collect_sections(world: &World) -> Vec<MuseumSection> {
    let mut sections = Vec::new();

    // 1. Primitives section
    let mut primitives = Vec::new();
    for shape in [
        PrimitiveShape::Cube,
        PrimitiveShape::Sphere,
        PrimitiveShape::Cylinder,
        PrimitiveShape::Capsule,
        PrimitiveShape::Plane,
    ] {
        primitives.push(MuseumItem {
            display_name: shape.display_name().to_string(),
            kind: MuseumItemKind::Primitive(shape),
        });
    }
    sections.push(MuseumSection {
        title: "Primitives".to_string(),
        items: primitives,
    });

    // 2. Object sections from assets/objects/
    let objects_dir = Path::new("assets/objects");
    if objects_dir.is_dir() {
        // Root-level files → "Objects" section
        let mut root_items = Vec::new();
        scan_gltf_dir(objects_dir, objects_dir, &mut root_items);

        if !root_items.is_empty() {
            sections.push(MuseumSection {
                title: "Objects".to_string(),
                items: root_items,
            });
        }

        // Subdirectory → named section
        if let Ok(entries) = std::fs::read_dir(objects_dir) {
            let mut subdirs: Vec<_> = entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .collect();
            subdirs.sort_by_key(|e| e.file_name());

            for entry in subdirs {
                let dir_path = entry.path();
                let dir_name = entry
                    .file_name()
                    .to_string_lossy()
                    .to_string();
                let mut dir_items = Vec::new();
                scan_gltf_recursive(objects_dir, &dir_path, &mut dir_items);
                if !dir_items.is_empty() {
                    sections.push(MuseumSection {
                        title: dir_name,
                        items: dir_items,
                    });
                }
            }
        }
    }

    // 3. Particle presets section — one emitter per library preset
    if let Some(library) = world.get_resource::<VfxLibrary>() {
        let mut particle_items: Vec<MuseumItem> = library
            .effects
            .keys()
            .map(|name| MuseumItem {
                display_name: name.clone(),
                kind: MuseumItemKind::ParticlePreset {
                    preset_name: name.clone(),
                },
            })
            .collect();
        particle_items.sort_by(|a, b| a.display_name.cmp(&b.display_name));

        if !particle_items.is_empty() {
            sections.push(MuseumSection {
                title: "Particle Effects".to_string(),
                items: particle_items,
            });
        }
    }

    // 4. Materials section — one cube per library preset
    if let Some(library) = world.get_resource::<MaterialLibrary>() {
        let mut mat_items: Vec<MuseumItem> = library
            .materials
            .keys()
            .map(|name| MuseumItem {
                display_name: name.clone(),
                kind: MuseumItemKind::MaterialPreset {
                    preset_name: name.clone(),
                },
            })
            .collect();
        mat_items.sort_by(|a, b| a.display_name.cmp(&b.display_name));

        if !mat_items.is_empty() {
            sections.push(MuseumSection {
                title: "Materials".to_string(),
                items: mat_items,
            });
        }
    }

    sections
}

/// Scan only direct children (non-recursive) of a directory for GLTF/GLB files.
fn scan_gltf_dir(base: &Path, dir: &Path, items: &mut Vec<MuseumItem>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let mut paths: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    paths.sort();

    for path in paths {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_ascii_lowercase();
            if ext_lower == "gltf" || ext_lower == "glb" {
                let relative = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                let display = path
                    .file_stem()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| relative.clone());

                items.push(MuseumItem {
                    display_name: display,
                    kind: MuseumItemKind::Gltf {
                        relative_path: format!("objects/{}", relative),
                    },
                });
            }
        }
    }
}

fn scan_gltf_recursive(base: &Path, dir: &Path, items: &mut Vec<MuseumItem>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();

    for path in paths {
        if path.is_dir() {
            scan_gltf_recursive(base, &path, items);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_ascii_lowercase();
            if ext_lower == "gltf" || ext_lower == "glb" {
                let relative = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                let display = path
                    .file_stem()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| relative.clone());

                items.push(MuseumItem {
                    display_name: display,
                    kind: MuseumItemKind::Gltf {
                        relative_path: format!("objects/{}", relative),
                    },
                });
            }
        }
    }
}

fn spawn_museum_primitive(world: &mut World, shape: PrimitiveShape, position: Vec3, name: &str) {
    let color = shape.default_color();
    let mesh = world.resource_mut::<Assets<Mesh>>().add(shape.create_mesh());
    let material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(shape.create_material());

    world.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        PrimitiveMarker { shape },
        MaterialRef::Inline(MaterialDefinition::standard(color)),
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(position),
        RigidBody::Static,
        shape.create_collider(),
    ));
}

fn spawn_museum_gltf(world: &mut World, relative_path: &str, position: Vec3, name: &str) {
    world.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        GltfSource {
            path: relative_path.to_string(),
            scene_index: 0,
        },
        Transform::from_translation(position),
        RigidBody::Static,
    ));
}

/// Spawn a cube that uses a named material preset from the library.
/// Spawned without Mesh3d/MeshMaterial3d — `regenerate_runtime_components` fills those in.
fn spawn_museum_material_cube(
    world: &mut World,
    preset_name: &str,
    position: Vec3,
    name: &str,
) {
    world.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        PrimitiveMarker {
            shape: PrimitiveShape::Cube,
        },
        MaterialRef::Library(preset_name.to_string()),
        Transform::from_translation(position),
        RigidBody::Static,
    ));
}

/// Spawn a particle effect entity from a named preset.
fn spawn_museum_particle(
    world: &mut World,
    preset_name: &str,
    position: Vec3,
    name: &str,
) {
    let system = world
        .get_resource::<VfxLibrary>()
        .and_then(|lib| lib.effects.get(preset_name).cloned())
        .unwrap_or_default();

    world.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        system,
        Transform::from_translation(position),
        Visibility::default(),
        Collider::sphere(crate::constants::physics::LIGHT_COLLIDER_RADIUS),
    ));
}

/// Spawn a ground plane encompassing all museum grid sections.
fn spawn_ground(world: &mut World) {
    let grid = world.resource::<MuseumGrid>();
    if grid.sections.is_empty() {
        return;
    }

    // Compute bounding box across all sections
    let mut x_min = f32::MAX;
    let mut x_max = f32::MIN;
    let mut z_min = f32::MAX;
    let mut z_max = f32::MIN;

    for section in &grid.sections {
        let half = section.spacing * 0.5;
        let sx_min = section.origin.x - half;
        let sx_max = section.origin.x + (section.cols as f32 - 1.0) * section.spacing + half;
        let sz_min = section.origin.z - half;
        let sz_max = section.origin.z + (section.rows as f32 - 1.0) * section.spacing + half;
        x_min = x_min.min(sx_min);
        x_max = x_max.max(sx_max);
        z_min = z_min.min(sz_min);
        z_max = z_max.max(sz_max);
    }

    // Add margin
    let margin = SPACING;
    x_min -= margin;
    x_max += margin;
    z_min -= margin;
    z_max += margin;

    let width = x_max - x_min;
    let depth = z_max - z_min;
    let center_x = (x_min + x_max) / 2.0;
    let center_z = (z_min + z_max) / 2.0;

    let ground_color = Color::srgb(0.35, 0.35, 0.4);
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::new(1.0, 1.0, 1.0));
    let material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: ground_color,
            ..default()
        });

    world.spawn((
        SceneEntity,
        Name::new("Ground"),
        PrimitiveMarker {
            shape: PrimitiveShape::Cube,
        },
        MaterialRef::Inline(MaterialDefinition::standard(ground_color)),
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(Vec3::new(center_x, -0.5, center_z))
            .with_scale(Vec3::new(width, 1.0, depth)),
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),
    ));
}

fn add_skybox(world: &mut World) {
    let asset_server = world.resource::<AssetServer>().clone();
    let cubemap = asset_server.load("skybox/citrus_orchard_road_puresky_4k_cubemap.ktx2");
    let diffuse = asset_server.load("skybox/citrus_orchard_road_puresky_4k_diffuse.ktx2");
    let specular = asset_server.load("skybox/citrus_orchard_road_puresky_4k_specular.ktx2");

    let camera_entity: Option<Entity> = {
        let mut query = world.query_filtered::<Entity, With<EditorCamera>>();
        query.iter(world).next()
    };

    if let Some(entity) = camera_entity {
        if let Ok(mut e) = world.get_entity_mut(entity) {
            e.insert((
                Skybox {
                    image: cubemap,
                    brightness: 1000.0,
                    rotation: Quat::IDENTITY,
                },
                EnvironmentMapLight {
                    diffuse_map: diffuse,
                    specular_map: specular,
                    intensity: 900.0,
                    rotation: Quat::IDENTITY,
                    affects_lightmapped_mesh_diffuse: true,
                },
            ));
        }
    }
}

fn spawn_directional_light(world: &mut World) {
    let marker = DirectionalLightMarker {
        color: Color::WHITE,
        illuminance: 1500.0,
        shadows_enabled: true,
    };

    world.spawn((
        SceneEntity,
        Name::new("Museum Sun"),
        marker.clone(),
        DirectionalLight {
            color: marker.color,
            illuminance: marker.illuminance,
            shadows_enabled: marker.shadows_enabled,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.7, 0.4, 0.0)),
        Visibility::default(),
        Collider::sphere(crate::constants::physics::LIGHT_COLLIDER_RADIUS),
    ));
}
