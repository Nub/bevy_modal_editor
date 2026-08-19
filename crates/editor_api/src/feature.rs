//! The feature entry point and registry (RFC §3).
//!
//! A feature crate implements `EditorFeature` and calls `app.add_editor_feature(...)`
//! under its own `editor` cargo feature. `editor_core` (when present) drains
//! `PendingFeatures`, runs `validate()`, and hard-errors on any problem — registration
//! mistakes are startup failures, never silent (spec §8: by construction).

use crate::actions::ActionDef;
use crate::bake::BakerDef;
use crate::ids::{ActionId, ContextId, FeatureId, GLOBAL_CONTEXT, ModeId};
use crate::keymap::{Binding, find_conflicts};
use crate::kinds::EntityKindDef;
use crate::panels::PanelDecl;
use crate::pipeline::ProcessorDef;
use crate::validate::{LevelValidatorDef, ValidatorDef};
use bevy::prelude::*;
use bevy::reflect::GetTypeRegistration;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;

pub struct FeatureManifest {
    pub id: FeatureId,
    pub name: Cow<'static, str>,
    /// editor_api version this feature was built against (semver drift diagnostics).
    pub api_version: &'static str,
    /// Hard deps on other features — allowed, discouraged (RFC §12.3).
    pub requires: Vec<FeatureId>,
}

impl FeatureManifest {
    pub fn new(id: impl Into<FeatureId>, name: &'static str) -> Self {
        Self {
            id: id.into(),
            name: Cow::Borrowed(name),
            api_version: env!("CARGO_PKG_VERSION"),
            requires: Vec::new(),
        }
    }
}

pub trait EditorFeature: Send + Sync + 'static {
    fn manifest(&self) -> FeatureManifest;
    fn register(&self, reg: &mut FeatureRegistry);
}

#[derive(Clone, Debug)]
pub struct ModeDef {
    pub id: ModeId,
    pub name: Cow<'static, str>,
    /// Shown in the status line while the mode is active.
    pub statusline_hint: Cow<'static, str>,
}

impl ModeDef {
    pub fn new(id: impl Into<ModeId>, name: &'static str) -> Self {
        Self {
            id: id.into(),
            name: Cow::Borrowed(name),
            statusline_hint: Cow::Borrowed(""),
        }
    }
    pub fn hint(mut self, hint: &'static str) -> Self {
        self.statusline_hint = Cow::Borrowed(hint);
        self
    }
}

/// Collects one feature's declarations. Owned by the kernel during registration; the
/// grow-only RFC surface (panels, components, kinds, gizmos, validators, pipeline)
/// lands as those subsystems arrive in later milestones.
/// A registered editor component: participates in serialization, despawn capture,
/// dirty tracking, and (later) inspector metadata — one call registers everything
/// (spec §5: one registration point per component).
#[derive(Clone)]
pub struct ComponentReg {
    pub type_id: std::any::TypeId,
    pub type_path: &'static str,
    /// Registers the type into the world's `AppTypeRegistry` at host time.
    pub register: fn(&AppTypeRegistry),
}

#[derive(Default)]
pub struct FeatureRegistry {
    pub(crate) actions: Vec<(FeatureId, ActionDef)>,
    pub(crate) modes: Vec<(FeatureId, ModeDef)>,
    pub(crate) components: Vec<(FeatureId, ComponentReg)>,
    pub(crate) contexts: Vec<(FeatureId, ContextId)>,
    pub kinds: Vec<(FeatureId, EntityKindDef)>,
    pub panels: Vec<(FeatureId, PanelDecl)>,
    pub validators: Vec<(FeatureId, ValidatorDef)>,
    pub level_validators: Vec<(FeatureId, LevelValidatorDef)>,
    pub processors: Vec<(FeatureId, ProcessorDef)>,
    pub bakers: Vec<(FeatureId, BakerDef)>,
    pub gizmos: Vec<(FeatureId, crate::gizmos::GizmoDef)>,
    current_feature: Option<FeatureId>,
}

