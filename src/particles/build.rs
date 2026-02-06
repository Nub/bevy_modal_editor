//! Build `EffectAsset` from the serializable `ParticleEffectMarker` data model.

use bevy::prelude::*;
use bevy_hanabi::prelude::*;
use bevy_hanabi::Gradient as HanabiGradient;

use super::data::*;

/// Build an `EffectAsset` from a `ParticleEffectMarker`.
///
/// Creates a fresh `Module`, converts all stored values into `ExprHandle`s,
/// constructs the modifiers, and assembles the final asset.
pub fn build_effect(marker: &ParticleEffectMarker) -> EffectAsset {
    // --- Spawner ---
    let spawner = match &marker.spawner {
        SpawnerConfig::Rate { rate } => SpawnerSettings::rate((*rate).into()),
        SpawnerConfig::Once { count } => SpawnerSettings::once((*count).into()),
        SpawnerConfig::Burst { count, period } => {
            SpawnerSettings::burst((*count).into(), (*period).into())
        }
    };

    // Build all expression handles into the module first, since EffectAsset::new
    // takes ownership of the module.
    let mut module = Module::default();

    let mut init_data: Vec<InitModBuilt> = Vec::new();
    for m in &marker.init_modifiers {
        init_data.push(build_init_modifier(&mut module, m));
    }

    let mut update_data: Vec<UpdateModBuilt> = Vec::new();
    for m in &marker.update_modifiers {
        update_data.push(build_update_modifier(&mut module, m));
    }

    // Now create the effect with the populated module
    let mut effect = EffectAsset::new(marker.capacity, spawner, module);

    // Top-level settings
    effect = effect
        .with_simulation_space(match marker.simulation_space {
            ParticleSimSpace::Global => SimulationSpace::Global,
            ParticleSimSpace::Local => SimulationSpace::Local,
        })
        .with_simulation_condition(match marker.simulation_condition {
            ParticleSimCondition::WhenVisible => SimulationCondition::WhenVisible,
            ParticleSimCondition::Always => SimulationCondition::Always,
        })
        .with_motion_integration(match marker.motion_integration {
            ParticleMotionIntegration::None => MotionIntegration::None,
            ParticleMotionIntegration::PreUpdate => MotionIntegration::PreUpdate,
            ParticleMotionIntegration::PostUpdate => MotionIntegration::PostUpdate,
        })
        .with_alpha_mode(match marker.alpha_mode {
            ParticleAlphaMode::Blend => bevy_hanabi::AlphaMode::Blend,
            ParticleAlphaMode::Premultiply => bevy_hanabi::AlphaMode::Premultiply,
            ParticleAlphaMode::Add => bevy_hanabi::AlphaMode::Add,
            ParticleAlphaMode::Multiply => bevy_hanabi::AlphaMode::Multiply,
            ParticleAlphaMode::Opaque => bevy_hanabi::AlphaMode::Opaque,
        });

    // Apply init modifiers
    for built in init_data {
        effect = apply_init_modifier(effect, built);
    }

    // Apply update modifiers
    for built in update_data {
        effect = apply_update_modifier(effect, built);
    }

    // Apply render modifiers
    for m in &marker.render_modifiers {
        effect = apply_render_modifier(effect, m);
    }

    effect
}

// ---------------------------------------------------------------------------
// Helper: build scalar ExprHandle from ScalarRange
// ---------------------------------------------------------------------------

fn scalar_expr(module: &mut Module, val: &ScalarRange) -> ExprHandle {
    match val {
        ScalarRange::Constant(v) => module.lit(*v),
        ScalarRange::Random(min, max) => {
            let lo = module.lit(*min);
            let hi = module.lit(*max);
            module.uniform(lo, hi)
        }
    }
}

// ---------------------------------------------------------------------------
// Init modifier building
// ---------------------------------------------------------------------------

enum InitModBuilt {
    SetAttribute(SetAttributeModifier),
    SetPositionSphere(SetPositionSphereModifier),
    SetPositionCircle(SetPositionCircleModifier),
    SetVelocitySphere(SetVelocitySphereModifier),
    SetVelocityTangent(SetVelocityTangentModifier),
}

fn build_init_modifier(module: &mut Module, data: &InitModifierData) -> InitModBuilt {
    match data {
        InitModifierData::SetLifetime(range) => {
            let val = scalar_expr(module, range);
            InitModBuilt::SetAttribute(SetAttributeModifier::new(Attribute::LIFETIME, val))
        }
        InitModifierData::SetColor(color) => {
            let val = module.lit(*color);
            InitModBuilt::SetAttribute(SetAttributeModifier::new(Attribute::HDR_COLOR, val))
        }
        InitModifierData::SetSize(range) => {
            let val = scalar_expr(module, range);
            InitModBuilt::SetAttribute(SetAttributeModifier::new(Attribute::SIZE, val))
        }
        InitModifierData::SetPositionSphere {
            center,
            radius,
            volume,
        } => {
            let c = module.lit(*center);
            let r = scalar_expr(module, radius);
            InitModBuilt::SetPositionSphere(SetPositionSphereModifier {
                center: c,
                radius: r,
                dimension: if *volume {
                    ShapeDimension::Volume
                } else {
                    ShapeDimension::Surface
                },
            })
        }
        InitModifierData::SetPositionCircle {
            center,
            axis,
            radius,
            volume,
        } => {
            let c = module.lit(*center);
            let a = module.lit(*axis);
            let r = scalar_expr(module, radius);
            InitModBuilt::SetPositionCircle(SetPositionCircleModifier {
                center: c,
                axis: a,
                radius: r,
                dimension: if *volume {
                    ShapeDimension::Volume
                } else {
                    ShapeDimension::Surface
                },
            })
        }
        InitModifierData::SetVelocitySphere { center, speed } => {
            let c = module.lit(*center);
            let s = scalar_expr(module, speed);
            InitModBuilt::SetVelocitySphere(SetVelocitySphereModifier {
                center: c,
                speed: s,
            })
        }
        InitModifierData::SetVelocityTangent {
            origin,
            axis,
            speed,
        } => {
            let o = module.lit(*origin);
            let a = module.lit(*axis);
            let s = scalar_expr(module, speed);
            InitModBuilt::SetVelocityTangent(SetVelocityTangentModifier {
                origin: o,
                axis: a,
                speed: s,
            })
        }
    }
}

