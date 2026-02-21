//! CPU-side mesh particle simulation.
//!
//! Mesh particles are typically low-count (debris, rocks, shrapnel — 10-50 particles).
//! Each particle is a real `Mesh3d` child entity. No GPU readback needed.

use avian3d::prelude::*;
use bevy::light::NotShadowCaster;
use bevy::math::Affine2;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::data::*;

// ---------------------------------------------------------------------------
// Components & resources
// ---------------------------------------------------------------------------

/// Container for all mesh emitter states on a VfxSystem entity.
/// One entry per mesh-mode emitter (supports compound effects with multiple mesh emitters).
#[derive(Component, Default)]
pub struct MeshParticleStates {
    pub entries: Vec<MeshParticleState>,
}

/// Per-emitter CPU particle state.
pub struct MeshParticleState {
    /// Which emitter index within the parent VfxSystem this tracks.
    pub emitter_index: usize,
    /// Live particles.
    pub particles: Vec<CpuParticle>,
    /// Fractional spawn accumulator (for Rate mode).
    pub spawn_accumulator: f32,
    /// Burst cycle counter.
    pub burst_cycle: u32,
    /// Burst timer.
    pub burst_timer: f32,
    /// Whether a Once burst has already fired.
    pub once_fired: bool,
    /// Template material for this emitter (cloned per particle on spawn).
    pub material_handle: Option<Handle<StandardMaterial>>,
    /// Per-emitter mesh handle.
    pub mesh_handle: Option<Handle<Mesh>>,
}

/// A single CPU-simulated particle.
pub struct CpuParticle {
    pub entity: Entity,
    pub position: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub lifetime: f32,
    pub scale: Vec3,
    pub initial_scale: Vec3,
    pub color: LinearRgba,
    pub emissive: LinearRgba,
    pub orientation: Quat,
    /// When true, physics engine drives position/velocity — skip manual integration.
    pub physics: bool,
}

/// Cached mesh handles for mesh particles.
/// Materials are per-emitter (stored in `MeshParticleState`), not shared.
#[derive(Resource, Default)]
pub struct MeshParticleAssets {
    pub meshes: HashMap<MeshShapeKey, Handle<Mesh>>,
}

/// Hashable key for built-in mesh shapes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MeshShapeKey {
    Cube,
    Sphere,
    Capsule,
    Cylinder,
    Quad,
    Custom(String),
}

impl From<&MeshShape> for MeshShapeKey {
    fn from(shape: &MeshShape) -> Self {
        match shape {
            MeshShape::Cube => Self::Cube,
            MeshShape::Sphere => Self::Sphere,
            MeshShape::Capsule => Self::Capsule,
            MeshShape::Cylinder => Self::Cylinder,
            MeshShape::Quad => Self::Quad,
            MeshShape::Custom(path) => Self::Custom(path.clone()),
        }
    }
}

/// Marker for child entities spawned by the mesh particle system.
#[derive(Component)]
pub struct MeshParticleChild;

/// Marker for mesh particle children that need a library material applied.
/// The editor resolves this using `apply_material_def_standalone` and removes it.
#[derive(Component)]
pub struct VfxMaterialPending(pub String);

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Auto-insert/update `MeshParticleStates` for VfxSystem entities with mesh emitters.
/// Adds entries for new mesh emitters, invalidates cached handles when config changes.
pub fn auto_insert_mesh_particle_state(
    mut commands: Commands,
    query: Query<(Entity, &VfxSystem), Changed<VfxSystem>>,
    mut existing: Query<&mut MeshParticleStates>,
) {
    for (entity, system) in &query {
        // Collect which emitter indices need mesh state
        let needed: Vec<usize> = system
            .emitters
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e.render, RenderModule::Mesh(_)))
            .map(|(i, _)| i)
            .collect();

        if let Ok(mut states) = existing.get_mut(entity) {
            // Invalidate cached handles on existing entries so next spawn picks up new config
            for state in &mut states.entries {
                if needed.contains(&state.emitter_index) {
                    state.material_handle = None;
                    state.mesh_handle = None;
                }
            }
            // Add entries for newly needed emitters
            for idx in &needed {
                if !states.entries.iter().any(|s| s.emitter_index == *idx) {
                    states.entries.push(MeshParticleState {
                        emitter_index: *idx,
                        particles: Vec::new(),
                        spawn_accumulator: 0.0,
                        burst_cycle: 0,
                        burst_timer: 0.0,
                        once_fired: false,
                        material_handle: None,
                        mesh_handle: None,
                    });
                }
            }
        } else if !needed.is_empty() {
            let entries = needed
                .iter()
                .map(|&idx| MeshParticleState {
                    emitter_index: idx,
                    particles: Vec::new(),
                    spawn_accumulator: 0.0,
                    burst_cycle: 0,
                    burst_timer: 0.0,
                    once_fired: false,
                    material_handle: None,
                    mesh_handle: None,
                })
                .collect();
            commands
                .entity(entity)
                .insert(MeshParticleStates { entries });
        }
    }
}