impl FeatureRegistry {
    pub fn action(&mut self, def: ActionDef) -> &mut Self {
        let feature = self.current().clone();
        self.actions.push((feature, def));
        self
    }
    pub fn mode(&mut self, def: ModeDef) -> &mut Self {
        let feature = self.current().clone();
        self.modes.push((feature, def));
        self
    }
    /// Register a spawnable entity kind (RFC §7). The kernel synthesizes an
    /// `insert.kind.<id>` action for the palette/insert mode automatically.
    pub fn entity_kind(&mut self, def: EntityKindDef) -> &mut Self {
        let feature = self.current().clone();
        self.kinds.push((feature, def));
        self
    }
    /// Kernel-side: inject a synthesized action under a feature's id (used for
    /// registry-derived actions like kind insertion — never by features directly).
    pub fn synthesize_action(&mut self, feature: FeatureId, def: ActionDef) -> &mut Self {
        self.actions.push((feature, def));
        self
    }
    /// Register a panel (RFC §9). The panel's focus context is registered implicitly;
    /// the layout manager owns docking, chrome, and focus.
    pub fn panel(&mut self, decl: PanelDecl) -> &mut Self {
        let feature = self.current().clone();
        self.panels.push((feature, decl));
        self
    }
    /// Register an asset processor (RFC, M4-D3).
    pub fn processor(&mut self, def: ProcessorDef) -> &mut Self {
        let feature = self.current().clone();
        self.processors.push((feature, def));
        self
    }
    /// Register a bake step (spec §6, M4-D8).
    pub fn baker(&mut self, def: BakerDef) -> &mut Self {
        let feature = self.current().clone();
        self.bakers.push((feature, def));
        self
    }

    /// Draw a custom gizmo for `T` (spec §7): the editor renders it for every
    /// entity carrying the component, and `pick_radius` gives a gizmo-only
    /// widget a click target so it selects like anything with a mesh.
    pub fn gizmo<T>(
        &mut self,
        id: crate::ids::GizmoId,
        pick_radius: Option<f32>,
        draw: crate::gizmos::GizmoDrawFn,
    ) -> &mut Self
    where
        T: bevy::prelude::Component + bevy::reflect::Reflect,
    {
        let feature = self.current().clone();
        self.gizmos.push((
            feature,
            crate::gizmos::GizmoDef {
                id,
                component: std::any::TypeId::of::<T>(),
                draw,
                pick_radius,
            },
        ));
        self
    }
    /// Register an import-time validator (RFC, M4-D2).
    pub fn validator(&mut self, def: ValidatorDef) -> &mut Self {
        let feature = self.current().clone();
        self.validators.push((feature, def));
        self
    }
    /// Register a LEVEL validator (v1 parity): required configs/objects/
    /// components the live scene must satisfy.
    pub fn level_validator(&mut self, def: LevelValidatorDef) -> &mut Self {
        let feature = self.current().clone();
        self.level_validators.push((feature, def));
        self
    }
    /// Register an overlay keymap context (gesture layers, focused-panel layers) that
    /// is not a mode. Activated via the kernel's `OverlayContext`.
    pub fn context(&mut self, id: impl Into<ContextId>) -> &mut Self {
        let feature = self.current().clone();
        self.contexts.push((feature, id.into()));
        self
    }
    /// Register an editor component (spec §5). Bounds match BSN blanket-template
    /// compatibility (spike 2): reflected + Default + Clone.
    pub fn component<T>(&mut self) -> &mut Self
    where
        T: Component + bevy::reflect::Reflect + GetTypeRegistration + bevy::reflect::TypePath,
    {
        let feature = self.current().clone();
        self.components.push((
            feature,
            ComponentReg {
                type_id: std::any::TypeId::of::<T>(),
                type_path: T::type_path(),
                register: |registry: &AppTypeRegistry| {
                    registry.write().register::<T>();
                },
            },
        ));
        self
    }
    fn current(&self) -> &FeatureId {
        self.current_feature
            .as_ref()
            .expect("registration outside a feature")
    }
    /// Kernel-side: run one feature's registration under its id.
    pub fn register_feature(&mut self, feature: &dyn EditorFeature) {
        self.current_feature = Some(feature.manifest().id);
        feature.register(self);
        self.current_feature = None;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    DuplicateAction {
        id: ActionId,
        first: FeatureId,
        second: FeatureId,
    },
    DuplicateMode {
        id: ModeId,
        first: FeatureId,
        second: FeatureId,
    },
    BadBinding {
        action: ActionId,
        binding: String,
        message: String,
    },
    UnknownContext {
        action: ActionId,
        context: ContextId,
    },
    BindingConflict {
        context: ContextId,
        detail: String,
    },
    DuplicatePanel {
        id: crate::ids::PanelId,
        first: FeatureId,
        second: FeatureId,
    },
    DuplicateValidator {
        id: crate::ids::ValidatorId,
        first: FeatureId,
        second: FeatureId,
    },
    DuplicateBaker {
        id: crate::ids::BakerId,
        first: FeatureId,
        second: FeatureId,
    },
    DuplicateProcessor {
        id: crate::ids::ProcessorId,
        first: FeatureId,
        second: FeatureId,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateAction { id, first, second } => {
                write!(f, "action {id} registered by both {first} and {second}")
            }
            Self::DuplicateMode { id, first, second } => {
                write!(f, "mode {id} registered by both {first} and {second}")
            }
            Self::BadBinding {
                action,
                binding,
                message,
            } => write!(f, "action {action}: binding {binding:?}: {message}"),
            Self::UnknownContext { action, context } => {
                write!(f, "action {action} targets unknown context {context}")
            }
            Self::BindingConflict { context, detail } => {
                write!(f, "keymap conflict in context {context}: {detail}")
            }
            Self::DuplicatePanel { id, first, second } => {
                write!(f, "panel {id} registered by both {first} and {second}")
            }
            Self::DuplicateValidator { id, first, second } => {
                write!(f, "validator {id} registered by both {first} and {second}")
            }
            Self::DuplicateBaker { id, first, second } => {
                write!(f, "baker {id} registered by both {first} and {second}")
            }
            Self::DuplicateProcessor { id, first, second } => {
                write!(f, "processor {id} registered by both {first} and {second}")
            }
        }
    }
}

