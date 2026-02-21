//! Prepare GPU buffers: upload emitter params, compute spawn counts.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::texture::GpuImage;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::buffers::*;
use super::extract::ExtractedVfxData;
use super::pipeline::VfxComputePipelines;
use crate::data::*;
use crate::render::billboard::VfxBillboardPipeline;

/// Persistent buffer cache keyed by (main-world entity, emitter index).
/// Survives across frames so we don't re-allocate GPU buffers every frame.
#[derive(Resource, Default)]
pub struct EmitterBufferCache {
    pub buffers: HashMap<(Entity, usize), EmitterBuffers>,
}

/// Hash the static (non-per-frame) portion of packed emitter params.
/// Everything before DYNAMIC_PARAMS_OFFSET (796 bytes) is considered static.
fn hash_static_params(params: &GpuEmitterParams) -> u64 {
    let bytes = bytemuck::bytes_of(params);
    let static_bytes = &bytes[..DYNAMIC_PARAMS_OFFSET as usize];
    let mut hasher = DefaultHasher::new();
    static_bytes.hash(&mut hasher);
    hasher.finish()
}

/// Upload emitter parameters and spawn counts to GPU buffers.
pub fn prepare_vfx_buffers(
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    time: Res<Time>,
    extracted: Res<ExtractedVfxData>,
    mut cache: ResMut<EmitterBufferCache>,
    mut active_buffers: ResMut<ActiveEmitterBuffers>,
    compute_pipelines: Option<Res<VfxComputePipelines>>,
    billboard_pipeline: Option<Res<VfxBillboardPipeline>>,
    gpu_images: Res<RenderAssets<GpuImage>>,
) {
    active_buffers.entries.clear();
    let dt = time.delta_secs();
    let elapsed = time.elapsed_secs();

    // We need both pipeline resources to create bind groups
    let pipelines_ready = compute_pipelines.is_some() && billboard_pipeline.is_some();

    // Track which keys are still active this frame (HashSet for O(1) eviction)
    let mut active_keys: HashSet<(Entity, usize)> =
        HashSet::with_capacity(extracted.emitters.len());

    for info in &extracted.emitters {
        let key = (info.source_entity, info.emitter_index);
        let emitter_def = &info.emitter;
        active_keys.insert(key);

        // Restart: evict cached buffers so they get fully re-initialized
        if info.restart {
            cache.buffers.remove(&key);
        }

        // Get or create buffers for this emitter
        let buffers = cache.buffers.entry(key).or_insert_with(|| {
            EmitterBuffers::new(&device, emitter_def.capacity)
        });

        // Recreate buffers if capacity changed
        if emitter_def.capacity != buffers.capacity {
            *buffers = EmitterBuffers::new(&device, emitter_def.capacity);
        }

        // Initialize buffers on first use or after recreation
        if buffers.last_emitter_def.is_none() {
            // Initialize dead buffer with all indices (all particles start dead)
            let dead_indices: Vec<u32> = (0..emitter_def.capacity).collect();
            queue.write_buffer(&buffers.dead_buffer, 0, bytemuck::cast_slice(&dead_indices));

            // Initialize counters (first 16 bytes = DrawIndirectArgs)
            let counters = GpuEmitterCounters {
                vertex_count: 6, // billboard quad = 2 triangles
                alive_count: 0,
                first_vertex: 0,
                first_instance: 0,
                dead_count: emitter_def.capacity,
                spawn_count: 0,
                billboard_flags: 0,
                _pad1: 0,
            };
            queue.write_buffer(&buffers.counter_buffer, 0, bytemuck::bytes_of(&counters));
        }

        // Compute per-system elapsed time (relative to start_time)
        let local_elapsed = elapsed - info.start_time;

        // Compute spawn count for this frame
        let spawn_count = compute_spawn_count(&emitter_def.spawn, dt, local_elapsed);

        // Fast change detection: compare EmitterDef by value to skip expensive
        // pack_emitter_params() (builds 912-byte struct) when nothing changed.
        let def_changed = buffers
            .last_emitter_def
            .as_ref()
            .map_or(true, |prev| prev != emitter_def);

        if def_changed {
            // EmitterDef changed (or first frame) — full pack + upload 912 bytes
            let params = pack_emitter_params(emitter_def, &info.transform, dt, local_elapsed, key);
            queue.write_buffer(&buffers.params_buffer, 0, bytemuck::bytes_of(&params));
            buffers.static_params_hash = hash_static_params(&params);
            buffers.last_emitter_def = Some(emitter_def.clone());
        } else {
            // Only dynamic params changed (dt, time, transform) — upload 84 bytes
            let dynamic = GpuDynamicParams {
                dt,
                time: local_elapsed,
                _pad13: [0.0; 3],
                emitter_transform: info.transform.to_matrix().to_cols_array_2d(),
            };
            queue.write_buffer(
                &buffers.params_buffer,
                DYNAMIC_PARAMS_OFFSET,
                bytemuck::bytes_of(&dynamic),
            );
        }

        // Update spawn count in counter buffer
        let offset = std::mem::offset_of!(GpuEmitterCounters, spawn_count) as u64;
        queue.write_buffer(
            &buffers.counter_buffer,
            offset,
            bytemuck::bytes_of(&spawn_count),
        );

        // Resolve texture for this emitter
        let has_texture = info.texture.is_some();
        let billboard_flags: u32 = if has_texture { BILLBOARD_FLAG_HAS_TEXTURE } else { 0 };
        let flags_offset = std::mem::offset_of!(GpuEmitterCounters, billboard_flags) as u64;
        queue.write_buffer(
            &buffers.counter_buffer,
            flags_offset,
            bytemuck::bytes_of(&billboard_flags),
        );

        // Invalidate billboard bind group when texture changes or finishes loading
        let texture_changed = buffers.bound_texture != info.texture;
        let texture_became_available = !buffers.bound_texture_resolved
            && info.texture.map_or(false, |id| gpu_images.get(id).is_some());
        if texture_changed || texture_became_available {
            buffers.billboard_bind_group = None;
            buffers.bound_texture = info.texture;
        }

        // Create or reuse cached bind groups
        if pipelines_ready {
            let compute_pl = compute_pipelines.as_ref().unwrap();
            let billboard_pl = billboard_pipeline.as_ref().unwrap();

            if buffers.compute_bind_group.is_none() {
                buffers.compute_bind_group = Some(device.create_bind_group(
                    "vfx_compute_bind_group",
                    &compute_pl.bind_group_layout,
                    &[
                        BindGroupEntry {
                            binding: 0,
                            resource: buffers.particle_buffer.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: buffers.alive_buffer.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 2,
                            resource: buffers.dead_buffer.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 3,
                            resource: buffers.counter_buffer.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 4,
                            resource: buffers.params_buffer.as_entire_binding(),
                        },
                    ],
                ));
            }

            if buffers.billboard_bind_group.is_none() {
                // Resolve texture: use real GpuImage if available, else fallback
                let (texture_view, tex_sampler, resolved) = if let Some(tex_id) = info.texture {
                    if let Some(gpu_image) = gpu_images.get(tex_id) {
                        (&gpu_image.texture_view, &gpu_image.sampler, true)
                    } else {
                        (&billboard_pl.fallback_texture_view, &billboard_pl.fallback_sampler, false)
                    }
                } else {
                    (&billboard_pl.fallback_texture_view, &billboard_pl.fallback_sampler, true)
                };
                buffers.bound_texture_resolved = resolved;

                buffers.billboard_bind_group = Some(device.create_bind_group(
                    "vfx_billboard_particle_bg",
                    &billboard_pl.particle_bind_group_layout,
                    &[
                        BindGroupEntry {
                            binding: 0,
                            resource: buffers.particle_buffer.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: buffers.alive_buffer.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 2,
                            resource: buffers.counter_buffer.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 3,
                            resource: BindingResource::TextureView(texture_view),
                        },
                        BindGroupEntry {
                            binding: 4,
                            resource: BindingResource::Sampler(tex_sampler),
                        },
                    ],
                ));
            }

            if let (Some(compute_bg), Some(billboard_bg)) = (
                &buffers.compute_bind_group,
                &buffers.billboard_bind_group,
            ) {
                active_buffers.entries.push(ActiveEmitterEntry {
                    capacity: buffers.capacity,
                    counter_buffer: buffers.counter_buffer.clone(),
                    compute_bind_group: compute_bg.clone(),
                    billboard_bind_group: billboard_bg.clone(),
                });
            }
        }
    }

    // Evict stale buffers for emitters that no longer exist (O(n) with HashSet)
    cache.buffers.retain(|k, _| active_keys.contains(k));
}

