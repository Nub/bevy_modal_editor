//! Material assets + library (M3-C6, spec §5/§7): a material is a VERSIONED asset
//! — `materials.ron` carries the same envelope discipline as scenes (format
//! version, atomic temp+rename save with a `.bak`, forward-compat via serde
//! defaults). Scenes reference materials by asset id only (`MaterialRef`), so a
//! scene file never embeds material data and survives library edits.
//!
//! Assignment is an ordinary `Set` transaction (one undo entry). Library PARAM
//! edits save immediately but are not yet undoable — the edit history is scoped
//! to scene state; asset-history is an M4 concern (noted at the gate).

use bevy::prelude::*;
use editor_core::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// 3: materials can INHERIT (`base` + `overridden`). A format-2 reader would
/// silently drop the wiring and flatten an instance to its own sparse values,
/// which is why this is a version bump and not just two more fields.
/// 2: textures moved from a single `base_color_texture` field to a slot table
/// carrying colour space, plus uv tiling/offset. Format-1 files load and
/// migrate on read (`MaterialDef::migrate`).
pub const MATERIALS_FORMAT_VERSION: u32 = 3;

/// Scene-side reference: which library material shades this entity. Serialized
/// with the scene BY ID — never by value.
#[derive(Component, Reflect, Clone, Copy, PartialEq, Debug, Default)]
#[reflect(Component)]
pub struct MaterialRef(pub Uuid);

/// How alpha is interpreted (mirrors `bevy::pbr::AlphaMode`'s designer-facing
/// subset). String-tagged in RON, serde-default Opaque — old files load clean.
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
pub enum MaterialAlphaMode {
    #[default]
    Opaque,
    Blend,
    /// Cutout: alpha below `alpha_cutoff` discards the fragment.
    Mask,
}

/// Build a primitive mesh WITH tangents. Bevy compiles the whole normal-mapping
/// branch out unless the vertex layout carries `ATTRIBUTE_TANGENT` (the shader
/// gates it behind `#ifdef VERTEX_TANGENTS`), and no primitive builder emits
/// one — so a normal map assigned to a cube or a sphere is silently discarded,
/// with no warning and no error. glTF meshes arrive with tangents from the
/// importer; everything the editor generates has to ask for them.
pub fn primitive_mesh(shape: impl Into<Mesh>) -> Mesh {
    let mut mesh = shape.into();
    if let Err(error) = mesh.generate_tangents() {
        // Not fatal: the surface still shades, it just cannot show a normal map.
        warn!("no tangents for a primitive mesh; normal maps will not show: {error}");
    }
    mesh
}

/// Which map a texture fills. The slot is DECLARED rather than implied by a
/// field name, because the one thing a texture pipeline must not get wrong is
/// colour space, and the slot is what determines it: a normal map or a
/// metallic-roughness map holds vectors and scalars, and gamma-correcting them
/// on load corrupts every value. Sampling every texture as sRGB — which is
/// what the single-slot version did — is silently wrong for three of these five.
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum TextureSlot {
    BaseColor,
    Normal,
    /// glTF/ORM convention: green carries roughness, blue carries metallic.
    MetallicRoughness,
    Occlusion,
    Emissive,
}