/// Spawn new mesh particles based on SpawnModule settings.
pub fn cpu_mesh_particle_spawn(
    mut commands: Commands,
    time: Res<Time>,
    mut assets: ResMut<MeshParticleAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut query: Query<(Entity, &VfxSystem, &GlobalTransform, &mut MeshParticleStates)>,
) {
    let dt = time.delta_secs();

    for (parent_entity, system, global_transform, mut states) in &mut query {
        for state in &mut states.entries {
            let Some(emitter) = system.emitters.get(state.emitter_index) else {
                continue;
            };
            if !emitter.enabled {
                continue;
            }
            let RenderModule::Mesh(ref mesh_config) = emitter.render else {
                continue;
            };

            // Compute how many to spawn this frame
            let spawn_count = compute_spawn_count(&emitter.spawn, state, dt);
            if spawn_count == 0 {
                continue;
            }

            // Per-emitter mesh: reuse from state or create a new one
            let mesh_handle = if let Some(ref h) = state.mesh_handle {
                h.clone()
            } else {
                let mesh_key = MeshShapeKey::from(&mesh_config.shape);
                let h = assets
                    .meshes
                    .entry(mesh_key)
                    .or_insert_with(|| {
                        let mesh = match &mesh_config.shape {
                            MeshShape::Cube => Mesh::from(Cuboid::from_size(Vec3::ONE)),
                            MeshShape::Sphere => Mesh::from(Sphere::new(0.5)),
                            MeshShape::Capsule => Mesh::from(Capsule3d::new(0.25, 0.5)),
                            MeshShape::Cylinder => Mesh::from(Cylinder::new(0.25, 1.0)),
                            MeshShape::Quad => {
                                Mesh::from(Plane3d::new(Vec3::Z, Vec2::splat(0.5)))
                            }
                            MeshShape::Custom(_) => Mesh::from(Cuboid::from_size(Vec3::ONE)),
                        };
                        let mesh = mesh
                            .with_generated_tangents()
                            .expect("particle mesh should support tangent generation");
                        meshes.add(mesh)
                    })
                    .clone();
                state.mesh_handle = Some(h.clone());
                h
            };

            // Extract UV scale from init modules (emitter-level)
            let uv_scale = emitter
                .init
                .iter()
                .find_map(|m| match m {
                    InitModule::SetUvScale(s) => Some(Vec2::new(s[0], s[1])),
                    _ => None,
                })
                .unwrap_or(Vec2::ONE);

            // Per-emitter material: each emitter gets its own unique material instance
            // so UV scroll and other per-emitter modifications don't bleed across emitters.
            let use_library_material = mesh_config.material_path.is_some();
            let mat_handle = if let Some(ref h) = state.material_handle {
                h.clone()
            } else if !use_library_material {
                let bevy_alpha = emitter.alpha_mode.to_bevy();
                let h = materials.add(StandardMaterial {
                    base_color: Color::LinearRgba(mesh_config.base_color),
                    uv_transform: Affine2::from_scale(uv_scale),
                    alpha_mode: bevy_alpha,
                    ..default()
                });
                state.material_handle = Some(h.clone());
                h
            } else {
                // Library material: placeholder for UV scroll tracking.
                // Real material applied by VfxMaterialPending system.
                let h = materials.add(StandardMaterial {
                    base_color: Color::LinearRgba(mesh_config.base_color),
                    ..default()
                });
                state.material_handle = Some(h.clone());
                h
            };

            let emitter_pos = global_transform.translation();

            for _ in 0..spawn_count {
                if state.particles.len() >= emitter.capacity as usize {
                    break;
                }

                // Sample init modules
                let mut lifetime = 5.0f32;
                let mut position = Vec3::ZERO;
                let mut velocity = Vec3::ZERO;
                let mut color = mesh_config.base_color;
                let mut scale = Vec3::ONE;
                let mut orientation = Quat::IDENTITY;
                let mut orient_mode = None;

                for init in &emitter.init {
                    match init {
                        InitModule::SetLifetime(range) => lifetime = range.sample(),
                        InitModule::SetPosition(shape) => position = sample_shape(shape),
                        InitModule::SetVelocity(mode) => {
                            velocity = sample_velocity(mode, position)
                        }
                        InitModule::SetColor(source) => {
                            color = match source {
                                ColorSource::Constant(c) => *c,
                                ColorSource::RandomFromGradient(g) => g.sample(fastrand::f32()),
                            };
                        }
                        InitModule::SetSize(range) => {
                            let s = range.sample();
                            scale = Vec3::splat(s);
                        }
                        InitModule::SetScale3d { x, y, z } => {
                            scale = Vec3::new(x.sample(), y.sample(), z.sample());
                        }
                        InitModule::SetRotation(range) => {
                            orientation = Quat::from_rotation_y(range.sample());
                        }
                        InitModule::SetOrientation(mode) => {
                            orient_mode = Some(*mode);
                            orientation = match mode {
                                OrientMode::Identity => Quat::IDENTITY,
                                OrientMode::RandomY => {
                                    Quat::from_rotation_y(
                                        fastrand::f32() * std::f32::consts::TAU,
                                    )
                                }
                                OrientMode::RandomFull => random_quat(),
                                OrientMode::AlignVelocity => Quat::IDENTITY,
                                OrientMode::FaceCamera => Quat::IDENTITY,
                            };
                        }
                        InitModule::SetUvScale(_) => {}
                        InitModule::InheritVelocity { .. } => {}
                    }
                }

                // Deferred: align to velocity after velocity is sampled
                if orient_mode == Some(OrientMode::AlignVelocity) {
                    let dir = velocity.normalize_or_zero();
                    if dir.length_squared() > 0.001 {
                        orientation = Quat::from_rotation_arc(Vec3::Z, dir);
                    }
                }

                // World-space position
                let world_pos = if emitter.sim_space == SimSpace::World {
                    emitter_pos + position
                } else {
                    position
                };

                // Spawn child entity
                let use_physics = mesh_config.collide;
                let mut child_cmd = commands.spawn((
                    MeshParticleChild,
                    Mesh3d(mesh_handle.clone()),
                    Transform::from_translation(world_pos)
                        .with_scale(scale)
                        .with_rotation(orientation),
                ));

                // Each particle gets its own material clone so per-particle UV scroll
                // (based on spawn time) doesn't bleed across particles.
                if let Some(ref mat_name) = mesh_config.material_path {
                    child_cmd.insert(VfxMaterialPending(mat_name.clone()));
                } else {
                    let particle_mat =
                        materials.get(&mat_handle).cloned().unwrap_or_default();
                    let particle_mat_handle = materials.add(particle_mat);
                    child_cmd.insert(MeshMaterial3d(particle_mat_handle));
                }

                if !mesh_config.cast_shadows {
                    child_cmd.insert(NotShadowCaster);
                }

                if use_physics {
                    let collider = match &mesh_config.shape {
                        MeshShape::Cube => Collider::cuboid(scale.x, scale.y, scale.z),
                        MeshShape::Sphere => Collider::sphere(scale.x * 0.5),
                        MeshShape::Capsule => Collider::capsule(scale.x * 0.25, scale.y * 0.5),
                        MeshShape::Cylinder => Collider::cylinder(scale.x * 0.25, scale.y),
                        MeshShape::Quad => Collider::cuboid(scale.x, scale.y, 0.01),
                        MeshShape::Custom(_) => Collider::cuboid(scale.x, scale.y, scale.z),
                    };
                    child_cmd.insert((
                        RigidBody::Dynamic,
                        collider,
                        LinearVelocity(velocity),
                        Restitution::new(mesh_config.restitution),
                    ));
                }

                let child = child_cmd.id();

                if emitter.sim_space == SimSpace::Local {
                    commands.entity(parent_entity).add_child(child);
                }

                state.particles.push(CpuParticle {
                    entity: child,
                    position: world_pos,
                    velocity,
                    age: 0.0,
                    lifetime,
                    scale,
                    initial_scale: scale,
                    color,
                    emissive: LinearRgba::BLACK,
                    orientation,
                    physics: use_physics,
                });
            }
        }
    }
}

