pub mod grid;

use std::collections::HashMap;
use std::path::Path;

use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, StandardMaterial};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy_editor_game::{BaseMaterialProps, MaterialDefinition, MaterialLibrary, MaterialRef};
use bevy_egui::egui;
use serde::{de::DeserializeOwned, Serialize};

/// Type alias for the extended grid material
pub type GridMat = ExtendedMaterial<StandardMaterial, bevy_grid_shader::GridMaterial>;

/// Load a texture with repeat wrapping enabled.
///
/// `is_srgb` should be `true` for color textures (base color, emissive) and
/// `false` for data textures (normal maps, metallic/roughness, occlusion).
fn load_texture_repeat(asset_server: &AssetServer, path: String, is_srgb: bool) -> Handle<Image> {
    asset_server.load_with_settings(path, move |settings: &mut ImageLoaderSettings| {
        settings.is_srgb = is_srgb;
        settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            address_mode_w: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::linear()
        });
    })
}

/// Load texture images from asset paths in `BaseMaterialProps` and set them on a `StandardMaterial`.
pub fn load_base_textures(
    mat: &mut StandardMaterial,
    props: &BaseMaterialProps,
    asset_server: &AssetServer,
) {
    if let Some(ref path) = props.base_color_texture {
        mat.base_color_texture = Some(load_texture_repeat(asset_server, path.clone(), true));
    }
    if let Some(ref path) = props.normal_map_texture {
        mat.normal_map_texture = Some(load_texture_repeat(asset_server, path.clone(), false));
    }
    if let Some(ref path) = props.metallic_roughness_texture {
        mat.metallic_roughness_texture = Some(load_texture_repeat(asset_server, path.clone(), false));
    }
    if let Some(ref path) = props.emissive_texture {
        mat.emissive_texture = Some(load_texture_repeat(asset_server, path.clone(), true));
    }
    if let Some(ref path) = props.occlusion_texture {
        mat.occlusion_texture = Some(load_texture_repeat(asset_server, path.clone(), false));
    }
    if let Some(ref path) = props.depth_map_texture {
        mat.depth_map = Some(load_texture_repeat(asset_server, path.clone(), false));
    }
}

/// Trait for game/editor material extension definitions.
///
/// Implement this to register a custom shader material that can be selected
/// in the material editor and serialized to scene files.
pub trait EditorMaterialDef: Send + Sync + 'static {
    /// Serializable properties for the extension (stored as RON in MaterialExtensionData)
    type Props: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static;
    /// The Bevy MaterialExtension type
    type Extension: MaterialExtension + Asset + AsBindGroup + Clone + Send + Sync + 'static;

    /// Unique type name used in serialization (e.g. "grid", "checkerboard")
    const TYPE_NAME: &'static str;
    /// Display name in the material editor UI
    const DISPLAY_NAME: &'static str;

    /// Convert serializable props to the GPU extension
    fn to_extension(props: &Self::Props) -> Self::Extension;
    /// Convert GPU extension back to serializable props
    fn from_extension(ext: &Self::Extension) -> Self::Props;
    /// Draw the extension-specific UI fields. Returns true if any value changed.
    fn draw_ui(ui: &mut egui::Ui, props: &mut Self::Props) -> bool;
}

/// Type-erased entry for a registered material type.
pub struct MaterialTypeEntry {
    pub type_name: &'static str,
    pub display_name: &'static str,
    /// Apply a material to an entity from base props + optional RON extension data.
    /// Inserts the appropriate MeshMaterial3d component.
    pub apply: fn(&mut World, Entity, &BaseMaterialProps, Option<&str>),
    /// Remove the material component from an entity.
    pub remove: fn(&mut World, Entity),
    /// Read the base material properties from an entity's current material.
    pub read_base: fn(&World, Entity) -> Option<BaseMaterialProps>,
    /// Read the extension properties as RON string from the entity.
    pub read_extension: fn(&World, Entity) -> Option<String>,
    /// Draw extension-specific UI. Input: RON data string. Output: (changed, new_ron_data).
    pub draw_extension_ui: fn(&mut egui::Ui, &str) -> (bool, String),
    /// Default extension data as RON, or None for standard material
    pub default_extension_data: Option<String>,
}

/// Registry of all material types available in the editor.
#[derive(Resource, Default)]
pub struct MaterialTypeRegistry {
    pub types: Vec<MaterialTypeEntry>,
}

impl MaterialTypeRegistry {
    /// Find an entry by type_name
    pub fn find(&self, type_name: &str) -> Option<&MaterialTypeEntry> {
        self.types.iter().find(|e| e.type_name == type_name)
    }
}

