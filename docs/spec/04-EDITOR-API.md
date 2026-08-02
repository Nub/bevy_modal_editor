# RFC: `editor_api` — the Feature Contract Crate

> Design deliverable for Milestone 0 (see `02-RECREATION-PROMPT.md` §2, §10). Status:
> **draft for owner review**. Signatures are illustrative-but-concrete: naming and exact
> generics may shift during implementation, the *shapes* and *responsibilities* may not
> without amending this document.

## 1. Goals & constraints (from the spec)

- **The north star: a crate provides game/engine features and editor interfaces
  *seamlessly*.** The particle system is the canonical example: the editor does not
  provide particles — `bevy_vfx` provides a mature runtime *and* a mature authoring UI,
  and `editor_api` is the seam that makes the second half possible without the first
  half knowing the editor exists. If this API makes a feature crate's editor UI feel
  second-class next to "built-in" panels, the API has failed its purpose.
- `editor_api` is the **ecosystem surface**: any crate anywhere becomes an editor feature
  by implementing it behind its own `editor` cargo feature. First-party and third-party
  crates use the identical path.
- Dependencies: **Bevy only** (plus the abstract property model defined here). Never
  `editor_core`, `editor_ui`, or `game_framework`. Must stay cheap to adopt.
- **No side door**: every mutation a feature performs flows through the edit pipeline so
  undo/redo/macros/headless-scripting hold universally.
- Registration is **declarative data through one builder**; nothing works by side effect.
- Semver-stable above all other crates. Pre-1.0: breaking changes allowed only at minor
  bumps, each with a migration note; post-1.0: deprecation cycle of ≥1 minor before
  removal.

## 2. Crate layout

```
editor_api/
├── feature.rs      EditorFeature, FeatureManifest, FeatureRegistry
├── ids.rs          FeatureId, ActionId, ContextId, ModeId, PanelId, SceneId (UUID)
├── actions.rs      ActionDef, ActionInvoked, ParamsSpec, ActionFlags
├── keymap.rs       Binding, KeySequence, ContextId layering rules
├── edits.rs        EditScope, Transaction, EditOp, built-in ops, EditError
├── components.rs   ComponentOpts, migrators, PropertyHint (abstract property model)
├── kinds.rs        EntityKindDef, PreviewMode
├── panels.rs       PanelDecl, Placement, PanelContent
├── gizmos.rs       GizmoCtx, HandleId (draw + pick from one geometry)
├── validate.rs     ValidatorDef, Problem, Severity
├── pipeline.rs     ImporterDef, ProcessorDef, BakerDef, content-hash types
├── conformance.rs  test harness feature crates run against their own impl
└── ui.rs           [feature "ui"] PanelUi, PanelCtx, WidgetKit trait
```

## 3. The feature entry point

```rust
pub trait EditorFeature: Send + Sync + 'static {
    fn manifest(&self) -> FeatureManifest;
    fn register(&self, reg: &mut FeatureRegistry);
}

pub struct FeatureManifest {
    pub id: FeatureId,                 // "vfx", "splines" — stable, kebab-case
    pub name: &'static str,
    pub api_version: ApiVersion,       // the editor_api version this was built against
    pub requires: &'static [FeatureId],// hard deps on other features (rare; discouraged)
}
```

Wiring: `editor_api` provides an `App` extension —

```rust
pub trait EditorAppExt {
    fn add_editor_feature(&mut self, feature: impl EditorFeature) -> &mut Self;
}
```

— implemented by pushing into a `PendingFeatures` resource that `editor_core` (when
present) drains at startup, resolves `requires` ordering for, and registers. A feature
crate's own Bevy plugin calls this under `#[cfg(feature = "editor")]`; without
`editor_core` in the app the pending queue is inert. Runtime systems (simulation,
rendering) are added by the feature's normal plugin as usual — `editor_api` governs only
the *editor-facing* surface.

### Relationship to `game_framework` (state is a provided pattern, not per-game)

`editor_api` never depends on `game_framework` — but **feature crates' runtime halves
should**. Game state management (`Editing/Playing/Paused`, session flow, level lifecycle)
is a pattern the framework provides, not something each game or feature invents: a
feature's simulation systems gate on the framework's states (`run_if(in_state(...))`)
rather than shipping their own pause flags, and respond to its lifecycle events
(`LevelReady`, `ResetRequested`) rather than defining parallel ones. This is what makes
play/pause/reset, replay, and networking behave uniformly across every feature — v1's
per-feature restart markers (`VfxRestart`) and ad-hoc pause checks are the anti-pattern.
The kernel enforces the editor half: editor-registered systems run only in editing
contexts unless a registration flag says otherwise.