/// Compute how many particles to spawn this frame.
fn compute_spawn_count(spawn: &SpawnModule, dt: f32, elapsed: f32) -> u32 {
    match spawn {
        SpawnModule::Rate(rate) => {
            let exact = rate * dt;
            let base = exact as u32;
            let frac = exact - base as f32;
            if (elapsed * 1000.0).fract() < frac {
                base + 1
            } else {
                base
            }
        }
        SpawnModule::Burst {
            count,
            interval,
            max_cycles,
            offset,
        } => {
            if *interval <= 0.0 {
                return 0;
            }
            let local = elapsed - offset;
            if local < 0.0 {
                return 0;
            }
            // Shift so first burst fires at local=0 (i.e. elapsed=offset),
            // then every interval after that
            let adjusted = local + *interval;
            let cycle = (adjusted / interval) as u32;
            let prev_adjusted = adjusted - dt;
            let prev_cycle =
                if prev_adjusted <= 0.0 { 0 } else { (prev_adjusted / interval) as u32 };
            if cycle > prev_cycle {
                if let Some(max) = max_cycles {
                    if cycle > *max {
                        return 0;
                    }
                }
                *count
            } else {
                0
            }
        }
        SpawnModule::Once { count, offset } => {
            let local = elapsed - offset;
            if local >= 0.0 && local < dt * 2.0 {
                *count
            } else {
                0
            }
        }
        SpawnModule::Distance { .. } => 0,
    }
}