/// Update particles: apply UpdateModule effects, advance age, kill expired.
pub fn cpu_mesh_particle_update(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(&VfxSystem, &mut MeshParticleStates)>,
) {
    let dt = time.delta_secs();

    for (system, mut states) in &mut query {
        for state in &mut states.entries {
            let Some(emitter) = system.emitters.get(state.emitter_index) else {
                continue;
            };

            let mut dead = Vec::new();

            for (i, p) in state.particles.iter_mut().enumerate() {
                p.age += dt;

                if p.age >= p.lifetime {
                    dead.push(i);
                    continue;
                }

                let t = p.age / p.lifetime;

                for update in &emitter.update {
                    match update {
                        UpdateModule::Gravity(g) if !p.physics => {
                            p.velocity += *g * dt;
                        }
                        UpdateModule::ConstantForce(f) if !p.physics => {
                            p.velocity += *f * dt;
                        }
                        UpdateModule::Drag(d) if !p.physics => {
                            p.velocity *= (1.0 - d * dt).max(0.0);
                        }
                        UpdateModule::Noise {
                            strength,
                            frequency,
                            scroll,
                        } if !p.physics => {
                            let phase = p.position * *frequency + *scroll * p.age;
                            let noise = Vec3::new(
                                (phase.x * 12.9898 + phase.y * 78.233).sin(),
                                (phase.y * 12.9898 + phase.z * 78.233).sin(),
                                (phase.z * 12.9898 + phase.x * 78.233).sin(),
                            );
                            p.velocity += noise * *strength * dt;
                        }
                        UpdateModule::OrbitAround {
                            axis,
                            speed,
                            radius_decay,
                        } if !p.physics => {
                            let to_center = -p.position;
                            let r = to_center.length();
                            if r > 0.001 {
                                let tangent = axis.cross(to_center).normalize_or_zero();
                                p.velocity += tangent * *speed * dt;
                                if *radius_decay > 0.0 {
                                    p.velocity +=
                                        to_center.normalize() * *radius_decay * dt;
                                }
                            }
                        }
                        UpdateModule::Attract {
                            target,
                            strength,
                            falloff,
                        } if !p.physics => {
                            let dir = *target - p.position;
                            let dist = dir.length();
                            if dist > 0.001 {
                                let force = *strength / (1.0 + dist.powf(*falloff));
                                p.velocity += dir.normalize() * force * dt;
                            }
                        }
                        UpdateModule::TangentAccel {
                            origin,
                            axis,
                            accel,
                        } if !p.physics => {
                            let to_origin = *origin - p.position;
                            let tangent = axis.cross(to_origin).normalize_or_zero();
                            p.velocity += tangent * *accel * dt;
                        }
                        UpdateModule::RadialAccel { origin, accel } if !p.physics => {
                            let dir = (p.position - *origin).normalize_or_zero();
                            p.velocity += dir * *accel * dt;
                        }
                        UpdateModule::KillZone { shape, invert } => {
                            let inside = match shape {
                                KillShape::Sphere { center, radius } => {
                                    p.position.distance(*center) < *radius
                                }
                                KillShape::Box {
                                    center,
                                    half_extents,
                                } => {
                                    let d = (p.position - *center).abs();
                                    d.x < half_extents.x
                                        && d.y < half_extents.y
                                        && d.z < half_extents.z
                                }
                            };
                            let should_kill = if *invert { !inside } else { inside };
                            if should_kill {
                                p.age = p.lifetime;
                                dead.push(i);
                            }
                        }
                        UpdateModule::SizeByLife(curve) => {
                            let factor = curve.sample(t);
                            p.scale = p.initial_scale * factor;
                        }
                        UpdateModule::Scale3dByLife { x, y, z } => {
                            p.scale = Vec3::new(
                                p.initial_scale.x * x.sample(t),
                                p.initial_scale.y * y.sample(t),
                                p.initial_scale.z * z.sample(t),
                            );
                        }
                        UpdateModule::OffsetByLife { x, y, z } if !p.physics => {
                            let offset = Vec3::new(x.sample(t), y.sample(t), z.sample(t));
                            p.position += offset * dt;
                        }
                        UpdateModule::ColorByLife(gradient) => {
                            p.color = gradient.sample(t);
                        }
                        UpdateModule::EmissiveOverLife(gradient) => {
                            p.emissive = gradient.sample(t);
                        }
                        UpdateModule::SizeBySpeed {
                            min_speed,
                            max_speed,
                            min_size,
                            max_size,
                        } => {
                            let speed = p.velocity.length();
                            let frac = ((speed - min_speed) / (max_speed - min_speed))
                                .clamp(0.0, 1.0);
                            let s = *min_size + (*max_size - *min_size) * frac;
                            p.scale = Vec3::splat(s);
                        }
                        UpdateModule::RotateByVelocity => {}
                        UpdateModule::Spin { axis, speed } => {
                            let norm = axis.normalize_or_zero();
                            if norm.length_squared() > 0.001 {
                                p.orientation =
                                    Quat::from_axis_angle(norm, *speed * dt) * p.orientation;
                            }
                        }
                        UpdateModule::UvScroll { .. } => {}
                        _ => {}
                    }
                }

                if !p.physics {
                    p.position += p.velocity * dt;
                }
            }

            dead.sort_unstable();
            dead.dedup();
            for &i in dead.iter().rev() {
                let p = state.particles.remove(i);
                commands.entity(p.entity).try_despawn();
            }
        }
    }
}