Registry shape (each method returns a typed builder; all registration is validated at
startup, with duplicate IDs / unknown contexts / conflicting bindings as hard errors):

```rust
impl FeatureRegistry {
    pub fn action(&mut self, def: ActionDef) -> &mut Self;
    pub fn mode(&mut self, def: ModeDef) -> &mut Self;
    pub fn panel(&mut self, def: PanelDecl) -> &mut Self;
    pub fn component<T: EditorComponent>(&mut self, opts: ComponentOpts<T>) -> &mut Self;
    pub fn entity_kind(&mut self, def: EntityKindDef) -> &mut Self;
    pub fn gizmos(&mut self, f: GizmoFn) -> &mut Self;
    pub fn validator(&mut self, def: ValidatorDef) -> &mut Self;
    pub fn importer(&mut self, def: ImporterDef) -> &mut Self;
    pub fn processor(&mut self, def: ProcessorDef) -> &mut Self;
    pub fn baker(&mut self, def: BakerDef) -> &mut Self;
}
```

## 4. Actions & keymaps (the macro substrate)

Actions are **data**; invocation is an **event**; features react with ordinary systems.
This indirection is what makes macros, palettes, which-key, and headless tests free.

```rust
pub struct ActionDef {
    pub id: ActionId,                       // "splines.point.add"
    pub name: &'static str,                 // "Add Control Point"
    pub description: &'static str,
    pub contexts: &'static [ContextId],     // keymap layers where valid (mode, panel, gesture)
    pub default_bindings: &'static [Binding], // parsed key sequences; may be empty (palette-only)
    pub params: ParamsSpec,                 // Reflect-described args; palette prompts + macro serialization
    pub flags: ActionFlags,                 // IS_EDIT (must open a transaction), HIDDEN, REPEATABLE ...
}

/// Emitted by the input resolver (or palette, macro player, test driver). Never
/// constructed by feature code to "call" another feature — features expose actions,
/// the kernel invokes them.
#[derive(Message)]
pub struct ActionInvoked {
    pub action: ActionId,
    pub args: Box<dyn PartialReflect>,      // matches ParamsSpec; serializable => recordable
    pub source: InvocationSource,           // Key, Palette, Macro(reg), Script, Test
}
```

A macro is `Vec<(ActionId, serialized args)>` — replayable through the same event, one
coalesced undo entry. `ActionFlags::IS_EDIT` is enforced: the kernel verifies that
handling such an action produced exactly one transaction (a CI-testable invariant).

Keymap contexts: each mode and panel registration auto-creates a `ContextId` layer;
bindings are data files layered defaults→user (per `03-KEYMAP-DESIGN.md`). Conflict
detection runs at load across all registered features.

## 5. Edits (`EditScope`) — the only door

```rust
/// SystemParam facade features use to mutate scene content.
pub struct EditScope<'w, 's> { /* kernel-owned queues */ }

impl EditScope<'_, '_> {
    /// Open a transaction; label appears in undo history. Dropping without commit aborts.
    pub fn transaction(&mut self, label: impl Into<String>) -> Transaction<'_>;
}

impl Transaction<'_> {
    pub fn set_component<T: EditorComponent>(&mut self, target: SceneId, value: T);
    /// ReflectPatch is BSN-patch-compatible by construction (spec §5 "one delta
    /// language"): the same per-field delta representation serves undo, prefab
    /// overrides, and BSN scene patches.
    pub fn patch_component<T: EditorComponent>(&mut self, target: SceneId, patch: ReflectPatch);
    pub fn insert<T: EditorComponent>(&mut self, target: SceneId, value: T);
    pub fn remove<T: EditorComponent>(&mut self, target: SceneId);
    pub fn spawn(&mut self, kind: &EntityKindId, at: Placement) -> PendingSceneId;
    pub fn spawn_prefab(&mut self, prefab: PrefabId, at: Placement) -> PendingSceneId;
    pub fn despawn(&mut self, target: SceneId);
    pub fn reparent(&mut self, target: SceneId, new_parent: Option<SceneId>);
    pub fn custom(&mut self, op: Box<dyn EditOp>);      // escape into typed custom ops
    pub fn commit(self);
}

/// Custom operation for domains the built-ins can't express (terrain tile deltas,
/// spline point topology). Must be Reflect (serialization for macros/journal) and
/// must return its inverse.
pub trait EditOp: Send + Sync + Reflect {
    fn apply(&mut self, world: &mut World, cx: &mut EditCx) -> Result<Box<dyn EditOp>, EditError>;
    /// Gesture coalescing: return true if `next` was absorbed (drag frames, brush strokes).
    fn coalesce(&mut self, next: &dyn EditOp) -> bool { false }
}
```