/// Pack an EmitterDef into the GPU-side GpuEmitterParams struct.
fn pack_emitter_params(
    emitter: &EmitterDef,
    transform: &GlobalTransform,
    dt: f32,
    time: f32,
    key: (Entity, usize),
) -> GpuEmitterParams {
    // Derive a per-emitter seed from entity bits + emitter index so
    // identically-configured emitters at the same time don't stack particles.
    let entity_bits = key.0.to_bits();
    let emitter_seed = entity_bits
        .wrapping_mul(2654435761) // Knuth multiplicative hash
        .wrapping_add(key.1 as u64 * 1073741827)
        as u32;

    let mut params = GpuEmitterParams {
        shape_type: 0,
        _pad0: [0; 3],
        shape_param0: [0.0; 3],
        _pad1: 0.0,
        shape_param1: [0.0; 3],
        shape_radius_min: 0.0,
        shape_radius_max: 0.0,
        velocity_mode: 0,
        _pad2: [0.0; 2],
        velocity_param0: [0.0; 3],
        velocity_speed_min: 0.0,
        velocity_speed_max: 0.0,
        velocity_cone_angle: 0.0,
        lifetime_min: 1.0,
        lifetime_max: 1.0,
        init_color: [1.0, 1.0, 1.0, 1.0],
        init_size_min: 0.1,
        init_size_max: 0.1,
        init_rotation_min: 0.0,
        init_rotation_max: 0.0,
        inherit_velocity_ratio: 0.0,
        _pad3: [0.0; 3],
        gravity: [0.0; 3],
        _pad4: 0.0,
        constant_force: [0.0; 3],
        drag: 0.0,
        noise_strength: 0.0,
        noise_frequency: 1.0,
        _pad5: [0.0; 2],
        noise_scroll: [0.0; 3],
        _pad6: 0.0,
        orbit_axis: [0.0, 1.0, 0.0],
        orbit_speed: 0.0,
        orbit_radius_decay: 0.0,
        _pad7: [0.0; 3],
        attract_target: [0.0; 3],
        attract_strength: 0.0,
        attract_falloff: 1.0,
        kill_zone_type: 0,
        _pad8: [0.0; 2],
        kill_zone_center: [0.0; 3],
        _pad9: 0.0,
        kill_zone_param: [0.0; 3],
        kill_zone_invert: 0,
        tangent_accel_origin: [0.0; 3],
        _pad10: 0.0,
        tangent_accel_axis: [0.0, 1.0, 0.0],
        tangent_accel_value: 0.0,
        radial_accel_origin: [0.0; 3],
        radial_accel_value: 0.0,
        flags: 0,
        size_curve_count: 0,
        _pad11: [0.0; 2],
        size_curve: [[0.0; 4]; MAX_CURVE_KEYS],
        color_gradient_count: 0,
        _pad12: [0; 3],
        color_gradient: [[0.0; 4]; MAX_GRADIENT_KEYS * 2],
        size_by_speed_min_speed: 0.0,
        size_by_speed_max_speed: 10.0,
        size_by_speed_min_size: 0.01,
        size_by_speed_max_size: 1.0,
        orient_mode: 0,
        alpha_mode: 0,
        sim_space: 0,
        dt,
        time,
        _pad13: [0.0; 3],
        emitter_transform: transform.to_matrix().to_cols_array_2d(),
        emitter_seed,
        _pad_tail: [0.0; 7],
    };

    // Pack init modules
    for module in &emitter.init {
        match module {
            InitModule::SetLifetime(range) => {
                params.lifetime_min = range.min_val();
                params.lifetime_max = range.max_val();
            }
            InitModule::SetPosition(shape) => {
                pack_shape_emitter(&mut params, shape);
            }
            InitModule::SetVelocity(mode) => {
                pack_velocity_mode(&mut params, mode);
            }
            InitModule::SetColor(source) => match source {
                ColorSource::Constant(c) => {
                    params.init_color = [c.red, c.green, c.blue, c.alpha];
                }
                ColorSource::RandomFromGradient(_) => {}
            },
            InitModule::SetSize(range) => {
                params.init_size_min = range.min_val();
                params.init_size_max = range.max_val();
            }
            InitModule::SetRotation(range) => {
                params.init_rotation_min = range.min_val();
                params.init_rotation_max = range.max_val();
            }
            InitModule::InheritVelocity { ratio } => {
                params.inherit_velocity_ratio = *ratio;
            }
            // CPU-only modules (mesh particles)
            InitModule::SetOrientation(_)
            | InitModule::SetScale3d { .. }
            | InitModule::SetUvScale(_) => {}
        }
    }

    // Pack update modules
    for module in &emitter.update {
        match module {
            UpdateModule::Gravity(g) => {
                params.gravity = g.to_array();
                params.flags |= FLAG_GRAVITY;
            }
            UpdateModule::ConstantForce(f) => {
                params.constant_force = f.to_array();
                params.flags |= FLAG_CONSTANT_FORCE;
            }
            UpdateModule::Drag(d) => {
                params.drag = *d;
                params.flags |= FLAG_DRAG;
            }
            UpdateModule::Noise {
                strength,
                frequency,
                scroll,
            } => {
                params.noise_strength = *strength;
                params.noise_frequency = *frequency;
                params.noise_scroll = scroll.to_array();
                params.flags |= FLAG_NOISE;
            }
            UpdateModule::OrbitAround {
                axis,
                speed,
                radius_decay,
            } => {
                params.orbit_axis = axis.to_array();
                params.orbit_speed = *speed;
                params.orbit_radius_decay = *radius_decay;
                params.flags |= FLAG_ORBIT;
            }
            UpdateModule::Attract {
                target,
                strength,
                falloff,
            } => {
                params.attract_target = target.to_array();
                params.attract_strength = *strength;
                params.attract_falloff = *falloff;
                params.flags |= FLAG_ATTRACT;
            }
            UpdateModule::KillZone { shape, invert } => {
                match shape {
                    KillShape::Sphere { center, radius } => {
                        params.kill_zone_type = 1;
                        params.kill_zone_center = center.to_array();
                        params.kill_zone_param = [*radius, 0.0, 0.0];
                    }
                    KillShape::Box {
                        center,
                        half_extents,
                    } => {
                        params.kill_zone_type = 2;
                        params.kill_zone_center = center.to_array();
                        params.kill_zone_param = half_extents.to_array();
                    }
                }
                params.kill_zone_invert = if *invert { 1 } else { 0 };
                params.flags |= FLAG_KILL_ZONE;
            }
            UpdateModule::SizeByLife(curve) => {
                let packed = curve.pack_for_gpu(MAX_CURVE_KEYS);
                params.size_curve_count = packed.len() as u32;
                for (i, key) in packed.iter().enumerate() {
                    if i < MAX_CURVE_KEYS {
                        params.size_curve[i] = [key[0], key[1], 0.0, 0.0];
                    }
                }
                params.flags |= FLAG_SIZE_BY_LIFE;
            }
            UpdateModule::ColorByLife(gradient) => {
                let packed = gradient.pack_for_gpu(MAX_GRADIENT_KEYS);
                params.color_gradient_count = packed.len() as u32;
                for (i, key) in packed.iter().enumerate() {
                    if i < MAX_GRADIENT_KEYS {
                        params.color_gradient[i * 2] = [key[0], key[1], key[2], key[3]];
                        params.color_gradient[i * 2 + 1] = [key[4], 0.0, 0.0, 0.0];
                    }
                }
                params.flags |= FLAG_COLOR_BY_LIFE;
            }
            UpdateModule::SizeBySpeed {
                min_speed,
                max_speed,
                min_size,
                max_size,
            } => {
                params.size_by_speed_min_speed = *min_speed;
                params.size_by_speed_max_speed = *max_speed;
                params.size_by_speed_min_size = *min_size;
                params.size_by_speed_max_size = *max_size;
                params.flags |= FLAG_SIZE_BY_SPEED;
            }
            UpdateModule::RotateByVelocity => {
                params.flags |= FLAG_ROTATE_BY_VELOCITY;
            }
            UpdateModule::TangentAccel {
                origin,
                axis,
                accel,
            } => {
                params.tangent_accel_origin = origin.to_array();
                params.tangent_accel_axis = axis.to_array();
                params.tangent_accel_value = *accel;
                params.flags |= FLAG_TANGENT_ACCEL;
            }
            UpdateModule::RadialAccel { origin, accel } => {
                params.radial_accel_origin = origin.to_array();
                params.radial_accel_value = *accel;
                params.flags |= FLAG_RADIAL_ACCEL;
            }
            // CPU-only modules (mesh particles)
            UpdateModule::Spin { .. }
            | UpdateModule::UvScroll { .. }
            | UpdateModule::Scale3dByLife { .. }
            | UpdateModule::OffsetByLife { .. }
            | UpdateModule::EmissiveOverLife(_) => {}
        }
    }

    // Pack render settings
    match &emitter.render {
        RenderModule::Billboard(config) => {
            params.orient_mode = match config.orient {
                BillboardOrient::FaceCamera => 0,
                BillboardOrient::ParallelCamera => 1,
                BillboardOrient::AlongVelocity => 2,
            };
        }
        _ => {}
    }

    params.alpha_mode = match emitter.alpha_mode {
        VfxAlphaMode::Blend => 0,
        VfxAlphaMode::Additive => 1,
        VfxAlphaMode::Premultiply => 2,
        VfxAlphaMode::Multiply => 3,
        VfxAlphaMode::Opaque => 4,
    };

    params.sim_space = match emitter.sim_space {
        SimSpace::World => 0,
        SimSpace::Local => 1,
    };

    params
}