/// Sync particle state to Transform and material on child entities.
pub fn cpu_mesh_particle_sync(
    mut query: Query<(&VfxSystem, &mut MeshParticleStates)>,
    mut transforms: Query<&mut Transform>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
) {
    let camera_pos = camera.iter().next().map(|gt| gt.translation());

    for (system, mut states) in &mut query {
        for state in &mut states.entries {
            let Some(emitter) = system.emitters.get(state.emitter_index) else {
                continue;
            };

            let orient_mode = emitter.init.iter().find_map(|m| match m {
                InitModule::SetOrientation(mode) => Some(*mode),
                _ => None,
            });
            let align_to_velocity = orient_mode == Some(OrientMode::AlignVelocity);
            let face_camera = orient_mode == Some(OrientMode::FaceCamera);
            let has_rotate_by_vel = emitter
                .update
                .iter()
                .any(|u| matches!(u, UpdateModule::RotateByVelocity));

            for p in state.particles.iter_mut() {
                if let Ok(mut transform) = transforms.get_mut(p.entity) {
                    if p.physics {
                        p.position = transform.translation;
                        transform.scale = p.scale;
                    } else {
                        transform.translation = p.position;
                        transform.scale = p.scale;

                        if face_camera {
                            if let Some(cam_pos) = camera_pos {
                                let dir = (cam_pos - p.position).normalize_or_zero();
                                if dir.length_squared() > 0.001 {
                                    transform.rotation =
                                        Quat::from_rotation_arc(Vec3::Z, dir);
                                }
                            }
                        } else if align_to_velocity || has_rotate_by_vel {
                            let dir = p.velocity.normalize_or_zero();
                            if dir.length_squared() > 0.001 {
                                transform.rotation = Quat::from_rotation_arc(Vec3::Z, dir);
                            }
                        } else {
                            transform.rotation = p.orientation;
                        }
                    }
                }
            }
        }
    }
}