// ---------------------------------------------------------------------------
// Standard material (no extension) — built-in first entry
// ---------------------------------------------------------------------------

fn apply_standard(world: &mut World, entity: Entity, base: &BaseMaterialProps, _ext: Option<&str>) {
    // Remove any extended material components that might be present
    // (We can't enumerate all extension types, but the caller should handle removal)
    let mut mat = base.to_standard_material();
    let asset_server = world.resource::<AssetServer>().clone();
    load_base_textures(&mut mat, base, &asset_server);
    let handle = world.resource_mut::<Assets<StandardMaterial>>().add(mat);
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(MeshMaterial3d(handle));
    }
}

fn remove_standard(world: &mut World, entity: Entity) {
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.remove::<MeshMaterial3d<StandardMaterial>>();
    }
}

fn read_base_standard(world: &World, entity: Entity) -> Option<BaseMaterialProps> {
    let mat_handle = world.get::<MeshMaterial3d<StandardMaterial>>(entity)?;
    let assets = world.resource::<Assets<StandardMaterial>>();
    let mat = assets.get(&mat_handle.0)?;
    Some(BaseMaterialProps::from_standard_material(mat))
}

fn read_extension_standard(_world: &World, _entity: Entity) -> Option<String> {
    None
}

fn draw_extension_ui_standard(_ui: &mut egui::Ui, data: &str) -> (bool, String) {
    (false, data.to_string())
}

fn standard_entry() -> MaterialTypeEntry {
    MaterialTypeEntry {
        type_name: "standard",
        display_name: "Standard",
        apply: apply_standard,
        remove: remove_standard,
        read_base: read_base_standard,
        read_extension: read_extension_standard,
        draw_extension_ui: draw_extension_ui_standard,
        default_extension_data: None,
    }
}

// ---------------------------------------------------------------------------
// Monomorphized functions for EditorMaterialDef implementors
// ---------------------------------------------------------------------------

fn apply_material<D: EditorMaterialDef>(
    world: &mut World,
    entity: Entity,
    base: &BaseMaterialProps,
    ext_data: Option<&str>,
) {
    let props: D::Props = ext_data
        .and_then(|s| ron::from_str(s).ok())
        .unwrap_or_default();
    let extension = D::to_extension(&props);
    let mut base_mat = base.to_standard_material();
    let asset_server = world.resource::<AssetServer>().clone();
    load_base_textures(&mut base_mat, base, &asset_server);
    let extended = ExtendedMaterial {
        base: base_mat,
        extension,
    };
    let handle = world
        .resource_mut::<Assets<ExtendedMaterial<StandardMaterial, D::Extension>>>()
        .add(extended);
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(MeshMaterial3d(handle));
    }
}

fn remove_material<D: EditorMaterialDef>(world: &mut World, entity: Entity) {
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.remove::<MeshMaterial3d<ExtendedMaterial<StandardMaterial, D::Extension>>>();
    }
}

fn read_base_ext<D: EditorMaterialDef>(world: &World, entity: Entity) -> Option<BaseMaterialProps> {
    let mat_handle =
        world.get::<MeshMaterial3d<ExtendedMaterial<StandardMaterial, D::Extension>>>(entity)?;
    let assets = world.resource::<Assets<ExtendedMaterial<StandardMaterial, D::Extension>>>();
    let mat = assets.get(&mat_handle.0)?;
    Some(BaseMaterialProps::from_standard_material(&mat.base))
}

fn read_extension_ext<D: EditorMaterialDef>(world: &World, entity: Entity) -> Option<String> {
    let mat_handle =
        world.get::<MeshMaterial3d<ExtendedMaterial<StandardMaterial, D::Extension>>>(entity)?;
    let assets = world.resource::<Assets<ExtendedMaterial<StandardMaterial, D::Extension>>>();
    let mat = assets.get(&mat_handle.0)?;
    let props = D::from_extension(&mat.extension);
    ron::to_string(&props).ok()
}

fn draw_extension_ui_ext<D: EditorMaterialDef>(
    ui: &mut egui::Ui,
    data: &str,
) -> (bool, String) {
    let mut props: D::Props = ron::from_str(data).unwrap_or_default();
    let changed = D::draw_ui(ui, &mut props);
    let new_data = ron::to_string(&props).unwrap_or_default();
    (changed, new_data)
}