- Targets are `SceneId` (serialized UUID component) — never `Entity` (undo survives
  respawn; references survive renames).
- The kernel applies queued transactions at a fixed point in the frame
  (`EditorSet::Mutate`), captures inverses, records history, marks dirty state, and
  emits `Edited { targets }` events that derived systems (regenerate hooks, bakes,
  navmesh) react to. Features never write scene components directly — the §8 fitness
  grep (`&mut World` outside kernel/port-gated code) enforces this.

## 6. Components & the abstract property model

```rust
/// Blanket-implemented marker: Component + Reflect + FromReflect + Serialize + Deserialize + PartialEq + Clone.
pub trait EditorComponent: /* … */ {}

pub struct ComponentOpts<T> {
    pub version: u32,
    pub migrate: Option<fn(from: u32, value: TypedValue) -> Result<TypedValue, MigrateError>>,
    pub on_regenerate: Option<RegenerateHook>,  // derive runtime state (mesh, collider, light)
    pub inspector: InspectorOpts,               // category, display name, per-field PropertyHint
    pub dirty_tracking: DirtyTracking,          // default: On — participates in scene-modified detection
    pub _marker: PhantomData<T>,
}

/// Abstract property model: HINTS, not widgets. This is what game_framework-registered
/// components also use — no egui anywhere in this path.
pub enum PropertyHint {
    Range { min: f64, max: f64, step: f64 },
    Color { alpha: bool },
    AssetRef { kind: AssetKind },
    Curve, Angle, Layer, Multiline,
    EnumLabels(&'static [&'static str]),
    Hidden, ReadOnly,
}
```

One call registers: reflection, serialization allow-list, dirty tracking, undo capture,
migrator chain, inspector metadata, regenerate hook. The spec's "one registration point
per component" lives here.

## 7. Entity kinds, gizmos, validators

```rust
pub struct EntityKindDef {
    pub id: EntityKindId,                    // "splines.catmull-rom"
    pub display_name: &'static str,
    pub category: &'static str,              // palette grouping
    pub spawn: fn(&mut KindSpawnCx),         // attach semantic components only
    pub preview: PreviewMode,                // InsertGhost | None (ghost derives from regenerate)
}

/// Gizmo drawing and picking share one geometry description (spec §4 requirement).
pub struct GizmoCtx<'a> { /* wraps bevy Gizmos + pick registration */ }
impl GizmoCtx<'_> {
    pub fn line(&mut self, a: Vec3, b: Vec3, style: GizmoStyle);
    pub fn circle(&mut self, ...); // etc.
    /// Registers pickable geometry; hover/click resolve through the kernel's single
    /// pick-arbitration pass and arrive as actions with the HandleId as param.
    pub fn handle(&mut self, id: HandleId, geometry: HandleGeometry, on_drag: ActionId);
}

pub struct ValidatorDef {
    pub id: &'static str,
    pub severity: Severity,                  // Error blocks publish gate; Warning doesn't
    pub run: fn(&ValidationCx) -> Vec<Problem>,
}
```

## 8. Pipeline hooks (importers, processors, bakers)

```rust
pub struct ImporterDef {
    pub extensions: &'static [&'static str],
    pub import: fn(&mut ImportCx) -> Result<ImportOutput, ImportError>,
    /// Propose layout/prefab metadata (sockets from DCC naming, bounds footprints).
    pub propose_metadata: Option<fn(&ImportOutput) -> Vec<ProposedComponent>>,
}

pub struct BakerDef {
    pub id: BakerId,
    pub version: u32,                        // bumping invalidates all caches
    pub inputs: fn(&BakeInputCx) -> ContentHash,   // hash of source data incl. seeds
    pub bake: fn(&mut BakeCx) -> Result<BakeArtifacts, BakeError>,
    pub on_override_invalidation: OverridePolicy,  // FallbackDynamic | PerInstanceBake
}
```

Determinism is enforced by the conformance harness: bake twice from identical inputs,
byte-compare (spec §6 invariant).

## 9. Panels & UI — the one designed-in tension

The coherent-UI pillar wants every panel built from one widget kit; real features (a VFX
module-stack editor, a node graph someday) need more than a property grid. Resolution:

