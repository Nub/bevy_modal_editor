//! Live 3D preview in the palette's right pane (v1 parity, owner-requested):
//! the highlighted primitive kind or prefab renders to a texture on an isolated
//! render layer — you SEE what you're about to place before committing.
//!
//! One camera + one content root live permanently on `PREVIEW_LAYER`; the
//! palette writes `PreviewSubject` as the highlight moves and the content is
//! rebuilt through the same reflection-apply path as real placement, so the
//! game's regenerate observers (mesh derivation) fire exactly as they do in
//! the scene. Preview entities carry no `SceneId` — invisible to capture,
//! selection, and every editor system by construction.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::ecs::relationship::RelationshipHookMode;
use bevy::prelude::*;
use editor_core::prelude::*;
use editor_prefabs::PrefabLibrary;
use uuid::Uuid;

/// Distinct from the outliner's silhouette layer (31).
const PREVIEW_LAYER: usize = 41;
const PREVIEW_SIZE: u32 = 512;
/// Content parks far below the scene: layer isolation already hides it from
/// every scene camera, this just keeps AABB/debug tooling from overlapping.
const PREVIEW_HOME: Vec3 = Vec3::new(0.0, -900.0, 0.0);

/// What the palette wants previewed (None = pane shows text docs only).
#[derive(Resource, Default, PartialEq)]
pub(crate) struct PreviewSubject(pub Option<Subject>);

#[derive(Clone, PartialEq)]
pub(crate) enum Subject {
    Kind(EntityKindId),
    Prefab(Uuid),
    /// A library material, shown on a sphere — the same read the material
    /// editor's own preview gives, so "which one is the rusty metal" is a
    /// look, not a guess at a name.
    Material(Uuid),
    /// An imported texture, unlit on the same sphere: picking a MAP is the one
    /// case where you want the image itself and not a lighting judgement, and
    /// unlit is as close to "the file" as this rig gets.
    Texture(Uuid),
    /// An imported model, rendered from the same `MeshRef` the scene uses.
    /// Without this a kit of forty near-identical wall pieces is forty text
    /// rows you have to already know the names of.
    Model(Uuid),
}

#[derive(Resource)]
pub(crate) struct PreviewRig {
    pub image: Handle<Image>,
    camera: Entity,
    root: Entity,
}

#[derive(Component)]
pub(crate) struct PreviewContent;

pub(crate) fn setup_preview_rig(
    environment: Res<crate::preview_env::PreviewEnvironment>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
) {
    let image = images.add(Image::new_target_texture(
        PREVIEW_SIZE,
        PREVIEW_SIZE,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    let root = commands
        .spawn((
            Transform::from_translation(PREVIEW_HOME),
            Visibility::default(),
        ))
        .id();
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                clear_color: ClearColorConfig::Custom(Color::NONE),
                order: -10,
                is_active: false,
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            Transform::from_translation(PREVIEW_HOME + Vec3::new(2.4, 1.8, 2.4))
                .looking_at(PREVIEW_HOME, Vec3::Y),
            RenderLayers::layer(PREVIEW_LAYER),
            // The same room as the material panel: a palette chip and the
            // editor's own preview must not disagree about what a surface
            // looks like.
            bevy::light::GeneratedEnvironmentMapLight {
                environment_map: environment.0.clone(),
                intensity: 900.0,
                ..default()
            },
        ))
        .id();
    commands.spawn((
        DirectionalLight {
            illuminance: 6_000.0,
            ..default()
        },
        Transform::from_translation(PREVIEW_HOME + Vec3::new(3.0, 6.0, 2.0))
            .looking_at(PREVIEW_HOME, Vec3::Y),
        RenderLayers::layer(PREVIEW_LAYER),
    ));
    commands.insert_resource(PreviewRig {
        image,
        camera,
        root,
    });
}

