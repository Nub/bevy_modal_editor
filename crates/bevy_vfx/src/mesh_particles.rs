//! CPU-side mesh particle simulation.
//!
//! Mesh particles are typically low-count (debris, rocks, shrapnel — 10-50 particles).
//! Each particle is a real `Mesh3d` child entity. No GPU readback needed.

use avian3d::prelude::*;
use bevy::math::Affine2;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::data::*;

// ---------------------------------------------------------------------------
// Components & resources
// ---------------------------------------------------------------------------

/// Per-emitter CPU particle state. Auto-inserted on entities with mesh emitters.
#[derive(Component)]
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
    /// Shared material handle for UV scroll updates.
    pub material_handle: Option<Handle<StandardMaterial>>,
    /// Accumulated UV scroll offset.
    pub uv_scroll_offset: Vec2,
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
    pub orientation: Quat,
    /// When true, physics engine drives position/velocity — skip manual integration.
    pub physics: bool,
}

/// Cached mesh and material handles for mesh particles.
#[derive(Resource, Default)]
pub struct MeshParticleAssets {
    pub meshes: HashMap<MeshShapeKey, Handle<Mesh>>,
    pub materials: HashMap<u64, Handle<StandardMaterial>>,
    /// Materials created from the editor's material library, keyed by name.
    pub named_materials: HashMap<String, Handle<StandardMaterial>>,
}

/// Hashable key for built-in mesh shapes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MeshShapeKey {
    Cube,
    Sphere,
    Capsule,
    Cylinder,
    Custom(String),
}

impl From<&MeshShape> for MeshShapeKey {
    fn from(shape: &MeshShape) -> Self {
        match shape {
            MeshShape::Cube => Self::Cube,
            MeshShape::Sphere => Self::Sphere,
            MeshShape::Capsule => Self::Capsule,
            MeshShape::Cylinder => Self::Cylinder,
            MeshShape::Custom(path) => Self::Custom(path.clone()),
        }
    }
}

/// Marker for child entities spawned by the mesh particle system.
#[derive(Component)]
pub struct MeshParticleChild;

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Auto-insert `MeshParticleState` for VfxSystem entities with mesh emitters.
pub fn auto_insert_mesh_particle_state(
    mut commands: Commands,
    query: Query<(Entity, &VfxSystem), Changed<VfxSystem>>,
    existing: Query<&MeshParticleState>,
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

        // Check which ones already have state
        let has_state: Vec<usize> = existing
            .iter_many(std::iter::once(entity))
            .map(|s| s.emitter_index)
            .collect();

        for idx in needed {
            if !has_state.contains(&idx) {
                if existing.get(entity).is_err() {
                    commands.entity(entity).insert(MeshParticleState {
                        emitter_index: idx,
                        particles: Vec::new(),
                        spawn_accumulator: 0.0,
                        burst_cycle: 0,
                        burst_timer: 0.0,
                        once_fired: false,
                        material_handle: None,
                        uv_scroll_offset: Vec2::ZERO,
                    });
                }
            }
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
    mut query: Query<(Entity, &VfxSystem, &GlobalTransform, &mut MeshParticleState)>,
) {
    let dt = time.delta_secs();

    for (parent_entity, system, global_transform, mut state) in &mut query {
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
        let spawn_count = compute_spawn_count(&emitter.spawn, &mut state, dt);
        if spawn_count == 0 {
            continue;
        }

        // Get or create mesh handle
        let mesh_key = MeshShapeKey::from(&mesh_config.shape);
        let mesh_handle = assets
            .meshes
            .entry(mesh_key.clone())
            .or_insert_with(|| {
                let mesh = match &mesh_config.shape {
                    MeshShape::Cube => Mesh::from(Cuboid::from_size(Vec3::ONE)),
                    MeshShape::Sphere => Mesh::from(Sphere::new(0.5)),
                    MeshShape::Capsule => Mesh::from(Capsule3d::new(0.25, 0.5)),
                    MeshShape::Cylinder => Mesh::from(Cylinder::new(0.25, 1.0)),
                    MeshShape::Custom(_) => Mesh::from(Cuboid::from_size(Vec3::ONE)),
                };
                let mesh = mesh
                    .with_generated_tangents()
                    .expect("particle mesh should support tangent generation");
                meshes.add(mesh)
            })
            .clone();

        // Extract UV scale from init modules (emitter-level)
        let uv_scale = emitter
            .init
            .iter()
            .find_map(|m| match m {
                InitModule::SetUvScale(s) => Some(Vec2::new(s[0], s[1])),
                _ => None,
            })
            .unwrap_or(Vec2::ONE);

        // Get material handle — check named library materials first
        let mat_handle = if let Some(ref mat_name) = mesh_config.material_path {
            if let Some(handle) = assets.named_materials.get(mat_name) {
                handle.clone()
            } else {
                // Fallback to base_color material
                get_or_create_color_material(
                    &mut assets,
                    &mut materials,
                    mesh_config.base_color,
                    uv_scale,
                )
            }
        } else {
            get_or_create_color_material(
                &mut assets,
                &mut materials,
                mesh_config.base_color,
                uv_scale,
            )
        };

        // Store material handle for UV scroll updates
        state.material_handle = Some(mat_handle.clone());

        let emitter_pos = global_transform.translation();

        for _ in 0..spawn_count {
            // Check capacity
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
                    InitModule::SetVelocity(mode) => velocity = sample_velocity(mode, position),
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
                                Quat::from_rotation_y(fastrand::f32() * std::f32::consts::TAU)
                            }
                            OrientMode::RandomFull => random_quat(),
                            OrientMode::AlignVelocity => Quat::IDENTITY, // deferred
                        };
                    }
                    InitModule::SetUvScale(_) => {} // handled at emitter level
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
                MeshMaterial3d(mat_handle.clone()),
                Transform::from_translation(world_pos)
                    .with_scale(scale)
                    .with_rotation(orientation),
            ));

            if use_physics {
                let collider = match &mesh_config.shape {
                    MeshShape::Cube => {
                        Collider::cuboid(scale.x, scale.y, scale.z)
                    }
                    MeshShape::Sphere => Collider::sphere(scale.x * 0.5),
                    MeshShape::Capsule => Collider::capsule(scale.x * 0.25, scale.y * 0.5),
                    MeshShape::Cylinder => Collider::cylinder(scale.x * 0.25, scale.y),
                    MeshShape::Custom(_) => {
                        Collider::cuboid(scale.x, scale.y, scale.z)
                    }
                };
                child_cmd.insert((
                    RigidBody::Dynamic,
                    collider,
                    LinearVelocity(velocity),
                    Restitution::new(mesh_config.restitution),
                ));
            }

            let child = child_cmd.id();

            // Parent for local-space sim
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
                orientation,
                physics: use_physics,
            });
        }
    }
}