/// Animate UV scrolling on mesh particle materials (non-library StandardMaterial).
/// Each particle's UV offset is based on its age (time since spawn).
pub fn cpu_mesh_particle_uv_scroll(
    query: Query<(&VfxSystem, &MeshParticleStates)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    particle_mats: Query<&MeshMaterial3d<StandardMaterial>, With<MeshParticleChild>>,
) {
    for (system, states) in &query {
        for state in &states.entries {
            let Some(emitter) = system.emitters.get(state.emitter_index) else {
                continue;
            };
            if let RenderModule::Mesh(ref config) = emitter.render {
                if config.material_path.is_some() {
                    continue;
                }
            }

            let scroll_speed = emitter
                .update
                .iter()
                .find_map(|m| match m {
                    UpdateModule::UvScroll { speed } => Some(Vec2::new(speed[0], speed[1])),
                    _ => None,
                })
                .unwrap_or(Vec2::ZERO);

            if scroll_speed == Vec2::ZERO {
                continue;
            }

            let uv_scale = emitter
                .init
                .iter()
                .find_map(|m| match m {
                    InitModule::SetUvScale(s) => Some(Vec2::new(s[0], s[1])),
                    _ => None,
                })
                .unwrap_or(Vec2::ONE);

            for p in &state.particles {
                let offset = scroll_speed * p.age;
                if let Ok(mat_comp) = particle_mats.get(p.entity) {
                    if let Some(mat) = materials.get_mut(&mat_comp.0) {
                        mat.uv_transform = Affine2::from_scale_angle_translation(
                            uv_scale, 0.0, offset,
                        );
                    }
                }
            }
        }
    }
}

