//! Renders the gizmos games register through `editor_api` (spec §7), and gives
//! each one a click target.
//!
//! The editor never learns the game's types: it finds entities carrying the
//! registered component through reflection, hands the draw function a painter
//! plus the component value, and lets the game decide what its own data looks
//! like.

use bevy::prelude::*;
use editor_api::gizmos::{GizmoCx, GizmoPainter};
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

pub(crate) fn draw_feature_gizmos(
    catalog: Res<GizmoCatalog>,
    registry: Res<AppTypeRegistry>,
    state: Res<EditorState>,
    // `EntityRef` already reads everything, so it cannot share a query with
    // other terms — pull the transform and selection off it instead.
    entities: Query<EntityRef>,
    mut gizmos: Gizmos,
) {
    if !state.active || catalog.gizmos.is_empty() {
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

/// Marks the invisible pick sphere a gizmo widget gets, so the click target is
/// rebuilt with the entity rather than leaking.
#[derive(Component)]
pub(crate) struct GizmoPickProxy;

/// On the PARENT once its proxy exists: `EntityRef` reads every component, so
/// this system can hold exactly one query — "already done" has to be a filter
/// on that query rather than a second one.
#[derive(Component)]
pub(crate) struct GizmoPickAttached;

#[derive(Resource, Default)]
pub(crate) struct PickProxyMesh(Option<Handle<Mesh>>);

/// A gizmo-only widget (a spawn point, a light) has NO mesh, so viewport
/// picking has nothing to hit — you could see it and never click it. This gives
/// it an invisible sphere to catch the ray, sized by the registration.
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
    let mut wanted: Vec<(Entity, f32)> = Vec::new();
    {
        let registry = registry.read();
        for entity in candidates {
            let Ok(entity_ref) = world.get_entity(entity) else {
                continue;
            };
            let radius = defs.iter().find_map(|def| {
                let registration = registry.get(def.component)?;
                let reflect_component =
                    registration.data::<bevy::ecs::reflect::ReflectComponent>()?;
                reflect_component.reflect(entity_ref)?;
                def.pick_radius
            });
            if let Some(radius) = radius {
                wanted.push((entity, radius));
            }
        }
    }
    if wanted.is_empty() {
        return;
    }
    let mesh = {
        let cached = world.resource::<PickProxyMesh>().0.clone();
        match cached {
            Some(mesh) => mesh,
            None => {
                let mesh = world.resource_mut::<Assets<Mesh>>().add(Sphere::new(1.0));
                world.resource_mut::<PickProxyMesh>().0 = Some(mesh.clone());
                mesh
            }
        }
    };
    for (entity, radius) in wanted {
        world.spawn((
            GizmoPickProxy,
            Mesh3d(mesh.clone()),
            // No material: invisible to the eye, solid to the raycast.
            Transform::from_scale(Vec3::splat(radius)),
            Visibility::Hidden,
            ChildOf(entity),
        ));
        world.entity_mut(entity).insert(GizmoPickAttached);
    }
}