/// Update particles: apply UpdateModule effects, advance age, kill expired.
pub fn cpu_mesh_particle_update(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(&VfxSystem, &mut MeshParticleState)>,
) {
    let dt = time.delta_secs();

    for (system, mut state) in &mut query {
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

            let t = p.age / p.lifetime; // normalized lifetime [0..1]

            // Apply update modules
            for update in &emitter.update {
                match update {
                    // Physics-driven particles: engine handles gravity, forces, drag
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
                                p.velocity += to_center.normalize() * *radius_decay * dt;
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
                    // Visual modules apply to all particles (physics or not)
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
                    UpdateModule::ColorByLife(gradient) => {
                        p.color = gradient.sample(t);
                    }
                    UpdateModule::SizeBySpeed {
                        min_speed,
                        max_speed,
                        min_size,
                        max_size,
                    } => {
                        let speed = p.velocity.length();
                        let frac =
                            ((speed - min_speed) / (max_speed - min_speed)).clamp(0.0, 1.0);
                        let s = *min_size + (*max_size - *min_size) * frac;
                        p.scale = Vec3::splat(s);
                    }
                    UpdateModule::RotateByVelocity => {
                        // orientation updated in sync
                    }
                    UpdateModule::Spin { axis, speed } => {
                        let norm = axis.normalize_or_zero();
                        if norm.length_squared() > 0.001 {
                            p.orientation =
                                Quat::from_axis_angle(norm, *speed * dt) * p.orientation;
                        }
                    }
                    UpdateModule::UvScroll { .. } => {
                        // handled at emitter level in cpu_mesh_particle_uv_scroll
                    }
                    // Skip physics-handled modules for physics particles
                    _ => {}
                }
            }

            // Integrate position (only for non-physics particles)
            if !p.physics {
                p.position += p.velocity * dt;
            }
        }

        // Remove dead particles (reverse order to preserve indices)
        dead.sort_unstable();
        dead.dedup();
        for &i in dead.iter().rev() {
            let p = state.particles.remove(i);
            commands.entity(p.entity).try_despawn();
        }
    }
}