impl TextureSlot {
    /// Every slot, in the order the panel shows them.
    pub const ALL: [TextureSlot; 5] = [
        TextureSlot::BaseColor,
        TextureSlot::Normal,
        TextureSlot::MetallicRoughness,
        TextureSlot::Occlusion,
        TextureSlot::Emissive,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TextureSlot::BaseColor => "base color",
            TextureSlot::Normal => "normal",
            TextureSlot::MetallicRoughness => "metal/rough",
            TextureSlot::Occlusion => "occlusion",
            TextureSlot::Emissive => "emissive",
        }
    }

    /// COLOUR is sRGB-encoded; DATA is linear. This is the whole reason the
    /// slot table exists.
    pub fn is_srgb(self) -> bool {
        matches!(self, TextureSlot::BaseColor | TextureSlot::Emissive)
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct MaterialDef {
    pub id: Uuid,
    pub name: String,
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    /// Linear-space emissive color, scaled by `emissive_intensity`.
    pub emissive: [f32; 3],
    pub emissive_intensity: f32,
    pub alpha_mode: MaterialAlphaMode,
    pub alpha_cutoff: f32,
    pub unlit: bool,
    pub double_sided: bool,
    /// LEGACY, format 1: the only texture slot there used to be. Kept so old
    /// `materials.ron` files still load; `migrate` folds it into `textures` and
    /// clears it, so nothing downstream has to know it existed.
    pub base_color_texture: Option<Uuid>,
    /// Imported textures (identity pipeline uuids) by slot. A map rather than a
    /// field per map: the panel renders one row per declared slot, so a new
    /// slot costs one enum variant and nothing else.
    pub textures: std::collections::BTreeMap<TextureSlot, Uuid>,
    /// How many times the texture repeats across the surface. A wall kit is
    /// unusable without it — one stretched copy per piece is not a wall.
    pub uv_tiling: [f32; 2],
    pub uv_offset: [f32; 2],
    /// Inherit from another library material: every field NOT listed in
    /// `overridden` resolves from it, live. Spec §6 mandates keeping BOTH
    /// library-reference and inline-override semantics, and only the reference
    /// half existed — every wall colour was a full copy with no link back, so a
    /// late art-direction change meant re-editing each one by hand.
    pub base: Option<Uuid>,
    /// Which fields this material owns outright. A closed set rather than
    /// reflect paths: a material's fields are known at compile time, so the
    /// compiler can check the resolution is exhaustive. The concept shared with
    /// prefab overrides is "a base plus the fields you took ownership of" — the
    /// semantics, not the encoding.
    pub overridden: std::collections::BTreeSet<MaterialField>,
}

/// One inheritable field of a `MaterialDef`. `id` and `name` are absent on
/// purpose: identity is never inherited.
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum MaterialField {
    BaseColor,
    Metallic,
    Roughness,
    Emissive,
    EmissiveIntensity,
    AlphaMode,
    AlphaCutoff,
    Unlit,
    DoubleSided,
    Textures,
    UvTiling,
    UvOffset,
}

impl MaterialField {
    pub const ALL: [MaterialField; 12] = [
        MaterialField::BaseColor,
        MaterialField::Metallic,
        MaterialField::Roughness,
        MaterialField::Emissive,
        MaterialField::EmissiveIntensity,
        MaterialField::AlphaMode,
        MaterialField::AlphaCutoff,
        MaterialField::Unlit,
        MaterialField::DoubleSided,
        MaterialField::Textures,
        MaterialField::UvTiling,
        MaterialField::UvOffset,
    ];
}

impl MaterialDef {
    /// This material's own values laid OVER `base`. Identity (id, name, and the
    /// inheritance wiring itself) always stays this material's own.
    fn over(&self, base: &MaterialDef) -> MaterialDef {
        let mut out = base.clone();
        out.id = self.id;
        out.name = self.name.clone();
        out.base = self.base;
        out.overridden = self.overridden.clone();
        for field in &self.overridden {
            match field {
                MaterialField::BaseColor => out.base_color = self.base_color,
                MaterialField::Metallic => out.metallic = self.metallic,
                MaterialField::Roughness => out.roughness = self.roughness,
                MaterialField::Emissive => out.emissive = self.emissive,
                MaterialField::EmissiveIntensity => {
                    out.emissive_intensity = self.emissive_intensity
                }
                MaterialField::AlphaMode => out.alpha_mode = self.alpha_mode,
                MaterialField::AlphaCutoff => out.alpha_cutoff = self.alpha_cutoff,
                MaterialField::Unlit => out.unlit = self.unlit,
                MaterialField::DoubleSided => out.double_sided = self.double_sided,
                MaterialField::Textures => out.textures = self.textures.clone(),
                MaterialField::UvTiling => out.uv_tiling = self.uv_tiling,
                MaterialField::UvOffset => out.uv_offset = self.uv_offset,
            }
        }
        out
    }
}

impl MaterialDef {
    /// Fold format-1 fields into their format-2 homes. Idempotent, so running
    /// it on an already-migrated def does nothing.
    fn migrate(&mut self) {
        if let Some(uuid) = self.base_color_texture.take() {
            self.textures.entry(TextureSlot::BaseColor).or_insert(uuid);
        }
    }

    pub fn texture(&self, slot: TextureSlot) -> Option<Uuid> {
        self.textures.get(&slot).copied()
    }

    pub fn set_texture(&mut self, slot: TextureSlot, uuid: Option<Uuid>) {
        match uuid {
            Some(uuid) => {
                self.textures.insert(slot, uuid);
            }
            None => {
                self.textures.remove(&slot);
            }
        }
    }
}

impl Default for MaterialDef {
    fn default() -> Self {
        Self {
            id: Uuid::nil(),
            name: "Material".into(),
            base_color: [0.8, 0.8, 0.8, 1.0],
            metallic: 0.0,
            roughness: 0.6,
            emissive: [0.0, 0.0, 0.0],
            emissive_intensity: 1.0,
            alpha_mode: MaterialAlphaMode::Opaque,
            alpha_cutoff: 0.5,
            unlit: false,
            double_sided: false,
            base_color_texture: None,
            textures: std::collections::BTreeMap::new(),
            uv_tiling: [1.0, 1.0],
            uv_offset: [0.0, 0.0],
            base: None,
            overridden: std::collections::BTreeSet::new(),
        }
    }
}

/// THE def → render-material conversion (game world and editor preview both
/// use this — one source of truth for how a `MaterialDef` looks). Textures
/// resolve through the identity pipeline (uuid → imported path), sampled
/// sRGB with repeat wrapping.
pub fn to_standard_material(
    def: &MaterialDef,
    models: &crate::models::ModelLibrary,
    assets: Option<&AssetServer>,
) -> bevy::pbr::StandardMaterial {
    // Each slot loads in ITS OWN colour space. `is_srgb` is not a sampling
    // preference — it decides whether the loader gamma-decodes the bytes, and
    // doing that to a normal or metallic-roughness map corrupts every value in
    // it. Repeat wrapping so `uv_tiling` has something to repeat.
    let load = |slot: TextureSlot| -> Option<Handle<Image>> {
        let entry = def.texture(slot).and_then(|uuid| models.get(&uuid))?;
        let assets = assets?;
        let srgb = slot.is_srgb();
        #[allow(deprecated)]
        Some(assets.load_with_settings(
            entry.asset_path.clone(),
            move |settings: &mut bevy::image::ImageLoaderSettings| {
                settings.is_srgb = srgb;
                settings.sampler =
                    bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
                        address_mode_u: bevy::image::ImageAddressMode::Repeat,
                        address_mode_v: bevy::image::ImageAddressMode::Repeat,
                        ..bevy::image::ImageSamplerDescriptor::linear()
                    });
            },
        ))
    };
    let base_color_texture = load(TextureSlot::BaseColor);
    bevy::pbr::StandardMaterial {
        base_color: Color::srgba(
            def.base_color[0],
            def.base_color[1],
            def.base_color[2],
            def.base_color[3],
        ),
        base_color_texture,
        metallic: def.metallic,
        perceptual_roughness: def.roughness.clamp(0.089, 1.0),
        emissive: LinearRgba::new(
            def.emissive[0] * def.emissive_intensity,
            def.emissive[1] * def.emissive_intensity,
            def.emissive[2] * def.emissive_intensity,
            1.0,
        ),
        alpha_mode: match def.alpha_mode {
            MaterialAlphaMode::Opaque => AlphaMode::Opaque,
            MaterialAlphaMode::Blend => AlphaMode::Blend,
            MaterialAlphaMode::Mask => AlphaMode::Mask(def.alpha_cutoff),
        },
        normal_map_texture: load(TextureSlot::Normal),
        metallic_roughness_texture: load(TextureSlot::MetallicRoughness),
        occlusion_texture: load(TextureSlot::Occlusion),
        emissive_texture: load(TextureSlot::Emissive),
        // One transform for every slot: an artist tiles a surface, not a map.
        uv_transform: bevy::math::Affine2::from_scale_angle_translation(
            Vec2::new(def.uv_tiling[0], def.uv_tiling[1]),
            0.0,
            Vec2::new(def.uv_offset[0], def.uv_offset[1]),
        ),
        unlit: def.unlit,
        double_sided: def.double_sided,
        cull_mode: if def.double_sided {
            None
        } else {
            Some(bevy::render::render_resource::Face::Back)
        },
        ..Default::default()
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct MaterialsEnvelope {
    format_version: u32,
    materials: Vec<MaterialDef>,
}

#[derive(Resource)]
pub struct MaterialLibrary {
    pub materials: Vec<MaterialDef>,
    pub path: PathBuf,
    /// Bumped on every library mutation — visual sync and saves key off this.
    pub generation: u64,
}

impl Default for MaterialLibrary {
    fn default() -> Self {
        Self {
            materials: Vec::new(),
            path: PathBuf::from("materials.ron"),
            generation: 0,
        }
    }
}

impl MaterialLibrary {
    pub fn get(&self, id: &Uuid) -> Option<&MaterialDef> {
        self.materials.iter().find(|m| &m.id == id)
    }
    /// The generation bump belongs to a lookup that SUCCEEDED. Bumping first
    /// meant every miss marked the library dirty, and the autosave rewrites
    /// materials.ron on any generation change — a steady stream of disk writes
    /// for edits that never happened (spec §8: no per-frame work at rest).
    pub fn get_mut(&mut self, id: &Uuid) -> Option<&mut MaterialDef> {
        let index = self.materials.iter().position(|m| &m.id == id)?;
        self.generation += 1;
        self.materials.get_mut(index)
    }
    pub fn add(&mut self, def: MaterialDef) {
        self.generation += 1;
        self.materials.push(def);
    }
    /// A material with its inheritance applied — THE thing to render, name a
    /// colour by, or preview. `get` returns what is STORED (own values plus the
    /// override set), which is what an editor edits; this returns what it looks
    /// like, which is what everything else wants.
    ///
    /// A cycle resolves to the deepest material's own values rather than
    /// looping: a base chain is a chain, and refusing to hang is worth more
    /// than refusing to render.
    pub fn resolved(&self, id: &Uuid) -> Option<MaterialDef> {
        let mut chain: Vec<&MaterialDef> = Vec::new();
        let mut seen: Vec<Uuid> = Vec::new();
        let mut current = self.get(id)?;
        loop {
            if seen.contains(&current.id) {
                break; // cycle: stop, keep what we have
            }
            seen.push(current.id);
            chain.push(current);
            let Some(base) = current.base.and_then(|base| self.get(&base)) else {
                break;
            };
            current = base;
        }
        // Deepest base first, each descendant laid over it.
        let mut resolved = chain.pop()?.clone();
        while let Some(child) = chain.pop() {
            resolved = child.over(&resolved);
        }
        Some(resolved)
    }

    /// Every material that inherits from `id`, directly.
    pub fn children_of(&self, id: &Uuid) -> Vec<Uuid> {
        self.materials
            .iter()
            .filter(|def| def.base == Some(*id))
            .map(|def| def.id)
            .collect()
    }

    /// Remove a material. The caller decides whether removing it is SAFE —
    /// see `material.delete`, which refuses while anything still references it.
    pub fn remove(&mut self, id: &Uuid) -> Option<MaterialDef> {
        let index = self.materials.iter().position(|m| &m.id == id)?;
        self.generation += 1;
        Some(self.materials.remove(index))
    }
}

// ---------------------------------------------------------------------------
// Resolution: `MaterialRef` -> live `MeshMaterial3d` (M4-D11, spec §7).
//
// The reference sits on the SCENE entity, but the renderable meshes may not:
// a placed model (`MeshRef`) carries no `Mesh3d` of its own — its geometry
// lives in the DERIVED gltf subtree (spec §6), a hierarchy of node entities
// whose leaves hold the primitives with the materials the artist exported.
// Bevy materials do not inherit down a hierarchy, so assignment must REACH
// those primitives; anything less shades nothing at all.
//
// Three arrivals must land the same override, because a GLB resolves
// ASYNCHRONOUSLY: the reference changing, the library changing, and the
// subtree's meshes appearing (first load, or a respawn after re-import).
// ---------------------------------------------------------------------------

/// GPU handles per library material — created on demand, patched IN PLACE on
/// library edits so every entity using the material re-shades live.
#[derive(Resource, Default)]
pub struct MaterialHandles(pub std::collections::HashMap<Uuid, Handle<StandardMaterial>>);

/// The material an overridden mesh had BEFORE the override (the artist's gltf
/// material, or a game's own). Removing the reference — undo of a first
/// assignment — restores exactly this, never a guess.
#[derive(Component, Clone)]
pub struct SourceMaterial(Option<MeshMaterial3d<StandardMaterial>>);

/// Every mesh in the subtree, and what it is currently shaded with.
type MeshState<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static MeshMaterial3d<StandardMaterial>>,
        Option<&'static SourceMaterial>,
    ),
    With<Mesh3d>,