fn entry_for<D: EditorMaterialDef>() -> MaterialTypeEntry {
    let default_props = D::Props::default();
    let default_data = ron::to_string(&default_props).unwrap_or_default();
    MaterialTypeEntry {
        type_name: D::TYPE_NAME,
        display_name: D::DISPLAY_NAME,
        apply: apply_material::<D>,
        remove: remove_material::<D>,
        read_base: read_base_ext::<D>,
        read_extension: read_extension_ext::<D>,
        draw_extension_ui: draw_extension_ui_ext::<D>,
        default_extension_data: Some(default_data),
    }
}

// ---------------------------------------------------------------------------
// App extension trait
// ---------------------------------------------------------------------------

/// Extension trait for registering material types with the editor.
///
/// Note: callers must ensure the corresponding `MaterialPlugin` for their
/// extension type is already registered (e.g. via `add_plugins(GridMaterialPlugin)`).
/// The registry only stores the type-erased function pointers; it does not
/// register rendering plugins.
pub trait RegisterMaterialTypeExt {
    fn register_material_type<D: EditorMaterialDef>(&mut self) -> &mut Self;
}

impl RegisterMaterialTypeExt for App {
    fn register_material_type<D: EditorMaterialDef>(&mut self) -> &mut Self {
        // Add entry to registry
        let mut registry = self
            .world_mut()
            .get_resource_or_insert_with(MaterialTypeRegistry::default);
        registry.types.push(entry_for::<D>());
        self
    }
}

// ---------------------------------------------------------------------------
// Helper: resolve a MaterialRef to a MaterialDefinition
// ---------------------------------------------------------------------------

/// Resolve a `MaterialRef` into a concrete `MaterialDefinition`.
///
/// For `Library` refs, looks up the name in the `MaterialLibrary`.
/// Returns `None` if a library reference can't be found.
pub fn resolve_material_ref<'a>(
    material_ref: &'a MaterialRef,
    library: &'a MaterialLibrary,
) -> Option<&'a MaterialDefinition> {
    match material_ref {
        MaterialRef::Inline(def) => Some(def),
        MaterialRef::Library(name) => library.materials.get(name.as_str()),
    }
}

// ---------------------------------------------------------------------------
// Helper: apply a MaterialDefinition to an entity using the registry
// ---------------------------------------------------------------------------

/// Apply a resolved `MaterialDefinition` to an entity.
///
/// Looks up the `MaterialTypeRegistry` from the world, extracts the needed
/// function pointer, then applies the material. This two-step approach avoids
/// holding an immutable borrow of `World` (via the registry) while also
/// needing a mutable borrow (for asset insertion).
pub fn apply_material_def(
    world: &mut World,
    entity: Entity,
    def: &MaterialDefinition,
    _registry: &MaterialTypeRegistry,
) {
    apply_material_def_standalone(world, entity, def);
}