/// Sync particle state to Transform and material on child entities.
/// For physics particles, reads Transform back to update position tracking.
/// For non-physics particles, writes our simulated position to Transform.
pub fn cpu_mesh_particle_sync(
    mut query: Query<(&VfxSystem, &mut MeshParticleState)>,
    mut transforms: Query<&mut Transform>,
) {
    for (system, mut state) in &mut query {
        let Some(emitter) = system.emitters.get(state.emitter_index) else {
            continue;
        };

        // Check orient mode from init modules
        let orient_mode = emitter.init.iter().find_map(|m| match m {
            InitModule::SetOrientation(mode) => Some(*mode),
            _ => None,
        });
        let align_to_velocity = orient_mode == Some(OrientMode::AlignVelocity);
        let has_rotate_by_vel = emitter
            .update
            .iter()
            .any(|u| matches!(u, UpdateModule::RotateByVelocity));

        for p in state.particles.iter_mut() {
            if let Ok(mut transform) = transforms.get_mut(p.entity) {
                if p.physics {
                    // Physics engine owns position/rotation — read back for tracking
                    p.position = transform.translation;
                    // Only update scale (size-over-life still applies)
                    transform.scale = p.scale;
                } else {
                    // We own position — write to transform
                    transform.translation = p.position;
                    transform.scale = p.scale;

                    if align_to_velocity || has_rotate_by_vel {
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

/// Animate UV scrolling on mesh particle materials.
/// Reads scroll speed from UpdateModule::UvScroll and UV scale from InitModule::SetUvScale.
pub fn cpu_mesh_particle_uv_scroll(
    time: Res<Time>,
    mut query: Query<(&VfxSystem, &mut MeshParticleState)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();

    for (system, mut state) in &mut query {
        let Some(emitter) = system.emitters.get(state.emitter_index) else {
            continue;
        };

        // Extract UV scroll speed from update modules
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

        // Extract UV scale from init modules
        let uv_scale = emitter
            .init
            .iter()
            .find_map(|m| match m {
                InitModule::SetUvScale(s) => Some(Vec2::new(s[0], s[1])),
                _ => None,
            })
            .unwrap_or(Vec2::ONE);

        state.uv_scroll_offset += scroll_speed * dt;
        // Wrap to prevent floating point issues
        state.uv_scroll_offset.x %= 1000.0;
        state.uv_scroll_offset.y %= 1000.0;

        if let Some(ref handle) = state.material_handle {
            if let Some(mat) = materials.get_mut(handle) {
                mat.uv_transform = Affine2::from_scale_angle_translation(
                    uv_scale,
                    0.0,
                    state.uv_scroll_offset,
                );
            }
        }
    }
}

/// Cleanup: despawn all mesh particle children when VfxSystem is removed or
/// when the emitter is switched away from Mesh mode.
pub fn cpu_mesh_particle_cleanup(
    mut commands: Commands,
    mut removed: RemovedComponents<VfxSystem>,
    mut query: Query<(Entity, &VfxSystem, &mut MeshParticleState)>,
) {
    // Handle removed VfxSystem entities
    for entity in removed.read() {
        commands.entity(entity).remove::<MeshParticleState>();
    }

    // Handle emitters that are no longer mesh mode
    for (entity, system, mut state) in &mut query {
        let still_mesh = system
            .emitters
            .get(state.emitter_index)
            .map(|e| matches!(e.render, RenderModule::Mesh(_)))
            .unwrap_or(false);

        if !still_mesh {
            // Despawn all child particles
            for p in state.particles.drain(..) {
                commands.entity(p.entity).try_despawn();
            }
            commands.entity(entity).remove::<MeshParticleState>();
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_or_create_color_material(
    assets: &mut MeshParticleAssets,
    materials: &mut Assets<StandardMaterial>,
    color: LinearRgba,
    uv_scale: Vec2,
) -> Handle<StandardMaterial> {
    let color_bits = color.red.to_bits() as u64
        ^ (color.green.to_bits() as u64).rotate_left(16)
        ^ (color.blue.to_bits() as u64).rotate_left(32)
        ^ (color.alpha.to_bits() as u64).rotate_left(48);
    let uv_bits =
        (uv_scale.x.to_bits() as u64) ^ (uv_scale.y.to_bits() as u64).rotate_left(32);
    let mat_key = color_bits ^ uv_bits.rotate_left(7);

    assets
        .materials
        .entry(mat_key)
        .or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::LinearRgba(color),
                uv_transform: Affine2::from_scale(uv_scale),
                ..default()
            })
        })
        .clone()
}

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
        } => {
            if let Some(max) = max_cycles {
                if state.burst_cycle >= *max {
                    return 0;
                }
            }
            state.burst_timer += dt;
            if state.burst_timer >= *interval {
                state.burst_timer -= interval;
                state.burst_cycle += 1;
                *count
            } else {
                0
            }
        }
        SpawnModule::Once(count) => {
            if state.once_fired {
                0
            } else {
                state.once_fired = true;
                *count
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
    // Uniform random quaternion via rejection sampling
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
