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

pub(crate) fn on_socket_added(
    add: On<bevy::ecs::lifecycle::Add, Socket>,
    mut assets: ResMut<SocketGizmoAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
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
    mut gizmos: Query<&mut Visibility, With<SocketGizmo>>,
) {
    for mut visibility in &mut gizmos {
        let target = if state.active {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != target {
            *visibility = target;
        }
    }
}