/// Apply a material definition to an entity without requiring a pre-borrowed registry.
pub fn apply_material_def_standalone(
    world: &mut World,
    entity: Entity,
    def: &MaterialDefinition,
) {
    match &def.extension {
        None => {
            apply_standard(world, entity, &def.base, None);
        }
        Some(ext) => {
            // Extract the apply function pointer from registry (short immutable borrow)
            let apply_fn = world
                .get_resource::<MaterialTypeRegistry>()
                .and_then(|r| r.find(&ext.type_name))
                .map(|e| e.apply);

            if let Some(apply_fn) = apply_fn {
                apply_fn(world, entity, &def.base, Some(&ext.data));
            } else {
                warn!(
                    "Unknown material extension type '{}', falling back to standard",
                    ext.type_name
                );
                apply_standard(world, entity, &def.base, None);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Default library presets
// ---------------------------------------------------------------------------

/// Populate the material library with default presets for each primitive shape and blockout type.
pub fn populate_default_library(library: &mut MaterialLibrary) {
    use crate::constants::primitive_colors;
    use crate::scene::blockout::blockout_colors;
    use crate::scene::PrimitiveShape;

    let grid_default_data = ron::to_string(&grid::GridMaterialProps::default()).unwrap_or_default();

    // Primitive shapes with grid extension
    for shape in [
        PrimitiveShape::Cube,
        PrimitiveShape::Sphere,
        PrimitiveShape::Cylinder,
        PrimitiveShape::Capsule,
        PrimitiveShape::Plane,
    ] {
        let name = format!("{} Default", shape.display_name());
        let color = primitive_colors::for_shape(shape);
        library.materials.entry(name).or_insert_with(|| {
            MaterialDefinition::with_extension(
                BaseMaterialProps {
                    base_color: color,
                    ..default()
                },
                "grid",
                grid_default_data.clone(),
            )
        });
    }

    // Blockout shapes with grid extension
    for (name, color) in [
        ("Stairs Default", blockout_colors::STAIRS),
        ("Ramp Default", blockout_colors::RAMP),
        ("Arch Default", blockout_colors::ARCH),
        ("L-Shape Default", blockout_colors::LSHAPE),
    ] {
        library.materials.entry(name.to_string()).or_insert_with(|| {
            MaterialDefinition::with_extension(
                BaseMaterialProps {
                    base_color: color,
                    ..default()
                },
                "grid",
                grid_default_data.clone(),
            )
        });
    }
}

// ---------------------------------------------------------------------------
// Helper: remove all possible material components from an entity
// ---------------------------------------------------------------------------

/// Remove all possible material components from an entity.
pub fn remove_all_material_components(world: &mut World, entity: Entity) {
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.remove::<MeshMaterial3d<StandardMaterial>>();
        e.remove::<MeshMaterial3d<GridMat>>();
    }
}

// ---------------------------------------------------------------------------
// Disk persistence for material presets
// ---------------------------------------------------------------------------

const MATERIALS_DIR: &str = "assets/materials";

/// Sanitize a preset name for use as a filename (replace filesystem-invalid chars).
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

/// Save a single material preset to disk as a RON file.
fn save_preset_to_disk(name: &str, def: &MaterialDefinition) {
    let dir = Path::new(MATERIALS_DIR);
    if let Err(e) = std::fs::create_dir_all(dir) {
        warn!("Failed to create materials directory: {}", e);
        return;
    }

    let filename = sanitize_filename(name);
    let path = dir.join(format!("{}.mat.ron", filename));

    let pretty = ron::ser::PrettyConfig::default();
    match ron::ser::to_string_pretty(def, pretty) {
        Ok(ron_str) => {
            if let Err(e) = std::fs::write(&path, &ron_str) {
                warn!("Failed to write material preset '{}': {}", name, e);
            }
        }
        Err(e) => {
            warn!("Failed to serialize material preset '{}': {}", name, e);
        }
    }
}

/// Load all material presets from `assets/materials/*.mat.ron` into the library.
/// Disk presets override any existing entry with the same name.
fn load_presets_from_disk(library: &mut MaterialLibrary) {
    let dir = Path::new(MATERIALS_DIR);
    if !dir.is_dir() {
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let fname = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if !fname.ends_with(".mat.ron") {
            continue;
        }

        let name = fname.trim_end_matches(".mat.ron").to_string();
        if name.is_empty() {
            continue;
        }

        let Ok(contents) = std::fs::read_to_string(&path) else {
            warn!("Failed to read material preset file: {:?}", path);
            continue;
        };

        match ron::from_str::<MaterialDefinition>(&contents) {
            Ok(def) => {
                library.materials.insert(name.clone(), def);
                info!("Loaded material preset '{}' from disk", name);
            }
            Err(e) => {
                warn!("Failed to parse material preset '{:?}': {}", path, e);
            }
        }
    }
}

/// System that auto-saves material presets when the library changes.
fn auto_save_presets(
    library: Res<MaterialLibrary>,
    mut prev_state: Local<HashMap<String, String>>,
) {
    if !library.is_changed() {
        return;
    }

    for (name, def) in &library.materials {
        let ron_str = ron::to_string(def).unwrap_or_default();
        let changed = match prev_state.get(name) {
            Some(prev) => prev != &ron_str,
            None => true,
        };
        if changed {
            save_preset_to_disk(name, def);
            prev_state.insert(name.clone(), ron_str);
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct MaterialsPlugin;

impl Plugin for MaterialsPlugin {
    fn build(&self, app: &mut App) {
        // Init registry with standard as the first entry
        let mut registry = MaterialTypeRegistry::default();
        registry.types.push(standard_entry());
        app.insert_resource(registry);

        // Init empty library (populated after all game plugins register)
        app.init_resource::<MaterialLibrary>();

        // Register grid material type (built-in)
        app.register_material_type::<grid::GridMaterialDef>();

        // Populate default library at PreStartup, then load disk overrides
        app.add_systems(PreStartup, init_default_library);

        // Auto-save presets when library changes
        app.add_systems(Update, auto_save_presets);
    }
}

fn init_default_library(mut library: ResMut<MaterialLibrary>) {
    populate_default_library(&mut library);
    load_presets_from_disk(&mut library);
}