/// Sync particle color and emissive to mesh particle materials.
/// Applies ColorByLife → base_color and EmissiveOverLife → emissive on each particle's material.
pub fn cpu_mesh_particle_color_sync(
    query: Query<(&VfxSystem, &MeshParticleStates)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    particle_mats: Query<&MeshMaterial3d<StandardMaterial>, With<MeshParticleChild>>,
) {
    for (system, states) in &query {
        for state in &states.entries {
            let Some(emitter) = system.emitters.get(state.emitter_index) else {
                continue;
            };
            if let RenderModule::Mesh(ref config) = emitter.render {
                if config.material_path.is_some() {
                    continue;
                }
            }

            let has_color_by_life = emitter
                .update
                .iter()
                .any(|m| matches!(m, UpdateModule::ColorByLife(_)));
            let has_emissive_by_life = emitter
                .update
                .iter()
                .any(|m| matches!(m, UpdateModule::EmissiveOverLife(_)));

            if !has_color_by_life && !has_emissive_by_life {
                continue;
            }

            for p in &state.particles {
                if let Ok(mat_comp) = particle_mats.get(p.entity) {
                    if let Some(mat) = materials.get_mut(&mat_comp.0) {
                        if has_color_by_life {
                            mat.base_color = Color::LinearRgba(p.color);
                        }
                        if has_emissive_by_life {
                            mat.emissive = p.emissive;
                        }
                    }
                }
            }
        }
    }
}