/// Rebuild the preview content when the highlighted subject changes; keep the
/// camera active only while something is showing (idle palette costs nothing).
pub(crate) fn sync_preview_content(world: &mut World) {
    let subject = world.resource::<PreviewSubject>().0.clone();
    let showing_changed = {
        let mut last = world.get_resource_or_insert_with::<LastShown>(Default::default);
        if last.0 == subject {
            false
        } else {
            last.0 = subject.clone();
            true
        }
    };
    if !showing_changed {
        return;
    }
    let (root, camera) = {
        let rig = world.resource::<PreviewRig>();
        (rig.root, rig.camera)
    };

    // Clear previous content.
    let old: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<PreviewContent>>();
        query.iter(world).collect()
    };
    for entity in old {
        // May already be gone: the palette UI despawns its pane's children
        // (recursively, previews included) when it closes or rebuilds.
        if let Ok(entity_mut) = world.get_entity_mut(entity) {
            entity_mut.despawn();
        }
    }
    if let Some(mut camera) = world.get_mut::<Camera>(camera) {
        camera.is_active = subject.is_some();
    }

    // Every subject starts from the SAME three-quarter pose. The turntable
    // never reset, so arrowing down a kit showed each piece at whatever yaw the
    // last one happened to reach — and for forty walls that differ by a window
    // or a broken corner, comparing them depends entirely on their being shown
    // alike. A flipbook makes the difference pop; forty unrelated pictures hide
    // it. It also makes the preview screenshot deterministic.
    if let Some(mut transform) = world.get_mut::<Transform>(root) {
        transform.rotation = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_8);
    }

    let mut radius: f32 = 1.0;
    match subject {
        None => {}
        Some(Subject::Texture(texture)) => {
            // A one-slot material carrying just this map, unlit: what the file
            // looks like, wrapped on the same ball as everything else.
            let mut def = editor_scene::materials::MaterialDef {
                unlit: true,
                ..Default::default()
            };
            def.set_texture(
                editor_scene::materials::TextureSlot::BaseColor,
                Some(texture),
            );
            let standard =
                world.resource_scope(|world, models: Mut<editor_scene::models::ModelLibrary>| {
                    let assets = world.get_resource::<AssetServer>().cloned();
                    editor_scene::materials::to_standard_material(&def, &models, assets.as_ref())
                });
            let material_handle = world
                .resource_mut::<Assets<StandardMaterial>>()
                .add(standard);
            let mesh = world
                .resource_mut::<Assets<Mesh>>()
                .add(editor_scene::materials::primitive_mesh(Sphere::new(1.0)));
            world.spawn((
                PreviewContent,
                Mesh3d(mesh),
                MeshMaterial3d(material_handle),
                Transform::default(),
                RenderLayers::layer(PREVIEW_LAYER),
                // Unparented, this sphere sat at the world ORIGIN while the
                // preview camera looked 900 units below it: the texture chip
                // has been rendering an empty pane.
                ChildOf(root),
            ));
        }
        Some(Subject::Material(material)) => {
            // The chip shows the RESOLVED material, the same as the scene.
            let Some(def) = world
                .resource::<editor_scene::materials::MaterialLibrary>()
                .resolved(&material)
            else {
                return;
            };
            // THE conversion (textures resolve through the identity pipeline),
            // so the chip shows exactly what the scene will.
            let standard =
                world.resource_scope(|world, models: Mut<editor_scene::models::ModelLibrary>| {
                    let assets = world.get_resource::<AssetServer>().cloned();
                    editor_scene::materials::to_standard_material(&def, &models, assets.as_ref())
                });
            let material_handle = world
                .resource_mut::<Assets<StandardMaterial>>()
                .add(standard);
            let mesh = world
                .resource_mut::<Assets<Mesh>>()
                .add(editor_scene::materials::primitive_mesh(Sphere::new(1.0)));
            world.spawn((
                PreviewContent,
                Mesh3d(mesh),
                MeshMaterial3d(material_handle),
                // The ROOT is already at `PREVIEW_HOME`; translating by it
                // again put this sphere at y = -1800, twice as far down as the
                // camera looks. The material chip has been empty too.
                Transform::default(),
                RenderLayers::layer(PREVIEW_LAYER),
                ChildOf(root),
            ));
        }
        Some(Subject::Kind(kind)) => {
            let Some(components) = world
                .resource::<KindCatalog>()
                .get(&kind)
                .map(|d| (d.components)(Vec3::ZERO))
            else {
                return;
            };
            spawn_preview_entity(world, root, &components);
        }
        Some(Subject::Model(model)) => {
            // The SAME reference the scene uses: the resolver spawns the
            // derived gltf subtree under this entity exactly as it does for a
            // placed model, so the preview cannot drift from what placement
            // actually produces.
            let bounds = world
                .resource::<editor_scene::models::ModelLibrary>()
                .get(&model)
                .and_then(|entry| entry.bounds);
            let entity = spawn_preview_entity(
                world,
                root,
                &[Box::new(editor_scene::models::MeshRef(model)).into_partial_reflect()],
            );
            // Centre and frame it from the bounds the PROCESS STAGE recorded at
            // import. Measuring live would mean waiting for the gltf to load —
            // the preview would open mis-framed and jump. The number is already
            // in the library on the first frame the pane appears.
            if let Some(bounds) = bounds {
                let (min, max) = (Vec3::from(bounds.min), Vec3::from(bounds.max));
                let centre = (min + max) * 0.5;
                if let Some(mut transform) = world.get_mut::<Transform>(entity) {
                    // A wall pivoted at one end, a prop pivoted at its base:
                    // most models are not centred on their own origin, and a
                    // preview that frames the origin shows half the asset.
                    transform.translation = -centre;
                }
                radius = radius.max((max - min).length() * 0.5);
            }
        }
        Some(Subject::Prefab(prefab)) => {
            // Template records → preview entities (same reflection path as
            // stamping, minus every piece of scene bookkeeping).
            let records: Vec<(
                SceneId,
                Option<SceneId>,
                Vec<Box<dyn bevy::reflect::PartialReflect>>,
            )> = {
                let library = world.resource::<PrefabLibrary>();
                let Some(def) = library.prefabs.get(&prefab) else {
                    return;
                };
                def.template
                    .records()
                    .map(|(id, parent, components)| {
                        (
                            id,
                            parent,
                            components.iter().map(|c| c.to_dynamic()).collect(),
                        )
                    })
                    .collect()
            };
            let mut spawned: std::collections::HashMap<SceneId, Entity> =
                std::collections::HashMap::new();
            for (id, _, components) in &records {
                let entity = spawn_preview_entity(world, root, components);
                spawned.insert(*id, entity);
                if let Some(transform) = world.get::<Transform>(entity) {
                    radius = radius.max(transform.translation.length() + 1.0);
                }
            }
            for (id, parent, _) in &records {
                if let Some(parent_entity) = parent.and_then(|p| spawned.get(&p))
                    && let Some(entity) = spawned.get(id)
                {
                    world.entity_mut(*entity).insert(ChildOf(*parent_entity));
                }
            }
        }
    }

    // Frame the content: fit distance from the content's bounding radius.
    if let Some(mut transform) = world.get_mut::<Transform>(camera) {
        let distance = radius * 2.2;
        *transform = Transform::from_translation(
            PREVIEW_HOME + Vec3::new(distance, distance * 0.75, distance),
        )
        .looking_at(PREVIEW_HOME + Vec3::Y * (radius * 0.3), Vec3::Y);
    }
}