>;

/// A descendant that is a scene entity in its own right — or carries its own
/// reference — owns its material; an ancestor's override stops there.
fn is_boundary(
    entity: Entity,
    refs: &Query<(Entity, Ref<MaterialRef>)>,
    scene_entities: &Query<(), With<SceneId>>,
) -> bool {
    refs.contains(entity) || scene_entities.contains(entity)
}

/// Where an override on `root` reaches: every mesh from `root` down, stopping
/// at boundaries. For a placed model this is the whole derived gltf subtree.
fn override_targets(
    root: Entity,
    children: &Query<&Children>,
    mesh_state: &MeshState,
    refs: &Query<(Entity, Ref<MaterialRef>)>,
    scene_entities: &Query<(), With<SceneId>>,
) -> Vec<Entity> {
    let mut targets = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        // The root always applies, boundary or not — it IS the boundary.
        if entity != root && is_boundary(entity, refs, scene_entities) {
            continue;
        }
        if mesh_state.contains(entity) {
            targets.push(entity);
        }
        if let Ok(kids) = children.get(entity) {
            stack.extend(kids.iter());
        }
    }
    targets
}

/// The nearest ancestor (self included) holding a `MaterialRef`, without
/// crossing another scene entity — used when a mesh APPEARS under an already
/// overridden root (async gltf load, re-import respawn).
fn owning_ref(
    entity: Entity,
    parents: &Query<&ChildOf>,
    refs: &Query<(Entity, Ref<MaterialRef>)>,
    scene_entities: &Query<(), With<SceneId>>,
) -> Option<(Entity, MaterialRef)> {
    let mut current = entity;
    loop {
        if let Ok((_, material_ref)) = refs.get(current) {
            return Some((current, *material_ref));
        }
        // A scene entity without a reference terminates the search: the mesh
        // belongs to IT, not to whatever it happens to sit under.
        if current != entity && scene_entities.contains(current) {
            return None;
        }
        current = parents.get(current).ok()?.parent();
    }
}

