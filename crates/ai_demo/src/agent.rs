//! Runtime agent logic: spawn agents on play, move them toward waypoints.

use bevy::prelude::*;
use bevy::render::view::Hdr;
use bevy_editor_game::{GameCamera, GameEntity, GameStartedEvent};
use bevy_landmass::prelude::*;
use bevy_landmass::Agent3d;
use bevy_modal_editor::navigation::NavmeshState;

use crate::{SpawnPoint, Waypoint};

pub struct AgentPlugin;

impl Plugin for AgentPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                spawn_agents_on_play,
                move_agents.after(LandmassSystems::Output),
                retarget_arrived_agents.after(LandmassSystems::Output),
                draw_agent_path_gizmo,
            ),
        );
    }
}

/// On GameStartedEvent, spawn a game camera and agents at each SpawnPoint.
fn spawn_agents_on_play(
    mut started: MessageReader<GameStartedEvent>,
    spawn_points: Query<&GlobalTransform, With<SpawnPoint>>,
    waypoints: Query<(Entity, &GlobalTransform), With<Waypoint>>,
    archipelagos: Query<Entity, With<Archipelago3d>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for _ in started.read() {
        info!("GameStartedEvent received — spawning game camera and agents");

        // Compute bounding center of all scene geometry for camera framing
        let spawn_positions: Vec<Vec3> = spawn_points.iter().map(|t| t.translation()).collect();
        let all_positions: Vec<Vec3> = spawn_points
            .iter()
            .map(|t| t.translation())
            .chain(waypoints.iter().map(|(_, t)| t.translation()))
            .collect();
        let focus = if all_positions.is_empty() {
            Vec3::ZERO
        } else {
            let min = all_positions.iter().copied().reduce(Vec3::min).unwrap();
            let max = all_positions.iter().copied().reduce(Vec3::max).unwrap();
            (min + max) * 0.5
        };

        // Spawn a top-down game camera so the whole level is visible in play mode
        commands.spawn((
            GameCamera,
            GameEntity,
            Camera3d::default(),
            Projection::Perspective(PerspectiveProjection {
                fov: 70.0_f32.to_radians(),
                ..default()
            }),
            Camera {
                is_active: true,
                order: 1,
                ..default()
            },
            Hdr,
            Transform::from_translation(focus + Vec3::new(0.0, 35.0, 25.0))
                .looking_at(focus, Vec3::Y),
        ));
        info!("Game camera spawned, focusing on {:?}", focus);

        let Ok(archipelago_entity) = archipelagos.single() else {
            warn!("No Archipelago found — generate a navmesh first (AI mode → Generate)");
            continue;
        };

        // Find nearest waypoint for targeting
        let waypoint_positions: Vec<(Entity, Vec3)> = waypoints
            .iter()
            .map(|(e, t)| (e, t.translation()))
            .collect();

        if waypoint_positions.is_empty() {
            warn!("No Waypoint entities found — add some in Insert mode");
            continue;
        }

        let agent_mesh = meshes.add(Capsule3d::new(0.4, 1.2));
        let agent_material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.8, 1.0),
            emissive: bevy::color::LinearRgba::new(0.1, 0.4, 0.6, 1.0),
            ..default()
        });

        for spawn_transform in &spawn_points {
            let pos = spawn_transform.translation();

            // Find nearest waypoint
            let (target_entity, _) = waypoint_positions
                .iter()
                .min_by(|(_, a), (_, b)| {
                    a.distance(pos).partial_cmp(&b.distance(pos)).unwrap()
                })
                .unwrap();

            info!("Spawning AI agent at {:?}", pos);
            commands.spawn((
                Name::new("AI Agent"),
                GameEntity,
                RuntimeAgent,
                Transform::from_translation(pos),
                Visibility::default(),
                Mesh3d(agent_mesh.clone()),
                MeshMaterial3d(agent_material.clone()),
                Agent3dBundle {
                    agent: Agent3d::default(),
                    settings: AgentSettings {
                        radius: 0.6,
                        desired_speed: 3.0,
                        max_speed: 5.0,
                    },
                    archipelago_ref: ArchipelagoRef3d::new(archipelago_entity),
                },
                AgentTarget3d::Entity(*target_entity),
            ));
        }

        info!(
            "Spawned {} AI agents, {} waypoints available",
            spawn_positions.len(),
            waypoint_positions.len()
        );
    }
}

/// Marker for runtime-spawned agents (not the editor placeholder).
#[derive(Component)]
struct RuntimeAgent;

/// Read desired velocity from landmass and apply to transform.
fn move_agents(
    time: Res<Time>,
    mut agents: Query<(&mut Transform, &AgentDesiredVelocity3d), With<RuntimeAgent>>,
) {
    for (mut transform, desired_vel) in &mut agents {
        let vel = desired_vel.velocity();
        if vel.length_squared() > 0.001 {
            transform.translation += vel * time.delta_secs();
            // Face movement direction
            transform.look_to(-vel.normalize(), Vec3::Y);
        }
    }
}

/// When an agent reaches its target, pick a random point on the navmesh as the new target.
fn retarget_arrived_agents(
    mut agents: Query<(&GlobalTransform, &mut AgentTarget3d), With<RuntimeAgent>>,
    transforms: Query<&GlobalTransform>,
    nav_state: Res<NavmeshState>,
) {
    if nav_state.wireframe_polygons.is_empty() {
        return;
    }

    let arrival_threshold = 1.5;

    for (agent_tf, mut target) in &mut agents {
        let agent_pos = agent_tf.translation();

        // Get target position
        let target_pos = match &*target {
            AgentTarget3d::Entity(e) => {
                if let Ok(t) = transforms.get(*e) {
                    t.translation()
                } else {
                    continue;
                }
            }
            AgentTarget3d::Point(p) => *p,
            AgentTarget3d::None => continue,
        };

        // Check if arrived
        let dist = agent_pos.distance(target_pos);
        if dist > arrival_threshold {
            continue;
        }

        // Pick a random point on the navmesh
        if let Some(point) = random_navmesh_point(&nav_state.wireframe_polygons) {
            *target = AgentTarget3d::Point(point);
        }
    }
}

/// Pick the centroid of a random navmesh polygon.
/// Centroids are always inside convex polygons, so this guarantees a valid on-mesh target.
fn random_navmesh_point(polygons: &[Vec<Vec3>]) -> Option<Vec3> {
    if polygons.is_empty() {
        return None;
    }

    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();

    let poly_idx = (seed as usize) % polygons.len();
    let poly = &polygons[poly_idx];

    // Exclude closure vertex (last == first). Need at least 3 unique vertices.
    let n = poly.len().saturating_sub(1);
    if n < 3 {
        return None;
    }

    let centroid = poly[..n].iter().copied().sum::<Vec3>() / n as f32;
    Some(centroid)
}

/// Draw debug line from agent to its target.
fn draw_agent_path_gizmo(
    agents: Query<(&GlobalTransform, &AgentTarget3d), With<RuntimeAgent>>,
    transforms: Query<&GlobalTransform>,
    mut gizmos: Gizmos,
) {
    for (agent_transform, target) in &agents {
        let from = agent_transform.translation();
        let to = match target {
            AgentTarget3d::Point(p) => *p,
            AgentTarget3d::Entity(e) => {
                if let Ok(t) = transforms.get(*e) {
                    t.translation()
                } else {
                    continue;
                }
            }
            AgentTarget3d::None => continue,
        };
        gizmos.line(from, to, Color::srgba(0.2, 0.6, 1.0, 0.5));
    }
}