/// A validated action binding, resolved into a concrete context.
pub struct CompiledBinding {
    pub context: ContextId,
    pub binding: Binding,
    pub action: ActionId,
}

/// The validated output the kernel builds its dispatch tables from.
pub struct ValidatedFeatures {
    pub actions: Vec<(FeatureId, ActionDef)>,
    pub modes: Vec<(FeatureId, ModeDef)>,
    pub bindings: Vec<CompiledBinding>,
    pub components: Vec<(FeatureId, ComponentReg)>,
    pub kinds: Vec<(FeatureId, EntityKindDef)>,
    pub panels: Vec<(FeatureId, PanelDecl)>,
    pub validators: Vec<(FeatureId, ValidatorDef)>,
    pub level_validators: Vec<(FeatureId, LevelValidatorDef)>,
    pub processors: Vec<(FeatureId, ProcessorDef)>,
    pub bakers: Vec<(FeatureId, BakerDef)>,
    pub gizmos: Vec<(FeatureId, crate::gizmos::GizmoDef)>,
}

impl FeatureRegistry {
    /// Validate everything registered (M1 acceptance A2/A3): duplicate ids, unparsable
    /// bindings, unknown contexts, per-context conflicts. All errors reported at once.
    pub fn validate(self) -> Result<ValidatedFeatures, Vec<RegistryError>> {
        let mut errors = Vec::new();

        let mut seen_actions: HashMap<ActionId, FeatureId> = HashMap::new();
        for (feature, def) in &self.actions {
            if let Some(first) = seen_actions.get(&def.id) {
                errors.push(RegistryError::DuplicateAction {
                    id: def.id.clone(),
                    first: first.clone(),
                    second: feature.clone(),
                });
            } else {
                seen_actions.insert(def.id.clone(), feature.clone());
            }
        }

        let mut seen_modes: HashMap<ModeId, FeatureId> = HashMap::new();
        for (feature, def) in &self.modes {
            if let Some(first) = seen_modes.get(&def.id) {
                errors.push(RegistryError::DuplicateMode {
                    id: def.id.clone(),
                    first: first.clone(),
                    second: feature.clone(),
                });
            } else {
                seen_modes.insert(def.id.clone(), feature.clone());
            }
        }

        // Known contexts: global + one per mode + registered overlay contexts.
        let mut known_contexts: HashSet<ContextId> = HashSet::new();
        known_contexts.insert(GLOBAL_CONTEXT);
        for (_, mode) in &self.modes {
            known_contexts.insert(ContextId::new(mode.id.as_str().to_string()));
        }
        for (_, context) in &self.contexts {
            known_contexts.insert(context.clone());
        }
        // Panel focus contexts are registered implicitly by the panel declaration.
        for (_, panel) in &self.panels {
            known_contexts.insert(panel.context.clone());
        }

        let mut seen_validators: HashMap<crate::ids::ValidatorId, FeatureId> = HashMap::new();
        for (feature, validator) in &self.validators {
            if let Some(first) = seen_validators.get(&validator.id) {
                errors.push(RegistryError::DuplicateValidator {
                    id: validator.id.clone(),
                    first: first.clone(),
                    second: feature.clone(),
                });
            } else {
                seen_validators.insert(validator.id.clone(), feature.clone());
            }
        }

        let mut seen_bakers: HashMap<crate::ids::BakerId, FeatureId> = HashMap::new();
        for (feature, baker) in &self.bakers {
            if let Some(first) = seen_bakers.get(&baker.id) {
                errors.push(RegistryError::DuplicateBaker {
                    id: baker.id.clone(),
                    first: first.clone(),
                    second: feature.clone(),
                });
            } else {
                seen_bakers.insert(baker.id.clone(), feature.clone());
            }
        }

        let mut seen_processors: HashMap<crate::ids::ProcessorId, FeatureId> = HashMap::new();
        for (feature, processor) in &self.processors {
            if let Some(first) = seen_processors.get(&processor.id) {
                errors.push(RegistryError::DuplicateProcessor {
                    id: processor.id.clone(),
                    first: first.clone(),
                    second: feature.clone(),
                });
            } else {
                seen_processors.insert(processor.id.clone(), feature.clone());
            }
        }

        let mut seen_panels: HashMap<crate::ids::PanelId, FeatureId> = HashMap::new();
        for (feature, panel) in &self.panels {
            if let Some(first) = seen_panels.get(&panel.id) {
                errors.push(RegistryError::DuplicatePanel {
                    id: panel.id.clone(),
                    first: first.clone(),
                    second: feature.clone(),
                });
            } else {
                seen_panels.insert(panel.id.clone(), feature.clone());
            }
        }

        let mut bindings = Vec::new();
        for (_, def) in &self.actions {
            let contexts: Vec<ContextId> = if def.contexts.is_empty() {
                vec![GLOBAL_CONTEXT]
            } else {
                def.contexts.clone()
            };
            for context in &contexts {
                if !known_contexts.contains(context) {
                    errors.push(RegistryError::UnknownContext {
                        action: def.id.clone(),
                        context: context.clone(),
                    });
                }
            }
            for raw in &def.default_bindings {
                match raw.parse::<Binding>() {
                    Ok(binding) => {
                        for context in &contexts {
                            bindings.push(CompiledBinding {
                                context: context.clone(),
                                binding: binding.clone(),
                                action: def.id.clone(),
                            });
                        }
                    }
                    Err(e) => errors.push(RegistryError::BadBinding {
                        action: def.id.clone(),
                        binding: raw.to_string(),
                        message: e.message,
                    }),
                }
            }
        }

        // Per-context conflict detection.
        let mut by_context: HashMap<&ContextId, Vec<(Binding, String)>> = HashMap::new();
        for compiled in &bindings {
            by_context
                .entry(&compiled.context)
                .or_default()
                .push((compiled.binding.clone(), compiled.action.to_string()));
        }
        for (context, entries) in &by_context {
            for conflict in find_conflicts(entries) {
                errors.push(RegistryError::BindingConflict {
                    context: (*context).clone(),
                    detail: conflict.to_string(),
                });
            }
        }

        if errors.is_empty() {
            Ok(ValidatedFeatures {
                actions: self.actions,
                modes: self.modes,
                bindings,
                components: self.components,
                kinds: self.kinds,
                panels: self.panels,
                validators: self.validators,
                level_validators: self.level_validators,
                processors: self.processors,
                bakers: self.bakers,
                gizmos: self.gizmos,
            })
        } else {
            Err(errors)
        }
    }
}