/// `MaterialRef` -> `MeshMaterial3d`, reaching into derived model subtrees.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_material_refs(
    library: Res<MaterialLibrary>,
    models: Res<crate::models::ModelLibrary>,
    asset_server: Option<Res<AssetServer>>,
    mut handles: ResMut<MaterialHandles>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    refs: Query<(Entity, Ref<MaterialRef>)>,
    mut removed: RemovedComponents<MaterialRef>,
    appeared: Query<Entity, Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    mesh_state: MeshState,
    scene_entities: Query<(), With<SceneId>>,
    mut commands: Commands,
) {
    // Headless contexts (unit tests, CLI) have no render assets — nothing to
    // resolve to, and nothing to see.
    let Some(mut materials) = materials else {
        return;
    };
    let server = asset_server.as_deref();
    // Library param edits patch the SHARED handles: no entity is touched, and
    // every user of the material re-shades this frame.
    if library.is_changed() {
        // Resolve, so an edit to a BASE re-shades every material inheriting
        // from it in the same pass — which is the whole point of inheritance.
        for def in &library.materials {
            if let Some(handle) = handles.0.get(&def.id)
                && let Some(mut material) = materials.get_mut(handle)
                && let Some(resolved) = library.resolved(&def.id)
            {
                *material = to_standard_material(&resolved, &models, server);
            }
        }
    }
    let handle_for = |id: &Uuid,
                      handles: &mut MaterialHandles,
                      materials: &mut Assets<StandardMaterial>|
     -> Option<Handle<StandardMaterial>> {
        let resolved = library.resolved(id)?;
        Some(
            handles
                .0
                .entry(resolved.id)
                .or_insert_with(|| materials.add(to_standard_material(&resolved, &models, server)))
                .clone(),
        )
    };
    let apply = |target: Entity, handle: Handle<StandardMaterial>, commands: &mut Commands| {
        let Ok((current, source)) = mesh_state.get(target) else {
            return;
        };
        // Remember what we displaced, ONCE — a second assignment must not
        // record the first override as the original.
        if source.is_none() {
            commands
                .entity(target)
                .insert(SourceMaterial(current.cloned()));
        }
        commands.entity(target).insert(MeshMaterial3d(handle));
    };

    // 1. Reference added/changed — and every reference when the library moved,
    //    since a freshly created material has no handle yet.
    let library_moved = library.is_changed();
    for (root, material_ref) in &refs {
        if !library_moved && !material_ref.is_changed() {
            continue;
        }
        let Some(handle) = handle_for(&material_ref.0, &mut handles, &mut materials) else {
            continue; // dangling reference — leave the geometry as authored
        };
        for target in override_targets(root, &children, &mesh_state, &refs, &scene_entities) {
            apply(target, handle.clone(), &mut commands);
        }
    }

    // 2. Meshes that only just appeared: a model's gltf subtree resolves
    //    frames after the reference was assigned (and respawns wholesale on
    //    re-import), so the override has to chase the geometry.
    for mesh in &appeared {
        let Some((_root, material_ref)) = owning_ref(mesh, &parents, &refs, &scene_entities) else {
            continue;
        };
        if let Some(handle) = handle_for(&material_ref.0, &mut handles, &mut materials) {
            apply(mesh, handle, &mut commands);
        }
    }

    // 3. Reference removed (undo of a first assignment): put back exactly what
    //    the override displaced.
    for root in removed.read() {
        if commands.get_entity(root).is_err() {
            continue; // despawned along with its reference
        }
        for target in override_targets(root, &children, &mesh_state, &refs, &scene_entities) {
            let Ok((_, Some(source))) = mesh_state.get(target) else {
                continue;
            };
            let mut entity = commands.entity(target);
            match &source.0 {
                Some(material) => {
                    entity.insert(material.clone());
                }
                None => {
                    entity.remove::<MeshMaterial3d<StandardMaterial>>();
                }
            }
            entity.remove::<SourceMaterial>();
        }
    }
}