```rust
pub struct PanelDecl {
    pub id: PanelId,
    pub title: &'static str,
    pub placement: Placement,                // Right, Left, Bottom, Floating
    pub context: ContextId,                  // keymap layer while focused
    pub content: PanelContent,
}

pub enum PanelContent {
    /// Zero-UI-code path: reflection inspector over a query/selection, using
    /// PropertyHints. Most panels should be this.
    Properties(PropertySource),
    /// Custom path: feature also enables editor_api/ui and registers a PanelUi.
    Custom,
}

// feature "ui" — built on bevy_ui + the official feathers/bevy_ui_widgets stack
// (spec §7). No egui anywhere in the contract.
pub trait PanelUi: Send + Sync {
    fn ui(&mut self, cx: &mut PanelCtx);
}
pub struct PanelCtx<'a> {
    pub kit: WidgetKitRef<'a>,   // property rows, cards, sections, lists, previews —
                                 //   trait defined here, implemented by editor_ui over
                                 //   feathers primitives (backend swappable behind it)
    pub edits: &'a mut EditScope<'a, 'a>,
    pub selection: &'a SelectionView,
    // …read-only world views
}
impl WidgetKitRef<'_> {
    /// ESCAPE HATCH — drop below the kit to the underlying UI builder (bevy_ui/feathers
    /// node construction). Kit-first is the rule; every raw() use must be justified in
    /// the PR (checklist item) and is a candidate for kit promotion — with feathers
    /// gaps preferring an upstream contribution over a local widget.
    pub fn raw(&mut self) -> RawUiBuilder<'_>;
}
```

Consequences, stated honestly: enabling `editor_api/ui` pulls the bevy_ui/feathers
widget stack into that feature crate's `editor` feature (never its runtime). Because the
kit is a trait, panels survive a UI-backend change (the M0 spike's egui fallback rides
this seam). `game_framework`-registered components never touch the ui module — hints
only.

## 10. Conformance harness

`editor_api::conformance` ships a test entry point every feature crate runs in CI:

```rust
#[test]
fn conforms() { editor_api::conformance::check(MyFeature::default()); }
```

Checks: manifest sanity; unique/parseable IDs and bindings; every `IS_EDIT` action opens
exactly one transaction when invoked headlessly; params round-trip through
serialization (macro-safety); bakers are deterministic; registered components satisfy
`EditorComponent` bounds and migrator chains are gap-free (v1→v2→…→current). The
harness is the contract-in-practice: if it passes, the feature composes.

## 11. Paper validation (required before freezing)

Walk two real crates through this API on paper — worked examples to be appended here:
1. **bevy_spline_3d**: mode + control-point actions + `GizmoCtx::handle` drags +
   `Properties` panel + `Spline` component with regenerate hook + custom `EditOp` for
   point topology. Expected: no `ui` feature needed. Validates §4–§7.
2. **bevy_vfx**: module-stack `Custom` panel (validates the §9 escape hatch), preset
   assets, emitter component regeneration, zero bakers/importers. Validates §9 and asset
   integration.

Anything either walkthrough cannot express cleanly is a design bug in this RFC — fix the
RFC, not the crate.

### 11.1 Walkthrough: `bevy_spline_3d` (core-path validation — no `ui` feature)

```rust
impl EditorFeature for SplinesFeature {
    fn manifest(&self) -> FeatureManifest { /* id: "splines" */ }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.component::<Spline>(ComponentOpts {
              version: 1,
              on_regenerate: Some(rebuild_spline_mesh),   // mesh + samples from control points
              inspector: props!{ tension: Range(0.0..=1.0), closed: _, kind: EnumLabels(..) },
              ..default() })
           .component::<SplineFollower>(/* refs by SceneId, PropertyHint::AssetRef-free */)
           .entity_kind(EntityKindDef { id: "splines.catmull-rom", spawn: attach_default_spline, .. })
           .mode(ModeDef { id: "spline-edit", entered_by: Descend, .. }) // Enter on a spline
           .action(ActionDef { id: "splines.point.add",    contexts: &[SPLINE_EDIT], flags: IS_EDIT, .. })
           .action(ActionDef { id: "splines.point.delete", .. })
           .action(ActionDef { id: "splines.point.move",   params: params!{ handle: HandleId, delta: Vec3 }, .. })
           .action(ActionDef { id: "splines.toggle-closed", .. })
           .gizmos(draw_spline_gizmos)   // curve polyline + one handle() per control point,
                                         //   on_drag: "splines.point.move"
           .validator(ValidatorDef { id: "splines.degenerate", run: fewer_than_two_points, .. });
    }
}

// Point moves: a custom EditOp so a whole drag coalesces to one undo entry.
struct MovePoint { target: SceneId, index: usize, delta: Vec3 }
impl EditOp for MovePoint {
    fn apply(..) -> inverse MovePoint { .. }
    fn coalesce(&mut self, next) -> bool { same target+index => accumulate delta }
}
// Topology (add/delete) = custom EditOps too; everything else is patch_component.
```