fn pack_shape_emitter(params: &mut GpuEmitterParams, shape: &ShapeEmitter) {
    match shape {
        ShapeEmitter::Point(p) => {
            params.shape_type = 0;
            params.shape_param0 = p.to_array();
        }
        ShapeEmitter::Sphere { center, radius } => {
            params.shape_type = 1;
            params.shape_param0 = center.to_array();
            params.shape_radius_min = radius.min_val();
            params.shape_radius_max = radius.max_val();
        }
        ShapeEmitter::Box {
            center,
            half_extents,
        } => {
            params.shape_type = 2;
            params.shape_param0 = center.to_array();
            params.shape_param1 = half_extents.to_array();
        }
        ShapeEmitter::Cone {
            angle,
            radius,
            height,
        } => {
            params.shape_type = 3;
            params.shape_param0 = [*angle, *radius, *height];
        }
        ShapeEmitter::Circle {
            center,
            axis,
            radius,
        } => {
            params.shape_type = 4;
            params.shape_param0 = center.to_array();
            params.shape_param1 = axis.to_array();
            params.shape_radius_min = radius.min_val();
            params.shape_radius_max = radius.max_val();
        }
        ShapeEmitter::Edge { start, end } => {
            params.shape_type = 5;
            params.shape_param0 = start.to_array();
            params.shape_param1 = end.to_array();
        }
    }
}