#[derive(Debug)]
pub enum MaterialsError {
    Io(std::io::Error),
    Format(String),
    FutureVersion { found: u32, supported: u32 },
}

pub fn save_materials(library: &MaterialLibrary, path: &Path) -> Result<(), MaterialsError> {
    let envelope = MaterialsEnvelope {
        format_version: MATERIALS_FORMAT_VERSION,
        materials: library.materials.clone(),
    };
    let text = ron::ser::to_string_pretty(&envelope, ron::ser::PrettyConfig::default())
        .map_err(|e| MaterialsError::Format(e.to_string()))?;
    let tmp = path.with_extension("ron.tmp");
    std::fs::write(&tmp, &text).map_err(MaterialsError::Io)?;
    if path.exists() {
        let bak = path.with_extension("ron.bak");
        let _ = std::fs::copy(path, bak);
    }
    std::fs::rename(&tmp, path).map_err(MaterialsError::Io)?;
    Ok(())
}

/// Non-destructive load: parse fully before touching the resource; unknown FUTURE
/// versions refuse loudly (same contract as scenes).
pub fn load_materials(path: &Path) -> Result<Vec<MaterialDef>, MaterialsError> {
    let text = std::fs::read_to_string(path).map_err(MaterialsError::Io)?;
    let envelope: MaterialsEnvelope =
        ron::from_str(&text).map_err(|e| MaterialsError::Format(e.to_string()))?;
    if envelope.format_version > MATERIALS_FORMAT_VERSION {
        return Err(MaterialsError::FutureVersion {
            found: envelope.format_version,
            supported: MATERIALS_FORMAT_VERSION,
        });
    }
    let mut materials = envelope.materials;
    for def in &mut materials {
        def.migrate();
    }
    Ok(materials)
}

pub(crate) fn load_library_at_startup(mut library: ResMut<MaterialLibrary>) {
    let path = library.path.clone();
    match load_materials(&path) {
        Ok(materials) => {
            library.materials = materials;
            library.generation += 1;
        }
        Err(MaterialsError::Io(_)) => {} // no library yet — starts empty
        Err(e) => error!("materials library load failed: {e:?} — starting empty"),
    }
}