/// Cleanup: despawn all mesh particle children when VfxSystem is removed or
/// when the emitter is switched away from Mesh mode.
pub fn cpu_mesh_particle_cleanup(
    mut commands: Commands,
    mut removed: RemovedComponents<VfxSystem>,
    mut query: Query<(Entity, &VfxSystem, &mut MeshParticleStates)>,
) {
    // Handle removed VfxSystem entities
    for entity in removed.read() {
        commands.entity(entity).remove::<MeshParticleStates>();
    }

    // Handle emitters that are no longer mesh mode
    for (entity, system, mut states) in &mut query {
        states.entries.retain_mut(|state| {
            let still_mesh = system
                .emitters
                .get(state.emitter_index)
                .map(|e| matches!(e.render, RenderModule::Mesh(_)))
                .unwrap_or(false);

            if !still_mesh {
                for p in state.particles.drain(..) {
                    commands.entity(p.entity).try_despawn();
                }
                false // remove this entry
            } else {
                true
            }
        });

        if states.entries.is_empty() {
            commands.entity(entity).remove::<MeshParticleStates>();
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn compute_spawn_count(spawn: &SpawnModule, state: &mut MeshParticleState, dt: f32) -> u32 {
    match spawn {
        SpawnModule::Rate(rate) => {
            state.spawn_accumulator += rate * dt;
            let count = state.spawn_accumulator as u32;
            state.spawn_accumulator -= count as f32;
            count
        }
        SpawnModule::Burst {
            count,
            interval,
            max_cycles,
            offset,
        } => {
            if let Some(max) = max_cycles {
                if state.burst_cycle >= *max {
                    return 0;
                }
            }
            state.burst_timer += dt;
            if state.burst_cycle == 0 {
                // First burst fires as soon as offset elapses
                if state.burst_timer >= *offset {
                    state.burst_timer -= *offset;
                    state.burst_cycle += 1;
                    *count
                } else {
                    0
                }
            } else {
                // Subsequent bursts fire every interval
                if state.burst_timer >= *interval {
                    state.burst_timer -= *interval;
                    state.burst_cycle += 1;
                    *count
                } else {
                    0
                }
            }
        }
        SpawnModule::Once { count, offset } => {
            if state.once_fired {
                0
            } else {
                state.burst_timer += dt;
                if state.burst_timer >= *offset {
                    state.once_fired = true;
                    *count
                } else {
                    0
                }
            }
        }
        SpawnModule::Distance { .. } => 0,
    }
}

fn sample_shape(shape: &ShapeEmitter) -> Vec3 {
    match shape {
        ShapeEmitter::Point(p) => *p,
        ShapeEmitter::Sphere { center, radius } => {
            let r = radius.sample();
            let dir = random_unit_sphere();
            *center + dir * r
        }
        ShapeEmitter::Box {
            center,
            half_extents,
        } => {
            let x = (fastrand::f32() * 2.0 - 1.0) * half_extents.x;
            let y = (fastrand::f32() * 2.0 - 1.0) * half_extents.y;
            let z = (fastrand::f32() * 2.0 - 1.0) * half_extents.z;
            *center + Vec3::new(x, y, z)
        }
        ShapeEmitter::Circle {
            center,
            axis,
            radius,
        } => {
            let r = radius.sample();
            let angle = fastrand::f32() * std::f32::consts::TAU;
            let up = if axis.y.abs() < 0.99 {
                Vec3::Y
            } else {
                Vec3::X
            };
            let tangent = axis.cross(up).normalize();
            let bitangent = axis.cross(tangent).normalize();
            *center + (tangent * angle.cos() + bitangent * angle.sin()) * r
        }
        ShapeEmitter::Edge { start, end } => {
            let t = fastrand::f32();
            *start + (*end - *start) * t
        }
        ShapeEmitter::Cone {
            angle,
            radius,
            height,
        } => {
            let a = fastrand::f32() * std::f32::consts::TAU;
            let h = fastrand::f32() * *height;
            let r = *radius * (h / height.max(0.001)) * angle.tan();
            Vec3::new(a.cos() * r, h, a.sin() * r)
        }
    }
}

fn sample_velocity(mode: &VelocityMode, position: Vec3) -> Vec3 {
    match mode {
        VelocityMode::Radial { center, speed } => {
            let dir = (position - *center).normalize_or_zero();
            let dir = if dir.length_squared() < 0.001 {
                random_unit_sphere()
            } else {
                dir
            };
            dir * speed.sample()
        }
        VelocityMode::Directional { direction, speed } => {
            direction.normalize_or_zero() * speed.sample()
        }
        VelocityMode::Tangent { axis, speed } => {
            let tangent = axis.cross(position).normalize_or_zero();
            let tangent = if tangent.length_squared() < 0.001 {
                random_unit_sphere()
            } else {
                tangent
            };
            tangent * speed.sample()
        }
        VelocityMode::Cone {
            direction,
            angle,
            speed,
        } => {
            let dir = random_cone(*direction, *angle);
            dir * speed.sample()
        }
        VelocityMode::Random { speed } => random_unit_sphere() * speed.sample(),
    }
}

fn random_unit_sphere() -> Vec3 {
    loop {
        let v = Vec3::new(
            fastrand::f32() * 2.0 - 1.0,
            fastrand::f32() * 2.0 - 1.0,
            fastrand::f32() * 2.0 - 1.0,
        );
        let len_sq = v.length_squared();
        if len_sq > 0.001 && len_sq <= 1.0 {
            return v / len_sq.sqrt();
        }
    }
}

fn random_quat() -> Quat {
    loop {
        let q = Quat::from_xyzw(
            fastrand::f32() * 2.0 - 1.0,
            fastrand::f32() * 2.0 - 1.0,
            fastrand::f32() * 2.0 - 1.0,
            fastrand::f32() * 2.0 - 1.0,
        );
        let len_sq = q.length_squared();
        if len_sq > 0.001 {
            return q.normalize();
        }
    }
}

fn random_cone(direction: Vec3, half_angle: f32) -> Vec3 {
    let dir = direction.normalize_or_zero();
    if dir.length_squared() < 0.001 {
        return random_unit_sphere();
    }

    let up = if dir.y.abs() < 0.99 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let right = dir.cross(up).normalize();
    let up2 = right.cross(dir).normalize();

    let angle = fastrand::f32() * std::f32::consts::TAU;
    let cos_theta = 1.0 - fastrand::f32() * (1.0 - half_angle.cos());
    let sin_theta = (1.0 - cos_theta * cos_theta).sqrt();

    (dir * cos_theta + right * sin_theta * angle.cos() + up2 * sin_theta * angle.sin()).normalize()
}