fn pack_velocity_mode(params: &mut GpuEmitterParams, mode: &VelocityMode) {
    match mode {
        VelocityMode::Radial { center, speed } => {
            params.velocity_mode = 0;
            params.velocity_param0 = center.to_array();
            params.velocity_speed_min = speed.min_val();
            params.velocity_speed_max = speed.max_val();
        }
        VelocityMode::Directional { direction, speed } => {
            params.velocity_mode = 1;
            params.velocity_param0 = direction.to_array();
            params.velocity_speed_min = speed.min_val();
            params.velocity_speed_max = speed.max_val();
        }
        VelocityMode::Tangent { axis, speed } => {
            params.velocity_mode = 2;
            params.velocity_param0 = axis.to_array();
            params.velocity_speed_min = speed.min_val();
            params.velocity_speed_max = speed.max_val();
        }
        VelocityMode::Cone {
            direction,
            angle,
            speed,
        } => {
            params.velocity_mode = 3;
            params.velocity_param0 = direction.to_array();
            params.velocity_cone_angle = *angle;
            params.velocity_speed_min = speed.min_val();
            params.velocity_speed_max = speed.max_val();
        }
        VelocityMode::Random { speed } => {
            params.velocity_mode = 4;
            params.velocity_speed_min = speed.min_val();
            params.velocity_speed_max = speed.max_val();
        }
    }
}