**Result: clean fit.** Mode via fractal descend, handle drags arriving as actions,
coalescing EditOps, `Properties` panel, zero egui. Validates §4–§7 as designed.

### 11.2 Walkthrough: `bevy_vfx` (custom-UI path + assets)

```rust
reg.component::<VfxSystem>(ComponentOpts { on_regenerate: Some(spawn_gpu_emitters), .. })
   .entity_kind(EntityKindDef { id: "vfx.emitter", .. })
   .asset_kind(AssetKindDef { id: "vfx.preset", extensions: &["vfx.ron"], .. })  // ← see gap #1
   .action(ActionDef { id: "vfx.module.add", params: params!{ kind: ModuleKind }, flags: IS_EDIT, .. })
   .action(ActionDef { id: "vfx.preset.save-as", params: params!{ name: String }, .. })
   .panel(PanelDecl { id: "vfx.editor", content: PanelContent::Custom, .. });

impl PanelUi for VfxPanel {
    fn ui(&mut self, cx: &mut PanelCtx) {
        for (i, module) in system.modules() {
            cx.kit.card(module.label(), |kit| {          // shared card widget — v1 had 2 copies
                kit.properties_of(&module);              // reflection rows inside custom layout
                kit.raw().add(CurveEditor::new(..));     // justified raw(): curve widget,
            });                                          //   promotion candidate for the kit
        }
        // every mutation: cx.edits.transaction("Edit emitter").patch_component::<VfxSystem>(..)
    }
}
// Runtime half: simulation systems run_if(in_state(GameState::Playing)) via game_framework —
// no VfxRestart-style private state (the v1 anti-pattern named in §3).
```

**Result: fits, and found two gaps** (the walkthrough doing its job):

- **Gap #1 — asset kinds.** The registry had no way to declare a feature's asset type so
  the asset browser/palette can list, preview, and create them. **Fixed in this RFC:**
  `reg.asset_kind(AssetKindDef { id, extensions, display_name, create_default })` added
  to §3's registry surface; `PropertyHint::AssetRef { kind }` references it.
- **Gap #2 — asset mutation is a different domain than scene mutation.** `EditScope`
  targets `SceneId`s; editing a *preset asset* (or library material) mutates an asset,
  not the scene. Decision recorded: assets get an explicit save model (dirty flag +
  save/save-as actions + unsaved-changes guard) in the core; a parallel undoable
  `AssetEditScope` is designed-for but post-core — asset edits before then are
  dirty-tracked but not undoable, stated honestly in the UI. This mirrors how DCCs
  actually behave and avoids inventing a second undo system under time pressure.

### Port-gate note: concrete v1 reuse through this API

Per the quarantine policy (`02` §11), keep-list code enters via port gates. Verified
inventory for the flagship case: `bevy_vfx` carries ~800 lines of WGSL (GPU
spawn/update/compact compute passes, billboard rendering, shared helpers) and ~1,360
lines of GPU plumbing including the hash-keyed persistent buffer cache
(`gpu/prepare.rs`) — real, reusable render engineering. Its port gate: keep shaders +
GPU pipeline + serializable module-stack data model; move the 1,521-line Rust preset
file to RON assets; feature-gate the avian3d dep; implement-or-delete Ribbon; then its
editor half is rebuilt against this API (Custom panel + `EditScope`), making it worked
example #2 in §11 — the port is the API's first real test.

## 12. Resolved decisions (owner review, 2026-08-01)

1. **§9 escape hatch: kit-first + justified `raw()`.** Feature crates must be able to
   ship mature authoring UI (module-stack editors, curve/gradient editors) on day one;
   every `raw()` use is justified in its PR and is a standing candidate for promotion
   into the widget kit, so raw usage shrinks as the kit matures. (Subsequent owner
   decision: the UI stack is bevy_ui + official feathers widgets, egui-free — `raw()`
   exposes the bevy_ui builder, and the WidgetKit trait is the backend seam; see spec
   §7.)
2. **Actions as events, everywhere.** One `ActionInvoked` path for key, palette, macro,
   script, and test invocation. No direct-callback fast path — a macro-invisible channel
   would reopen the side door this architecture exists to close.
3. **`requires` between features: allowed, discouraged.** Startup ordering + hard error
   on missing dependency; the conformance harness warns on use; the CI build matrix must
   still pass with any subset of independent features.

Remaining before freeze: the §11 paper walkthroughs (splines, vfx) appended to this
document, reviewed against these decisions.
