//! Socket gizmos (M4-D9): every `Socket` entity shows a small cyan cone along
//! its +Z mating direction — editor-only, never pickable, never serialized
//! (no SceneId on the marker child).

use bevy::prelude::*;
use editor_core::prelude::*;
use editor_prefabs::sockets::Socket;

#[derive(Component)]
pub(crate) struct SocketGizmo;

#[derive(Resource, Default)]
pub(crate) struct SocketGizmoAssets(Option<(Handle<Mesh>, Handle<StandardMaterial>)>);

/// Idempotent on purpose: a socket that gets its component re-added must not
/// grow a second cone. Nothing should be doing that — but something was, and
/// the visible result was a heap of stacked cones nobody could click through.
pub(crate) fn on_socket_added(
    add: On<bevy::ecs::lifecycle::Add, Socket>,
    mut assets: ResMut<SocketGizmoAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<&Children>,
    gizmos: Query<(), With<SocketGizmo>>,
    mut commands: Commands,
) {
    if let Ok(children) = existing.get(add.entity)
        && children.iter().any(|child| gizmos.contains(child))
    {
        return;
    }
    let (mesh, material) = assets
        .0
        .get_or_insert_with(|| {
            (
                meshes.add(Cone {
                    radius: 0.12,
                    height: 0.3,
                }),
                materials.add(StandardMaterial {
                    base_color: Color::srgba(0.3, 0.9, 0.9, 0.85),
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                }),
            )
        })
        .clone();
    // The socket itself needs the visibility components, or the chain BREAKS
    // here: propagation only descends through entities that carry them, so a
    // cone under a bare socket entity is never visited and keeps whatever it
    // was born with — which is nothing. The cone used to be
    // `Visibility::Visible`, unconditional, so it drew anyway; it is
    // `Inherited` now so it can follow a hidden piece, and that only works if
    // every link in the chain exists.
    commands
        .entity(add.entity)
        .insert_if_new(Visibility::Inherited);
    commands.entity(add.entity).with_children(|socket| {
        socket.spawn((
            SocketGizmo,
            Mesh3d(mesh),
            MeshMaterial3d(material),
            // Cone points +Y natively; the mating direction is +Z.
            Transform::from_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            // The cone is the socket's ONLY visible geometry, so it has to be
            // its click target too — a socket entity carries no mesh of its
            // own, and picking resolves a hit to the nearest `SceneId`
            // ancestor, which is the socket. Selection is already gated on the
            // editor being active, so this never leaks into play.
            Visibility::Hidden,
        ));
    });
}

/// Editor-only visibility (grid discipline).
pub(crate) fn sync_socket_gizmos(
    state: Res<EditorState>,
    cones: Query<Entity, With<SocketGizmo>>,
    parents: Query<&ChildOf>,
    has_visibility: Query<(), With<Visibility>>,
    mut gizmos: Query<&mut Visibility, With<SocketGizmo>>,
    mut commands: Commands,
) {
    // Fill the visibility chain ABOVE every cone, every frame.
    //
    // Propagation only recurses through children that carry the components,
    // so one bare link — a socket entity, a group that is just a transform —
    // orphans everything below it, and the cone then keeps whatever value it
    // was born with rather than what the piece says. It has to be a system
    // and not the add-observer: a socket is spawned and REPARENTED in the
    // same transaction, so at `Add<Socket>` time there is no chain to walk.
    for cone in &cones {
        let mut current = cone;
        while let Ok(parent) = parents.get(current) {
            let parent = parent.parent();
            if has_visibility.get(parent).is_err() {
                commands.entity(parent).try_insert(Visibility::Inherited);
            }
            current = parent;
        }
    }
    for mut visibility in &mut gizmos {
        let target = if state.active {
            // INHERITED, not Visible: `Visibility::Visible` is unconditional,
            // so a cone on a hidden piece would keep drawing while its piece
            // was gone. Inherited makes the cone follow what it belongs to.
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != target {
            *visibility = target;
        }
    }
}

/// A socket that stops being one takes its cone with it.
pub(crate) fn on_socket_removed(
    remove: On<bevy::ecs::lifecycle::Remove, Socket>,
    children: Query<&Children>,
    gizmos: Query<(), With<SocketGizmo>>,
    mut commands: Commands,
) {
    let Ok(kids) = children.get(remove.entity) else {
        return;
    };
    for child in kids.iter() {
        if gizmos.contains(child) {
            commands.entity(child).despawn();
        }
    }
}