#[derive(Resource, Default)]
struct LastShown(Option<Subject>);

fn spawn_preview_entity(
    world: &mut World,
    root: Entity,
    components: &[Box<dyn bevy::reflect::PartialReflect>],
) -> Entity {
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();
    let entity = world
        .spawn((
            PreviewContent,
            RenderLayers::layer(PREVIEW_LAYER),
            bevy::picking::Pickable::IGNORE,
            ChildOf(root),
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    for value in components {
        let Some(info) = value.get_represented_type_info() else {
            continue;
        };
        let Some(registration) = registry.get(info.type_id()) else {
            continue;
        };
        let Some(reflect_component) = registration.data::<bevy::ecs::reflect::ReflectComponent>()
        else {
            continue;
        };
        let Ok(mut entity_mut) = world.get_entity_mut(entity) else {
            continue;
        };
        reflect_component.apply_or_insert_mapped(
            &mut entity_mut,
            value.as_ref(),
            &registry,
            &mut (),
            RelationshipHookMode::Run,
        );
    }
    entity
}

/// Everything under the preview root joins the preview — and nothing under it
/// is allowed to touch the real editor.
///
/// Two separate reasons this cannot be "stamp the entity I spawned":
///
/// 1. Regenerate observers attach meshes AFTER the spawn, and a model's
///    geometry arrives deeper still — the gltf scene spawner builds a whole
///    subtree of its own, none of it carrying `PreviewContent`. `RenderLayers`
///    does not propagate in Bevy, so an unstamped mesh is invisible to the
///    preview camera and visible to the level's.
/// 2. A gltf brings whatever the artist saved in it. Bevy's loader defaults to
///    `load_cameras: true, load_lights: true`, and the camera it spawns is
///    ACTIVE when no other active camera was found, pointed at the primary
///    window. A Blender file with its default Sun and Camera would therefore
///    light — or take over — the actual level, once per highlighted palette
///    row. The preview must be a room, not a guest with opinions.
///
/// Walks DOWN from the root rather than testing ancestry across the world:
/// scene meshes carry no `RenderLayers` either, so a global query would sweep
/// the whole level every frame to rediscover that a wall is not a palette chip.
pub(crate) fn contain_preview_content(
    rig: Option<Res<PreviewRig>>,
    children: Query<&Children>,
    meshes: Query<(), (With<Mesh3d>, Without<RenderLayers>)>,
    lights: Query<
        (),
        (
            Without<RenderLayers>,
            Or<(With<DirectionalLight>, With<PointLight>, With<SpotLight>)>,
        ),
    >,
    mut cameras: Query<&mut Camera, Without<RenderLayers>>,
    mut commands: Commands,
) {
    let Some(rig) = rig else { return };
    let mut stack = vec![rig.root];
    while let Some(entity) = stack.pop() {
        if let Ok(kids) = children.get(entity) {
            stack.extend(kids.iter());
        }
        if meshes.contains(entity) {
            commands.entity(entity).insert((
                RenderLayers::layer(PREVIEW_LAYER),
                // A chip is a picture, not a click target.
                bevy::picking::Pickable::IGNORE,
            ));
        } else if lights.contains(entity) {
            // Confine it: an imported sun lights the preview room only.
            commands
                .entity(entity)
                .insert(RenderLayers::layer(PREVIEW_LAYER));
        } else if let Ok(mut camera) = cameras.get_mut(entity) {
            // An imported camera renders to the primary window at order 0.
            // Nothing about highlighting a palette row should change what the
            // designer is looking at.
            camera.is_active = false;
        }
    }
}

/// Slow turntable while the preview is live — reads as "this is a 3D object".
pub(crate) fn turn_preview(
    time: Res<Time>,
    rig: Option<Res<PreviewRig>>,
    subject: Res<PreviewSubject>,
    mut transforms: Query<&mut Transform>,
) {
    let Some(rig) = rig else { return };
    if subject.0.is_none() {
        return;
    }
    if let Ok(mut transform) = transforms.get_mut(rig.root) {
        transform.rotate_y(time.delta_secs() * 0.6);
    }
}

/// How many meshes are actually rendering into the preview pane right now.
///
/// On the preview layer AND within reach of the preview camera. The layer
/// alone is not the question: both sibling chips carried the right layer and
/// sat 900 units from where the camera looks, so "a mesh exists on the preview
/// layer" was true of two panes that rendered nothing. What a probe wants to
/// know is whether the picture has anything in it.
pub(crate) fn preview_mesh_count(world: &mut World) -> usize {
    let layer = RenderLayers::layer(PREVIEW_LAYER);
    world
        .query_filtered::<(&RenderLayers, &GlobalTransform), With<Mesh3d>>()
        .iter(world)
        .filter(|(layers, global)| {
            layers.intersects(&layer) && global.translation().distance(PREVIEW_HOME) < 50.0
        })
        .count()
}

/// The live preview image, for a probe that wants to LOOK at it.
pub(crate) fn preview_image(world: &World) -> Option<Handle<Image>> {
    world
        .get_resource::<PreviewRig>()
        .map(|rig| rig.image.clone())
}
