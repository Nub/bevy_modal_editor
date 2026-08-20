//! Renders the gizmos games register through `editor_api` (spec §7), and gives
//! each one a click target.
//!
//! The editor never learns the game's types: it finds entities carrying the
//! registered component through reflection, hands the draw function a painter
//! plus the component value, and lets the game decide what its own data looks
//! like.

use bevy::prelude::*;
use editor_api::gizmos::{GizmoCx, GizmoPainter, PickProxy};
use editor_core::prelude::*;

/// `GizmoPainter` over bevy's immediate-mode gizmos — the one place that knows
/// how a line actually gets drawn.
struct Painter<'a, 'w, 's> {
    gizmos: &'a mut Gizmos<'w, 's>,
}

impl GizmoPainter for Painter<'_, '_, '_> {
    fn line(&mut self, from: Vec3, to: Vec3, color: Color) {
        self.gizmos.line(from, to, color);
    }
    fn sphere(&mut self, at: Vec3, radius: f32, color: Color) {
        self.gizmos.sphere(at, radius, color);
    }
    fn arrow(&mut self, from: Vec3, to: Vec3, color: Color) {
        self.gizmos.arrow(from, to, color);
    }
    fn cuboid(&mut self, at: Transform, color: Color) {
        self.gizmos.cube(at, color);
    }
    fn circle(&mut self, at: Vec3, normal: Vec3, radius: f32, color: Color) {
        let facing = Quat::from_rotation_arc(Vec3::Z, normal.normalize_or(Vec3::Z));
        self.gizmos
            .circle(Isometry3d::new(at, facing), radius, color);
    }
}

/// Keep drawing feature gizmos after the editor hands the world to the game.
///
/// Normally gizmos are editor furniture and vanish when you press play, which
/// is right: you asked to see the GAME. But a widget with no geometry — a
/// trigger volume above all — is invisible exactly when you are testing
/// whether you can walk into it, and "nothing happened" then has three
/// explanations you cannot tell apart. This toggle keeps them on screen
/// through a play session, the same way collider wireframes work.
#[derive(Resource, Default)]
pub(crate) struct GizmosWhilePlaying(pub bool);

pub(crate) fn toggle_gizmos_while_playing(
    mut reader: MessageReader<ActionInvoked>,
    mut pinned: ResMut<GizmosWhilePlaying>,
    mut feedback: MessageWriter<editor_scene::SceneIoFeedback>,
) {
    for invoked in reader.read() {
        if invoked.action.as_str() == "view.toggle-play-gizmos" {
            pinned.0 = !pinned.0;
            feedback.write(editor_scene::SceneIoFeedback {
                message: if pinned.0 {
                    "Gizmos stay visible while playing".into()
                } else {
                    "Gizmos hide while playing".into()
                },
                success: true,
            });
        }
    }
}

pub(crate) fn draw_feature_gizmos(
    catalog: Res<GizmoCatalog>,
    registry: Res<AppTypeRegistry>,
    state: Res<EditorState>,
    pinned: Res<GizmosWhilePlaying>,
    // `EntityRef` already reads everything, so it cannot share a query with
    // other terms — pull the transform and selection off it instead.
    entities: Query<EntityRef>,
    mut gizmos: Gizmos,
) {
    if (!state.active && !pinned.0) || catalog.gizmos.is_empty() {
        return;
    }
    let registry = registry.read();
    for entity in &entities {
        let Some(global) = entity.get::<GlobalTransform>().copied() else {
            continue;
        };
        let selected = entity.contains::<Selected>();
        for def in &catalog.gizmos {
            let Some(registration) = registry.get(def.component) else {
                continue;
            };
            let Some(reflect_component) =
                registration.data::<bevy::ecs::reflect::ReflectComponent>()
            else {
                continue;
            };
            let Some(value) = reflect_component.reflect(entity) else {
                continue; // this entity does not carry the component
            };
            let mut painter = Painter {
                gizmos: &mut gizmos,
            };
            let mut cx = GizmoCx {
                painter: &mut painter,
                transform: global,
                selected,
                value: value.as_partial_reflect(),
            };
            (def.draw)(&mut cx);
        }
    }
}