/// Queue features push into before the kernel exists; `editor_core` drains it at
/// startup. Inert without the kernel.
#[derive(Resource, Default)]
pub struct PendingFeatures(pub Vec<Box<dyn EditorFeature>>);

pub trait EditorAppExt {
    fn add_editor_feature(&mut self, feature: impl EditorFeature) -> &mut Self;
}

impl EditorAppExt for App {
    fn add_editor_feature(&mut self, feature: impl EditorFeature) -> &mut Self {
        self.world_mut()
            .get_resource_or_insert_with(PendingFeatures::default)
            .0
            .push(Box::new(feature));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FeatA;
    impl EditorFeature for FeatA {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("feat-a", "Feature A")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.mode(ModeDef::new("normal", "Normal"))
                .action(ActionDef::new("a.undo", "Undo").bind("u"))
                .action(
                    ActionDef::new("a.top", "Go Top")
                        .context("normal")
                        .bind("g g"),
                );
        }
    }

    struct FeatDup;
    impl EditorFeature for FeatDup {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("feat-dup", "Duplicator")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            // duplicate action id, conflicting binding, unknown context, bad parse
            reg.action(ActionDef::new("a.undo", "Undo Again").bind("u"))
                .action(
                    ActionDef::new("d.greedy", "Greedy")
                        .context("normal")
                        .bind("g"),
                )
                .action(
                    ActionDef::new("d.lost", "Lost")
                        .context("no-such-mode")
                        .bind("x"),
                )
                .action(ActionDef::new("d.bad", "Bad").bind("ctrl+wibble"));
        }
    }

    // A3 (clean path)
    #[test]
    fn valid_feature_validates() {
        let mut reg = FeatureRegistry::default();
        reg.register_feature(&FeatA);
        let validated = reg.validate().expect("should validate");
        assert_eq!(validated.actions.len(), 2);
        assert_eq!(validated.modes.len(), 1);
        assert_eq!(validated.bindings.len(), 2);
    }

    // A2 + A3 (error path): every problem reported, each naming its culprits
    #[test]
    fn all_errors_reported_with_names() {
        let mut reg = FeatureRegistry::default();
        reg.register_feature(&FeatA);
        reg.register_feature(&FeatDup);
        let Err(errors) = reg.validate() else {
            panic!("expected validation errors")
        };

        assert!(errors.iter().any(|e| matches!(e,
            RegistryError::DuplicateAction { first, second, .. }
                if first.as_str() == "feat-a" && second.as_str() == "feat-dup")));
        assert!(errors.iter().any(|e| matches!(e,
            RegistryError::BindingConflict { context, detail }
                if context.as_str() == "normal" && detail.contains("shadows"))));
        assert!(errors.iter().any(|e| matches!(e,
            RegistryError::UnknownContext { context, .. }
                if context.as_str() == "no-such-mode")));
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, RegistryError::BadBinding { .. }))
        );
        // duplicate "u" in global: FeatA's undo vs FeatDup's undo — same id though;
        // duplicate-id already reported; binding duplicate also surfaces:
        assert!(errors.iter().any(|e| matches!(e,
            RegistryError::BindingConflict { context, .. }
                if context.as_str() == "global")));
    }

    // Same binding in DIFFERENT contexts is not a conflict (A2)
    #[test]
    fn same_binding_different_contexts_ok() {
        struct F;
        impl EditorFeature for F {
            fn manifest(&self) -> FeatureManifest {
                FeatureManifest::new("f", "F")
            }
            fn register(&self, reg: &mut FeatureRegistry) {
                reg.mode(ModeDef::new("normal", "Normal"))
                    .mode(ModeDef::new("insert", "Insert"))
                    .action(ActionDef::new("f.a", "A").context("normal").bind("x"))
                    .action(ActionDef::new("f.b", "B").context("insert").bind("x"));
            }
        }
        let mut reg = FeatureRegistry::default();
        reg.register_feature(&F);
        assert!(reg.validate().is_ok());
    }
}