/// Library mutations persist immediately (atomic).
pub(crate) fn save_library_on_change(library: Res<MaterialLibrary>, mut last_saved: Local<u64>) {
    if library.generation == *last_saved || library.generation == 0 {
        return;
    }
    *last_saved = library.generation;
    let path = library.path.clone();
    if let Err(e) = save_materials(&library, &path) {
        error!("materials library save failed: {e:?}");
    }
}

/// `material.new`: append a fresh material and report it (the palette lists it
/// immediately; params are tuned from the inspector once assigned).
pub(crate) fn handle_material_actions(
    mut reader: MessageReader<ActionInvoked>,
    mut library: ResMut<MaterialLibrary>,
    mut feedback: MessageWriter<crate::SceneIoFeedback>,
) {
    for invoked in reader.read() {
        if invoked.action.as_str() == "material.new" {
            let count = library.materials.len() + 1;
            let def = MaterialDef {
                id: Uuid::new_v4(),
                name: format!("Material {count}"),
                ..Default::default()
            };
            let name = def.name.clone();
            library.add(def);
            feedback.write(crate::SceneIoFeedback {
                message: format!("created {name}"),
                success: true,
            });
        }
    }
}

pub(crate) struct MaterialsFeature;

impl EditorFeature for MaterialsFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("materials", "Material Library")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.component::<MaterialRef>()
            .action(
                ActionDef::new("material.new", "New Material")
                    .describe("Create a material in the library")
                    .context("normal"),
            )
            .action(
                ActionDef::new("material.assign", "Assign Material")
                    .describe("Pick a library material for the selection")
                    .context("normal")
                    .bind("space m"),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::prelude::{History, HistoryRequests};

    fn library_with(defs: Vec<MaterialDef>) -> MaterialLibrary {
        let mut library = MaterialLibrary::default();
        for def in defs {
            library.add(def);
        }
        library
    }

    // Spec §6: a material is a library REFERENCE plus inline overrides. An
    // instance follows its base for everything it has not claimed.
    #[test]
    fn an_instance_resolves_over_its_base() {
        let base = Uuid::new_v4();
        let instance = Uuid::new_v4();
        let library = library_with(vec![
            MaterialDef {
                id: base,
                name: "Stone".into(),
                base_color: [0.5, 0.5, 0.5, 1.0],
                roughness: 0.9,
                metallic: 0.1,
                ..Default::default()
            },
            MaterialDef {
                id: instance,
                name: "Mossy Stone".into(),
                base: Some(base),
                base_color: [0.2, 0.6, 0.2, 1.0],
                overridden: [MaterialField::BaseColor].into_iter().collect(),
                ..Default::default()
            },
        ]);
        let resolved = library.resolved(&instance).unwrap();
        assert_eq!(
            resolved.base_color,
            [0.2, 0.6, 0.2, 1.0],
            "the claimed field is the instance's own"
        );
        assert_eq!(resolved.roughness, 0.9, "the rest follows the base");
        assert_eq!(resolved.metallic, 0.1, "all of the rest");
        assert_eq!(resolved.name, "Mossy Stone", "identity is never inherited");
    }

    // THE payoff: change the base once and every instance follows. This is what
    // makes a late art-direction change one edit instead of N.
    #[test]
    fn a_base_edit_reaches_every_instance() {
        let base = Uuid::new_v4();
        let instances: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        let mut defs = vec![MaterialDef {
            id: base,
            roughness: 0.9,
            ..Default::default()
        }];
        defs.extend(instances.iter().map(|id| MaterialDef {
            id: *id,
            base: Some(base),
            ..Default::default()
        }));
        let mut library = library_with(defs);

        library.get_mut(&base).unwrap().roughness = 0.1;

        for id in &instances {
            assert_eq!(
                library.resolved(id).unwrap().roughness,
                0.1,
                "the instance followed the base"
            );
        }
    }

    // Inheritance is a CHAIN: an instance of an instance resolves through both.
    #[test]
    fn inheritance_resolves_through_a_chain() {
        let (root, middle, leaf) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let library = library_with(vec![
            MaterialDef {
                id: root,
                roughness: 0.9,
                metallic: 0.0,
                ..Default::default()
            },
            MaterialDef {
                id: middle,
                base: Some(root),
                metallic: 1.0,
                overridden: [MaterialField::Metallic].into_iter().collect(),
                ..Default::default()
            },
            MaterialDef {
                id: leaf,
                base: Some(middle),
                unlit: true,
                overridden: [MaterialField::Unlit].into_iter().collect(),
                ..Default::default()
            },
        ]);
        let resolved = library.resolved(&leaf).unwrap();
        assert_eq!(resolved.roughness, 0.9, "from the root");
        assert_eq!(resolved.metallic, 1.0, "from the middle");
        assert!(resolved.unlit, "its own");
    }

    // A cycle must not hang the editor. Refusing to render would be worse than
    // resolving to something sane.
    #[test]
    fn a_cycle_resolves_instead_of_hanging() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let library = library_with(vec![
            MaterialDef {
                id: a,
                base: Some(b),
                roughness: 0.1,
                overridden: [MaterialField::Roughness].into_iter().collect(),
                ..Default::default()
            },
            MaterialDef {
                id: b,
                base: Some(a),
                ..Default::default()
            },
        ]);
        let resolved = library.resolved(&a).expect("a cycle still resolves");
        assert_eq!(resolved.id, a, "and resolves to the material asked for");
        assert_eq!(resolved.roughness, 0.1, "keeping its own claimed field");
    }

    // Detaching must not change what the surface LOOKS like — it only changes
    // where the values come from.
    #[test]
    fn detaching_keeps_exactly_what_it_looked_like() {
        let base = Uuid::new_v4();
        let instance = Uuid::new_v4();
        let mut library = library_with(vec![
            MaterialDef {
                id: base,
                roughness: 0.9,
                metallic: 0.4,
                ..Default::default()
            },
            MaterialDef {
                id: instance,
                base: Some(base),
                unlit: true,
                overridden: [MaterialField::Unlit].into_iter().collect(),
                ..Default::default()
            },
        ]);
        let before = library.resolved(&instance).unwrap();

        let resolved = library.resolved(&instance).unwrap();
        *library.get_mut(&instance).unwrap() = MaterialDef {
            base: None,
            overridden: std::collections::BTreeSet::new(),
            ..resolved
        };

        let after = library.resolved(&instance).unwrap();
        assert_eq!(after.roughness, before.roughness, "the look is identical");
        assert_eq!(after.metallic, before.metallic);
        assert_eq!(after.unlit, before.unlit);
        assert!(after.base.is_none(), "and it follows nothing now");

        // Proof it detached: the base can move and the instance no longer cares.
        library.get_mut(&base).unwrap().roughness = 0.0;
        assert_eq!(
            library.resolved(&instance).unwrap().roughness,
            before.roughness,
            "a detached material stopped listening"
        );
    }

    // A material with no base resolves to itself, unchanged — the common case
    // has to cost nothing and change nothing.
    #[test]
    fn a_material_with_no_base_resolves_to_itself() {
        let id = Uuid::new_v4();
        let library = library_with(vec![MaterialDef {
            id,
            name: "Plain".into(),
            roughness: 0.33,
            ..Default::default()
        }]);
        assert_eq!(library.resolved(&id).unwrap(), *library.get(&id).unwrap());
    }
    // The autosave rewrites materials.ron whenever the generation moves, so a
    // FAILED lookup must not move it — that was a disk write for an edit that
    // never happened (spec §8: no per-frame work at rest).
    #[test]
    fn a_failed_lookup_does_not_dirty_the_library() {
        let mut library = MaterialLibrary::default();
        library.add(MaterialDef {
            id: Uuid::new_v4(),
            ..Default::default()
        });
        let clean = library.generation;
        assert!(library.get_mut(&Uuid::new_v4()).is_none(), "a miss is None");
        assert_eq!(
            library.generation, clean,
            "a miss leaves the library clean, so nothing is written"
        );
        let id = library.materials[0].id;
        assert!(library.get_mut(&id).is_some());
        assert!(
            library.generation > clean,
            "a hit still marks it dirty, so the edit is saved"
        );
    }

    // Removing reports what it removed and marks the library dirty; removing
    // something absent changes nothing at all.
    #[test]
    fn remove_takes_exactly_one_material() {
        let mut library = MaterialLibrary::default();
        let keep = Uuid::new_v4();
        let drop = Uuid::new_v4();
        for id in [keep, drop] {
            library.add(MaterialDef {
                id,
                name: format!("m{id}"),
                ..Default::default()
            });
        }
        let before = library.generation;
        let removed = library.remove(&drop).expect("the material was there");
        assert_eq!(removed.id, drop);
        assert_eq!(library.materials.len(), 1);
        assert_eq!(library.materials[0].id, keep, "the other one is untouched");
        assert!(library.generation > before, "removal is a change");

        let after = library.generation;
        assert!(library.remove(&drop).is_none(), "removing twice is a miss");
        assert_eq!(library.generation, after, "and a miss changes nothing");
    }
    // Format 1 wrote a single `base_color_texture`; format 2 has a slot table.
    // Old files have to keep working, and the migration has to be idempotent.
    #[test]
    fn format_1_textures_migrate_into_the_slot_table() {
        let dir = std::env::temp_dir().join(format!("mat-migrate-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("materials.ron");
        let texture = Uuid::new_v4();
        std::fs::write(
            &path,
            format!(
                "(format_version: 1, materials: [(id: \"{}\", name: \"Old\", \
                 base_color_texture: Some(\"{texture}\"))])",
                Uuid::new_v4()
            ),
        )
        .unwrap();

        let loaded = load_materials(&path).expect("a format-1 file still loads");
        assert_eq!(loaded.len(), 1);
        assert_eq!(
            loaded[0].texture(TextureSlot::BaseColor),
            Some(texture),
            "the legacy field lands in the base colour slot"
        );
        assert!(
            loaded[0].base_color_texture.is_none(),
            "and is cleared, so nothing downstream reads two sources"
        );

        // Idempotent: migrating again changes nothing.
        let mut again = loaded[0].clone();
        again.migrate();
        assert_eq!(again, loaded[0], "migration runs clean twice");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // THE reason the slot table exists: colour maps are sRGB-encoded and data
    // maps are linear. Loading a normal map as sRGB gamma-corrupts every vector
    // in it, and that is exactly what a single hardcoded flag did.
    #[test]
    fn only_colour_slots_are_srgb() {
        for slot in TextureSlot::ALL {
            let expected = matches!(slot, TextureSlot::BaseColor | TextureSlot::Emissive);
            assert_eq!(
                slot.is_srgb(),
                expected,
                "{slot:?} carries {} data",
                if expected { "colour" } else { "linear" }
            );
        }
    }

    // A def round-trips through RON with every slot filled, so a saved
    // material comes back the same material.
    #[test]
    fn a_full_slot_table_round_trips() {
        let mut def = MaterialDef {
            id: Uuid::new_v4(),
            name: "Full".into(),
            uv_tiling: [4.0, 2.0],
            uv_offset: [0.25, 0.5],
            ..Default::default()
        };
        for slot in TextureSlot::ALL {
            def.set_texture(slot, Some(Uuid::new_v4()));
        }
        let text = ron::ser::to_string(&def).unwrap();
        let back: MaterialDef = ron::from_str(&text).unwrap();
        assert_eq!(back, def, "every slot and the uv transform survive a save");
    }

    // Clearing a slot removes it rather than storing a hole.
    #[test]
    fn clearing_a_slot_empties_it() {
        let mut def = MaterialDef::default();
        def.set_texture(TextureSlot::Normal, Some(Uuid::new_v4()));
        assert!(def.texture(TextureSlot::Normal).is_some());
        def.set_texture(TextureSlot::Normal, None);
        assert_eq!(def.texture(TextureSlot::Normal), None);
        assert!(def.textures.is_empty(), "no empty entry left behind");
    }
    // C6: assignment is ONE undoable transaction; undo removes the reference.
    #[test]
    fn assignment_is_undoable() {
        let mut app = App::new();
        app.add_plugins(editor_core::EditorCorePlugin);
        struct TestFeature;
        impl EditorFeature for TestFeature {
            fn manifest(&self) -> FeatureManifest {
                FeatureManifest::new("mat-test", "Mat Test")
            }
            fn register(&self, reg: &mut FeatureRegistry) {
                reg.component::<MaterialRef>();
            }
        }
        app.add_editor_feature(TestFeature);
        app.init_resource::<bevy::input::ButtonInput<bevy::input::keyboard::KeyCode>>();
        app.finish();
        app.update();

        let (a, b) = (SceneId::random(), SceneId::random());
        for id in [a, b] {
            app.world_mut()
                .resource_mut::<EditQueue>()
                .0
                .push(Transaction {
                    label: "spawn".into(),
                    gesture: None,
                    ops: vec![Op::Spawn {
                        id,
                        components: vec![],
                    }],
                });
        }
        app.update();

        let material = Uuid::new_v4();
        let depth = app.world().resource::<History>().undo_depth();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Assign Material".into(),
                gesture: None,
                ops: [a, b]
                    .into_iter()
                    .map(|target| Op::Set {
                        target,
                        value: Box::new(MaterialRef(material)).into_partial_reflect(),
                    })
                    .collect(),
            });
        app.update();
        let world = app.world_mut();
        let assigned = world
            .query::<&MaterialRef>()
            .iter(world)
            .filter(|m| m.0 == material)
            .count();
        assert_eq!(assigned, 2);
        assert_eq!(
            world.resource::<History>().undo_depth(),
            depth + 1,
            "one entry"
        );

        world.resource_mut::<HistoryRequests>().undo = 1;
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world.query::<&MaterialRef>().iter(world).count(),
            0,
            "undo removes"
        );
    }

    // C6: versioned envelope round-trips byte-identically; future versions refuse.
    #[test]
    fn library_round_trip_and_versioning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("materials.ron");
        let mut library = MaterialLibrary::default();
        library.add(MaterialDef {
            id: Uuid::new_v4(),
            name: "Rust".into(),
            base_color: [0.7, 0.3, 0.1, 1.0],
            metallic: 0.9,
            roughness: 0.3,
            ..Default::default()
        });
        save_materials(&library, &path).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();

        let loaded = load_materials(&path).unwrap();
        assert_eq!(loaded, library.materials);
        let mut reloaded = MaterialLibrary::default();
        reloaded.materials = loaded;
        save_materials(&reloaded, &path).unwrap();
        let second = std::fs::read_to_string(&path).unwrap();
        assert_eq!(first, second, "save -> load -> save byte-identical");

        std::fs::write(&path, "(format_version: 99, materials: [])").unwrap();
        assert!(matches!(
            load_materials(&path),
            Err(MaterialsError::FutureVersion { found: 99, .. })
        ));
    }
}