/// Marks the invisible pick body a gizmo widget gets, so the click target is
/// rebuilt with the entity rather than leaking.
#[derive(Component)]
pub(crate) struct GizmoPickProxy;

/// On the PARENT once its proxy exists: `EntityRef` reads every component, so
/// this system can hold exactly one query — "already done" has to be a filter
/// on that query rather than a second one.
#[derive(Component)]
pub(crate) struct GizmoPickAttached;

#[derive(Resource, Default)]
pub(crate) struct PickProxyMesh {
    sphere: Option<Handle<Mesh>>,
    cube: Option<Handle<Mesh>>,
}

/// A gizmo-only widget (a spawn point, a trigger volume) has NO mesh, so
/// viewport picking has nothing to hit — you could see it and never click it.
/// This gives it an invisible body to catch the ray, shaped by the
/// registration.
pub(crate) fn attach_gizmo_pick_targets(world: &mut World) {
    if world.resource::<GizmoCatalog>().gizmos.is_empty() {
        return;
    }
    // Exclusive: `EntityRef` reads every component, so it cannot share a system
    // with anything else that touches components. This runs rarely (only for
    // entities without a proxy yet), so the cost is nothing.
    let defs = world.resource::<GizmoCatalog>().gizmos.clone();
    let registry = world.resource::<AppTypeRegistry>().clone();
    let candidates: Vec<Entity> = {
        let mut query =
            world.query_filtered::<Entity, (With<SceneId>, Without<GizmoPickAttached>)>();
        query.iter(world).collect()
    };
    let mut wanted: Vec<(Entity, PickProxy)> = Vec::new();
    {
        let registry = registry.read();
        for entity in candidates {
            let Ok(entity_ref) = world.get_entity(entity) else {
                continue;
            };
            let pick = defs.iter().find_map(|def| {
                let registration = registry.get(def.component)?;
                let reflect_component =
                    registration.data::<bevy::ecs::reflect::ReflectComponent>()?;
                reflect_component.reflect(entity_ref)?;
                def.pick
            });
            if let Some(pick) = pick {
                wanted.push((entity, pick));
            }
        }
    }
    for (entity, pick) in wanted {
        let (mesh, transform) = match pick {
            PickProxy::Sphere { radius } => (
                proxy_mesh(world, false),
                Transform::from_scale(Vec3::splat(radius)),
            ),
            // A unit cube parented to the widget inherits its transform, so the
            // click target IS the box being drawn — at every size, forever,
            // with nothing to keep in sync as the designer scales it.
            PickProxy::UnitBox => (proxy_mesh(world, true), Transform::IDENTITY),
        };
        world.spawn((
            GizmoPickProxy,
            Mesh3d(mesh),
            // No material and no `Visibility::Hidden`. A mesh with no material
            // is drawn by nothing, so it is already invisible — while HIDING it
            // would take it out of picking entirely, because the mesh backend
            // ray-casts `VisibleInView` by default. Hidden proxies are how this
            // mechanism came to exist and never once caught a click.
            transform,
            ChildOf(entity),
        ));
        world.entity_mut(entity).insert(GizmoPickAttached);
    }
}

/// The two proxy meshes, made once and shared by every widget that wants one.
fn proxy_mesh(world: &mut World, cube: bool) -> Handle<Mesh> {
    let cached = {
        let meshes = world.resource::<PickProxyMesh>();
        if cube {
            meshes.cube.clone()
        } else {
            meshes.sphere.clone()
        }
    };
    if let Some(mesh) = cached {
        return mesh;
    }
    let mesh = if cube {
        world
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::from_length(1.0))
    } else {
        world.resource_mut::<Assets<Mesh>>().add(Sphere::new(1.0))
    };
    let mut meshes = world.resource_mut::<PickProxyMesh>();
    if cube {
        meshes.cube = Some(mesh.clone());
    } else {
        meshes.sphere = Some(mesh.clone());
    }
    mesh
}