fn apply_init_modifier(effect: EffectAsset, built: InitModBuilt) -> EffectAsset {
    match built {
        InitModBuilt::SetAttribute(m) => effect.init(m),
        InitModBuilt::SetPositionSphere(m) => effect.init(m),
        InitModBuilt::SetPositionCircle(m) => effect.init(m),
        InitModBuilt::SetVelocitySphere(m) => effect.init(m),
        InitModBuilt::SetVelocityTangent(m) => effect.init(m),
    }
}

// ---------------------------------------------------------------------------
// Update modifier building
// ---------------------------------------------------------------------------

enum UpdateModBuilt {
    Accel(AccelModifier),
    RadialAccel(RadialAccelModifier),
    LinearDrag(LinearDragModifier),
    KillAabb(KillAabbModifier),
    KillSphere(KillSphereModifier),
}

fn build_update_modifier(module: &mut Module, data: &UpdateModifierData) -> UpdateModBuilt {
    match data {
        UpdateModifierData::Accel(d) => {
            let a = module.lit(d.accel);
            UpdateModBuilt::Accel(AccelModifier::new(a))
        }
        UpdateModifierData::RadialAccel(d) => {
            let o = module.lit(d.origin);
            let a = module.lit(d.accel);
            UpdateModBuilt::RadialAccel(RadialAccelModifier::new(o, a))
        }
        UpdateModifierData::LinearDrag(d) => {
            let drag = module.lit(d.drag);
            UpdateModBuilt::LinearDrag(LinearDragModifier::new(drag))
        }
        UpdateModifierData::KillAabb(d) => {
            let c = module.lit(d.center);
            let hs = module.lit(d.half_size);
            UpdateModBuilt::KillAabb(
                KillAabbModifier::new(c, hs).with_kill_inside(d.kill_inside),
            )
        }
        UpdateModifierData::KillSphere(d) => {
            let c = module.lit(d.center);
            let sqr = module.lit(d.radius * d.radius);
            UpdateModBuilt::KillSphere(
                KillSphereModifier::new(c, sqr).with_kill_inside(d.kill_inside),
            )
        }
    }
}

fn apply_update_modifier(effect: EffectAsset, built: UpdateModBuilt) -> EffectAsset {
    match built {
        UpdateModBuilt::Accel(m) => effect.update(m),
        UpdateModBuilt::RadialAccel(m) => effect.update(m),
        UpdateModBuilt::LinearDrag(m) => effect.update(m),
        UpdateModBuilt::KillAabb(m) => effect.update(m),
        UpdateModBuilt::KillSphere(m) => effect.update(m),
    }
}

// ---------------------------------------------------------------------------
// Render modifiers
// ---------------------------------------------------------------------------

fn apply_render_modifier(effect: EffectAsset, data: &RenderModifierData) -> EffectAsset {
    match data {
        RenderModifierData::ColorOverLifetime { keys } => {
            let mut gradient = HanabiGradient::<Vec4>::new();
            for k in keys {
                gradient.add_key(k.ratio, k.value);
            }
            effect.render(ColorOverLifetimeModifier::new(gradient))
        }
        RenderModifierData::SizeOverLifetime { keys } => {
            let mut gradient = HanabiGradient::<Vec3>::new();
            for k in keys {
                gradient.add_key(k.ratio, k.value.truncate());
            }
            effect.render(SizeOverLifetimeModifier {
                gradient,
                screen_space_size: false,
            })
        }
        RenderModifierData::SetColor { color } => {
            effect.render(SetColorModifier::new(CpuValue::Single(*color)))
        }
        RenderModifierData::SetSize { size } => {
            effect.render(SetSizeModifier {
                size: CpuValue::Single(*size),
            })
        }
        RenderModifierData::Orient { mode } => {
            let orient_mode = match mode {
                ParticleOrientMode::ParallelCameraDepthPlane => {
                    OrientMode::ParallelCameraDepthPlane
                }
                ParticleOrientMode::FaceCameraPosition => OrientMode::FaceCameraPosition,
                ParticleOrientMode::AlongVelocity => OrientMode::AlongVelocity,
            };
            effect.render(OrientModifier::new(orient_mode))
        }
        RenderModifierData::ScreenSpaceSize => {
            effect.render(ScreenSpaceSizeModifier)
        }
    }
}
