# Recreation Prompt — Studio-Grade Modal Level Editor for Bevy

> This document is a self-contained build prompt. Hand it (plus `01-REVIEW.md` and the v1
> repository as reference material) to the implementing agent/team. It specifies vision,
> architecture, quality bars, and phasing. Where it conflicts with v1 code, this document wins.

---

## 1. Vision

Build **a keyboard-driven, modal level editor and game framework for Bevy** — "neovim for
level editing" — engineered to a standard a game studio can adopt for shipping titles.

Identity statements, in priority order:

1. **Editor-in-game, not game-in-editor.** The editor is a Bevy plugin compiled into the
   *game's* binary behind a cargo feature (`editor`). There is no standalone editor
   executable. Stripping is **compile-time by feature flag, never by release
   optimization**: with the feature off, zero editor code/assets exist in the artifact;
   with it on, **the editor runs wherever the game runs** — an opt-in capability of the
   game on every platform the game compiles for, not a desktop app with its own platform
   list. Dev builds boot into the game with the editor available as an overlay/mode.
2. **Keyboard-first, modal, discoverable.** Every operation is reachable via keystrokes;
   the mouse is an accelerator, never a requirement. Vim conventions where an analogue
   exists; deliberate, documented deviations where 3D demands it. All bindings configurable
   as data.
3. **An opinionated game framework.** The workspace ships not just editing tools but the
   patterns a game is expected to follow: app lifecycle states, level loading, settings,
   session flow. The editor understands and integrates with those patterns; games that adopt
   them get save/load/play-in-editor/hot-reload for free.
4. **Assets flow through a pipeline; prefabs are the product.** Raw files (GLTF, textures,
   audio) are imported, validated, processed, and packaged into **prefabs** — the unit of
   "game-ready." Level content is prefab instances with overrides.
5. **Studio-grade engineering.** Versioned file formats, atomic writes, tested subsystems,
   deterministic system ordering, cross-platform dev environment, CI. Every invariant is
   enforced by construction, not convention (see §8 — this is the core lesson of v1).

### Definition of Done — the 1.0 acceptance scenario

This is the forcing function every phase is measured against. **A team of 1–2 developers
can, without ever modifying editor source code:**

1. **Import a host of raw assets** and turn them into game-ready prefabs (colliders,
   materials, gameplay components, bakes) through the pipeline.
2. **Design those prefabs into a playable level** using rapid prototyping tools — modular
   socket/kit snapping is mandatory here; freeform placement alone is too slow for trialing
   ideas.
3. **Play / edit / replay in a tight loop until a game cycle emerges.** Lots of games come
   from random ideas that need trial; the editor exists to make idea → playable trial take
   minutes, not days.
4. **Publish the result to a small playtest group** — client-server, with one player
   hosting (listen server) to start.

"Editor codebase never changed" is the extensibility acceptance test: any gap a game hits
during this loop must be fillable through `editor_api`/`game_framework` registration. If it
can't be, that is a 1.0-blocking bug in the contract, not an excuse to patch the editor.

**Minimum feature set implied by this scenario** (required for 1.0): screen/post-process
effects, particle effects, animation sequences (a minimal timeline asset — tween/sequence
transforms, properties, and events; distinct from the open skeletal-graph question),
Rust hot code reload in dev builds, in-editor shader authoring with hot reload, and
modular snap-based building kits. The goal, stated once and binding everywhere: **rapid
prototypes of ideas into playable demos.**

### Relationship to the official Bevy Editor

Bevy is building an official editor on the same foundations this spec adopts (BSN,
feathers). That is an asset, not a threat — but the relationship must be explicit:

- **Why this project exists anyway**: the official editor is a general-purpose standalone
  tool. This project's identity is everything the official editor is not aiming at —
  modal/keyboard-first UX, the editor living *inside your game's binary*, the opinionated
  `game_framework`, the prefab/asset/publish pipeline, and the rapid-prototyping DoD.
- **Posture**: adopt upstream relentlessly (widgets, BSN, and — when it ships — any
  official editor infrastructure that's better than ours), differentiate on workflow.
  Sharing foundations makes adoption cheap in both directions; contributions flow
  upstream per the feathers/BSN gap rules.
- **Tail risk, stated honestly**: if the official editor someday covers this niche, the
  feature crates, `game_framework`, and the pipeline survive on their own merits — the
  architecture (independent crates against a small contract) is also the exit strategy.

### Non-goals (v2.0)
- **No in-editor mesh modeling in 1.0, at all.** v1's 10k-line half-edge modeler is cut and
  no reduced op set replaces it. The actual goal it was chasing — rapid level
  layout/design/blockout — is served by parametric blockout shapes (stairs, ramps, arches,
  walls with live-editable dimensions) plus the modular snap-kit system in the
  rapid-prototyping toolkit. If real mesh editing is ever wanted, it returns post-1.0 as an
  external feature crate; meanwhile, DCC tools own modeling and the pipeline owns import.
- No gaussian splatting, no road-intersection authoring in core (defer; design the plugin
  API so they can return as optional plugins). A *minimal* animation-sequence timeline IS
  in scope (see Definition of Done); a full cinematic sequencer is not.
- No custom docking engine, no custom reflection UI from scratch — build on `egui_dock`/
  `egui_tiles` and extend the reflection-editor approach (or evaluate `bevy-inspector-egui`)
  rather than hand-rolling.
- Multiplayer netcode itself is out of scope; the framework only defines the *states and
  events* (connect/disconnect/reconnect) that a netcode crate plugs into.

---

## 2. Workspace Architecture

Kernel-and-plugins. The kernel defines contracts; every feature is a plugin against those
contracts; the game is the binary.

```
workspace/
├── crates/
│   ├── editor_api/         # THE CONTRACT CRATE — small, few deps, semver-stable:
│   │                       #   EditorFeature trait, FeatureRegistry, action/panel/
│   │                       #   component/importer registration types, abstract
│   │                       #   property-model. This is what external crates depend
│   │                       #   on to become editor features.
│   ├── editor_core/        # kernel: actions, keymaps, modes, selection, undo,
│   │                       #   feature registry host, edit-command queue
│   ├── editor_ui/          # panel framework (docking), theme, widget kit,
│   │                       #   fuzzy palette engine, reflection inspector
│   ├── editor_scene/       # versioned serialization, stable IDs, scene I/O
│   ├── editor_assets/      # ingestion pipeline: import → validate → process → cook
│   ├── editor_prefabs/     # prefab authoring: overrides, nesting, variants
│   ├── game_framework/     # opinionated game-side patterns (NO editor deps):
│   │                       #   app lifecycle, level loading, settings, session flow
│   ├── features/           # first-party feature crates — structurally identical to
│   │   │                   #   third-party ones; runtime always builds, editor
│   │   │                   #   integration behind their own `editor` feature:
│   │   ├── bevy_vfx/           particles: runtime sim/render + [editor] authoring
│   │   ├── bevy_spline_3d/     splines: runtime + [editor] control-point editing
│   │   ├── bevy_layout/        assisted layout: five placement paradigms, snap kits,
│   │                       #   parametric shapes, painting + automation verbs
│   ├── bevy_terrain/       heightmap terrain: proc gen + basic sculpt/paint now,
│   │                       #   tile format designed for GPU-driven upgrade later
│   │   ├── bevy_scatter/       procedural placement
│   │   ├── bevy_navmesh_kit/   recast/landmass integration
│   │   └── editor_materials/   grid + channel extensions (merged from v1)
│   ├── support/            # bevy_outliner (render-only, no editor API needed)
│   └── template_game/      # the reference binary: a minimal game built on
│                           #   game_framework with `editor` feature — serves as
│                           #   living documentation, integration test, and starter kit
```

**Feature crates are independent libraries, not editor modules.** A crate like `bevy_vfx`
is first a standalone runtime library any Bevy game can use without the editor existing.
Editor authoring support lives *inside the same crate* behind its own `editor` cargo
feature, implemented against `editor_api` only. This is the inversion that matters:
the editor does not wrap libraries (v1's `SplineEditPlugin` bridge); libraries present
themselves to the editor through published hooks. Third-party crates gain editor support
the exact same way first-party ones do — `editor_api` is the ecosystem surface, so keep it
small, documented, and stable above all other crates.

Dependency rules (enforce with cargo deps + CI check):
- `game_framework` never depends on any `editor_*` crate or egui.
- `editor_api` depends on Bevy (+ the abstract property model) only — never on
  `editor_core`/`editor_ui`; it must stay cheap for libraries to adopt.
- `editor_core` depends on `game_framework` and hosts `editor_api` implementations (the
  editor understands game states, not vice versa).
- Feature crates depend on `editor_api` (under their `editor` feature) — never on
  `editor_core`, `editor_ui`, or each other.
- The game binary composes: `game_framework` + feature-crate runtimes always;
  `EditorPlugin` + each feature crate's `editor` feature under
  `#[cfg(feature = "editor")]`.

### The `editor_api` contract (the heart of modularity)

A feature crate registers everything through one builder; nothing works by side effect:

```rust
impl EditorFeature for SplinesFeature {
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.actions(...)          // named actions with default bindings + palette metadata
           .mode(...)             // optional: a modal mode (id, name, statusline hint)
           .panel(...)            // optional: panels via the EditorPanel trait
           .components(...)       // serializable components: one call registers reflection,
                                  //   serialization allow-list, dirty-tracking, undo capture,
                                  //   inspector metadata, and regenerate hook
           .entity_kind(...)      // spawnable kinds: spawn/preview/display-name/duplicate
           .gizmos(...)           // viewport overlay draw hooks
           .validators(...)       // scene validation rules
           .importers(...)        // asset pipeline importers/processors
           .bakers(...)           // prefab bake steps: precompute heavy derived data
                                  //   at author/cook time (see §6)
    }
}
```

The kernel owns iteration order, input dispatch, and lifecycle. A plugin can be removed from
the build and the editor must still compile and run — CI builds a matrix of feature
combinations to prove it.

**Uniform capability requirement**: every feature's mutations flow through the kernel's
action + `EditQueue` pipeline (§5) — never direct world writes. This is what guarantees that
*every* editor feature, core or plugin, automatically supports undo/redo, macro
record/replay (§4), and headless scripting. A feature that can't express its edits through
the pipeline is not accepted into the registry; the contract has no side door.

---

## 3. Game Framework (opinionated patterns)

`game_framework` defines the canonical structure games follow. Ship it with defaults that
work out of the box and override points for everything.

- **App lifecycle state machine** (Bevy `States` + sub-states):
  `Boot → MainMenu → Settings → LoadingLevel(level_id) → InGame(Session)`, where `Session`
  has sub-states `RoundStart → Playing → RoundEnd`, and orthogonal connection state
  `Connected / Disconnected / Reconnecting` for networked games. Each transition emits typed
  events (`LevelLoadRequested`, `LevelReady`, `RoundStarted`, `PlayerDisconnected`, …).
  Games add systems against these states/events instead of inventing their own flow.
- **Level loading service**: async load of cooked level assets with progress reporting,
  driving the `LoadingLevel` state; the editor's play-in-editor reuses this exact path, so
  "works in editor" implies "works in game."
- **Settings service**: layered config (defaults < user file < runtime), serialized to a
  versioned file, with a registration API so both game and editor settings ride the same
  mechanism (and the editor auto-generates a settings UI from it).
- **Component/entity registration** (evolved from v1's `bevy_editor_game`): games register
  gameplay components, custom entity kinds, validators, and material types through the same
  `FeatureRegistry`-style API the editor plugins use. Fix v1's flaws: closures not bare fn
  pointers, `Result` not panic on unknown kinds, and *no egui types in the contract* — the
  game describes properties abstractly; `editor_ui` renders them.
- **Editor bridge**: when the `editor` feature is on, a keybind/flag switches between game
  and editor at runtime; the editor drives the lifecycle (load level in edit mode, play from
  current state, reset), reusing `GameState` semantics proven in v1.
- **Play / Pause / Reset — hot-in-the-loop testing** (first-class, toolbar + keybound):
  **Play** snapshots the edited scene and enters `InGame` through the real level-loading
  path (same code path as a shipped build — "works in editor" implies "works in game");
  **Pause** freezes simulation while the editor remains fully usable for inspection and
  live-tweaking of the running world; **Reset** restores the pre-play snapshot exactly —
  selection, camera, and undo history intact. Edits made *while playing* are clearly marked
  as ephemeral by default, with an explicit "apply to scene" verb to keep them (the classic
  "lost my play-mode tweaks" failure gets a designed answer, not an accident). Combined with
  asset hot reload (§6), the loop is: tweak → play → observe → reset → tweak, in seconds,
  without leaving the binary.

`template_game` must exercise every one of these patterns and be kept compiling in CI — it is
the successor to v1's demo crates, which were the strongest evidence the v1 API worked.

---

## 4. Interaction Model (keyboard-first, modal)

### Action + keymap system (build this first; everything depends on it)
- Every operation is a **named `Action`** (stable string ID, description, palette metadata).
  Feature plugins contribute actions; nothing but the resolver reads `ButtonInput<KeyCode>`.
- **One input resolver**: `(mode, modifier state, key sequence) → Action`, from a declarative
  keymap. Applies focus/egui/game-state gates exactly once. Emits actions as events;
  systems consume actions, never keycodes. Mouse clicks route through one pick-arbitration
  pass with explicit priority (gizmo handle > control point > entity > empty click).
- **Keymaps are data**: shipped defaults are a keymap file (RON/TOML) like any user keymap;
  user files layer over defaults; per-mode tables; multi-key sequences supported (leader
  keys, `g`-prefix chords); conflict detection at load with actionable errors.
- **Settings are data** *(amended at M2 close)*: every user-tunable static — type scale,
  selection-outline color/width, ghost tint, camera fly speed/boost/sensitivity, grid
  step, chrome timings, palette caps — lives in ONE serde-ready `EditorSettings`
  resource (`editor_core`), defaults in code, `#[serde(default)]` at every level for
  forward compatibility. A user `editor-settings.ron` layering over defaults lands
  beside the keymap file (same pattern). Design tokens that define the chrome's
  identity (spacing/radius scales, glyphs) stay code constants — user settings tune
  the editor, they don't fork the design system.
- **Discoverability**: which-key popup on held prefix/mode entry showing available bindings;
  every action searchable in the command palette with its current binding displayed; a
  generated cheat-sheet view.
- **Macros** *(deferred — owner decision, M2 2026-08-02)*: vim-style record/replay
  (`q<reg>` record, `q` stop, `@<reg>` replay, `<count>@<reg>` repeat) implemented *at the
  action layer* — a macro is a recorded sequence of resolved actions (with their
  parameters), replayed through the same dispatch path as live input, undoable as a single
  coalesced entry. **Precondition discovered in M2**: the action stream alone is not a
  faithful recording surface — placement clicks and drag gestures are mouse input, not
  actions, so a replay re-enters modes without reproducing the edits. Macros return only
  after actions are *parameterized* (spatial inputs — placement position, drag delta —
  carried as action arguments, i.e. gestures-as-data). Until then the coalescing machinery
  (`MergeFrameEntries`) remains as the substrate for scripted batch invocations and
  headless tests.

### Default keymap design
Redesign from scratch — v1 bindings are reference input, not requirements. Principles:
- Vim muscle memory where an analogue exists: `i` insert, `v` visual/selection semantics,
  `d`/`y`/`p` delete/yank/paste entities, `u`/`Ctrl-r` undo/redo, `/` find-object, `:` or
  Space-leader for the palette, marks (`m`/`'`) for camera positions (v1's camera-marks
  design carries over), `gg`-style jumps in hierarchy.
- Where 3D has no vim analogue, decide by frequency ergonomics (home row first, chord depth
  proportional to rarity) and document each decision with rationale — e.g. explicitly choose
  between v1's Q/W/E/R/T transform ops and Blender's G/R/S; either is fine, *accidental* is
  not.
- Deliver as `docs/keymap-design.md` in the new repo before implementing panels: full
  per-mode tables + rationale. This is a design deliverable, not an afterthought.
  A reviewed draft exists at `03-KEYMAP-DESIGN.md` alongside this document — start from it.
- Counts and repeat (`3<nudge>`, `.` repeat-last-action) and registers for yank/paste are
  in scope for the action system's design even if implemented in a later phase.

### Modes
Fewer, stronger modes than v1's eleven. Core set: **Normal** (navigate/select/camera),
**Edit** (transform ops on selection), **Insert** (place entities with live preview) —
plus panel-focused contexts (hierarchy, inspector) that are *focus targets with their own
keymap layers*, not top-level modes. Feature plugins may register modes (blockout, spline
editing) through the registry. Escape always walks home to Normal. Every mode shows its
key hints in the status line (v1 did this well).

### Carried interaction designs (treat as spec, reimplement cleanly)
- Camera marks/registers; ortho-view keys; jump-back stack.
- Snap sub-modes (Surface/Center/Aligned/Vertex) as `PlacementSolver` implementations
  shared by insert-preview, snap-to-object, and paste — one implementation, three consumers
  (v1 forked this logic; don't).
- Screen-space 1:1 constrained-drag math (port `calculate_axis_movement` — the math, not the
  file). Free-drag must use the same screen-space approach so all drags feel identical.
- Alt-drag AABB edge snapping with visual guides.
- **Gizmo interaction as an explicit state machine**:
  `Idle → Hovering(handle) → Dragging{originals, handle, start_ray} → Commit | Cancel`.
  Escape cancels and restores originals; one undo entry per gesture; gizmo geometry defined
  once and used for both rendering and hit-testing.

---

## 5. Scene & Data Layer

### BSN-first policy (binding across this section, §6, and §7)

Bevy 0.19's next-gen scene system (BSN) is upstream's answer to problems this spec also
solves. Policy: **adopt BSN semantics everywhere, adopt BSN implementation wherever it
exists, and where BSN doesn't yet cover a need, build the minimal gap-filler with a
designed convergence path — recorded in a tracked BSN gap ledger** (`docs/bsn-ledger.md`
in the v2 repo, reviewed at every phase-boundary Bevy upgrade). Never fork BSN; prefer
upstream contributions for gaps, mirroring the feathers rule.

**Adopt now (shipped in 0.19):**
- **Patches as the one delta language.** BSN patches are per-field component deltas —
  exactly what prefab instance overrides, undo deltas, and property edits need. All three
  speak the same representation (`ReflectPatch` in `04-EDITOR-API.md` is BSN-patch-
  compatible by construction), so "what changed" has one answer everywhere.
- **Scene inheritance for prefab variants**; BSN templates/composition for prefab
  nesting; dependency-aware loading for asset-before-scene ordering; BSN entity
  references where they fit.
- **BSN scenes as the runtime instantiation path** wherever the editor spawns composed
  content (prefab stamping, UI documents, kit assemblies).

**Known gaps (as of 0.19) and how we prepare:**
1. **Asset-driven workflow (.bsn files on disk) has not shipped.** Our on-disk format
   remains the versioned envelope — but its payload *mirrors BSN semantics* (per-field
   patches, inheritance references, no expanded trees) so that when .bsn assets land,
   convergence is a mechanical migration (the envelope wraps or references .bsn), not a
   redesign. Ledger item #1; re-evaluated every Bevy release.
2. **Versioning/migration: BSN has none.** The envelope + migrator chain is *permanent
   architecture*, not a stopgap — it wraps whatever payload format, including future
   .bsn.
3. **Stable UUID identity.** `SceneId` remains ours; it rides inside BSN payloads as an
   ordinary component and all editor references stay UUID-based regardless of BSN's
   reference mechanics.
4. **Cell partitioning, merge-first layout, streaming** — ours; each cell's payload is a
   BSN-semantic document.
5. **Editing model.** BSN has no notion of edits — `EditQueue`/undo/macros are ours
   entirely, but EditOps express component changes as BSN-style patches (see "one delta
   language" above).

- **One registration point per component** (via `FeatureRegistry::components`): reflection,
  allow-list, dirty-flag tracking, inspector metadata, and regenerate hook in a single call.
  v1's six-places-per-type is the anti-pattern this kills.
- **Regenerate via component hooks/observers**, not a monolithic function: inserting
  `SceneLight` *causes* `PointLight` + collider construction via `on_add`/required
  components — one path shared by spawn, load, undo, prefab stamp, and inspector edit.
- **Versioned envelope** on every file the editor writes — scenes, prefabs, materials,
  keymaps, settings, asset `.meta`: `{ format_version, editor_version, payload }`. On-disk
  type names are stable short names decoupled from Rust paths.
- **Migration policy (in force from day one, not deferred to 1.0).** The editor is expected
  to evolve rapidly pre-stabilization; user content must survive that:
  - Migrations are **chained single-step functions** (`v3→v4`, `v4→v5`, …) composed
    automatically, so each feature PR that changes a format writes exactly one small
    migrator alongside it — schema change without a migrator fails CI.
  - **Two version axes**: the envelope's `format_version` for structural changes, plus
    per-component data versions for feature-crate payloads — feature crates evolve on their
    own schedules and register their own component migrators through `editor_api`, without
    bumping the global format.
  - **Compat window**: pre-1.0, any file written by any prior dev release must load in the
    current build (migrate-on-load, write current version on save). If a specific break is
    ever truly unavoidable, it requires an explicit `editor migrate` batch CLI shipped in
    the same release — silent orphaning of content is never acceptable.
  - **Migration test corpus in CI**: fixture files are frozen at every format bump and every
    release; the full corpus must load and round-trip green forever after.
  - **Loud failures**: unknown versions, unknown components, and parse errors surface as
    typed load errors/warnings in the UI (unknown component data is preserved and re-emitted
    on save, not dropped). v1 silently defaulted on parse failure — banned.
- **Stable identity**: every scene entity carries a serialized UUID. All entity references
  (followers, placer templates, prefab overrides) are UUIDs. Renames can't break anything.
- **Atomic, non-destructive I/O**: parse and validate into a staging world *before*
  despawning anything; temp-file + rename writes; one `.bak` retained. Loading a corrupt
  file must leave the current scene untouched — write the test.
- **Undo as transactional edit commands.** All mutations flow through an `EditQueue`; the
  queue captures inverse state — snapshotting cannot be forgotten because there is no other
  way to mutate scene entities. Per-entity deltas keyed by UUID (O(change), selection
  survives undo); scene-snapshot fallback only for global ops. Gestures coalesce (a drag or
  held-key repeat is one entry). Descriptions retained for a history UI. UI panels emit edit
  commands; **no UI system takes `&mut World` to poke components directly** (v1 had 36).
- **Dirty tracking is derived from registration**, never a hand-maintained query list.

---

## 6. Asset Pipeline & Prefabs

### Ingestion pipeline (`editor_assets`)
`Import → Validate → Process → Cook`, as data-driven stages that plugins extend:
- **Import**: watch folders / explicit import for GLTF, images, audio; each import gets a
  stable asset UUID and a `.meta` sidecar (import settings, content hash, pipeline version).
  Re-import preserves the UUID — this is what makes references survive.
- **Validate**: scale/units sanity, missing materials, triangle budgets, texture power-of-two
  — via the same validator registry games extend; failures surface in a problems panel.
- **Process**: per-type processors with cached, hash-keyed outputs — mesh optimization,
  texture compression (per-platform), collider generation (convex hull/decomposition/trimesh
  choice recorded in meta), LOD generation. Deterministic: same input + settings ⇒ same
  output; cache invalidation by content hash + processor version.
- **Cook**: game builds consume only processed output; a manifest maps UUID → cooked path.
  Editor builds can fall back to source assets with on-demand processing + hot reload.
- Evaluate Bevy's asset-processing (`AssetProcessor`/distill-style) infrastructure first and
  build on it where it fits; wrap rather than fork.


**Two of the four stages were dead (2026-08-20).** Import ran. Validate ran
against an EMPTY catalog, because `builtin_validators()` was written, tested,
and registered by nobody — so every import in the real binary reported "0
problems" whatever it was handed. Process was never called at all: a game could
register a processor and it would never execute. Both are now registered by
`editor_scene`'s models feature and run during import, and a test asserts the
catalogs are non-empty, because a stage with an empty registry fails silently
and looks exactly like a stage that passed.

- **Where processing happens.** The runner in `editor_assets` runs at import
  time, synchronously, on the bytes the stage already read. That is not a
  divergence from "game builds consume cooked output": the editor is the
  fall-back path §6 already describes, and Cook remains owed. The ledger's
  wrap-don't-fork decision still stands for cook-time packaging (ledger #10).
- **Cache location.** `<assets>/../.editor-cache/process`, deliberately a
  SIBLING of the asset root: a cache underneath it would be served by the asset
  server and re-scanned as source on the next import, and the pipeline would
  start eating its own output. Gitignored — it is derived, hash-keyed and
  regenerable, like every bake artifact.
- **Failure policy.** A processor that fails, or panics, is a PROBLEM; the asset
  still imports. Processors run at startup, so one panicking on one malformed
  file must not be able to make a project unopenable — a panic is caught and
  reported exactly like a returned error.
- **`extensions: &[]` means "every extension"** in Process, matching the
  validator registry exactly. The two registries sit in one pipeline, and a game
  registering an any-asset processor by the validator's precedent would
  otherwise get one that silently never runs — the very bug this wiring ends.

**Nothing is dropped in silence (2026-08-20).** The scan walks `models/` and
`textures/` RECURSIVELY: a purchased pack arrives as `models/dungeon/walls/*`,
and a flat scan found the directory, could make no asset of it, and said
nothing. A file no importer and no processor claims is now a PROBLEM naming its
extension, rather than an absence — this project's own
`assets/textures/*.tif` had been invisible with no way to find that out. A file
only a PROCESSOR claims (a `.tif` waiting on a converter) is pipeline input and
deliberately NOT a library entry: the editor cannot load one, and offering it in
the palette would place something that never appears.

**First processor: `gltf.bounds` (2026-08-20).** Model bounds and triangle
counts, read from POSITION accessor `min`/`max` at the JSON layer — the same
chunk the validators parse and the same bytes the cache is keyed on. Chosen over
a texture or mesh processor for a reason worth recording: the cache key hashes
ONE file, so any processor that reads external `.bin` buffers or images would be
silently wrong the moment a `.gltf` sibling changed. Bounds are hermetic under
that key — never wrong, at worst stale.

It answers a question the editor could not previously ask: how big is this
asset, BEFORE it has loaded. `piece_bounds`, socket generation and the socket
snap all gave up when a model had not finished loading — "select the wall you
just imported, generate sockets, get nothing" — and now fall back to the
recorded box. The live `Aabb` stays authoritative once loaded; this is the same
answer arriving earlier, not a second source of truth (§11).

Deliberate limits, documented not fixed: the content hash covers one file, so a
`.gltf` whose sibling `.bin` changes does not invalidate; there is no cache
eviction and no clear verb; processing is synchronous on the main thread, which
is why the first processor had to be a cheap one; and Cook — the manifest from
UUID to cooked path — remains owed, with `ProcessedAssets` recording exactly
what it will need.
### Prefabs (`editor_prefabs`) — the unit of game-ready
The pipeline's terminal product and the primary authoring workflow:
- A **prefab** = versioned asset: entity hierarchy of registered components + references (by
  UUID) to processed assets + default overrides exposed as named parameters. "Game-ready"
  includes **layout metadata** (sockets, kit tags, footprints — see the assisted layout
  system in §9): a prefab the automation tools can't place correctly isn't done.
- **Instances are references with override deltas** — `{ prefab_id, transform, overrides }`
  is what serializes into a level, never the expanded tree (v1 expanded copies; that defeats
  the point). Stamping happens at load through the same regenerate hooks. Per the BSN-first
  policy (§5): overrides are **BSN patches** (per-field), variants are **BSN scene
  inheritance**, nesting is BSN composition — the prefab system is an authoring/UX layer
  over BSN's substrate, not a parallel scene engine.
- **Override semantics**: per-instance component-field deltas tracked explicitly; UI shows
  overridden fields distinctly; verbs for *revert override* and *apply override to prefab*;
  source edits propagate to all non-overriding instances (open scenes update live).
- **Nesting and variants**: prefabs compose prefabs; variant chains inherit and override.
  Cycle detection required.
- **Prefab edit mode**: open a prefab in an isolated edit context (own undo scope), modal
  and keyboard-driven like everything else; save propagates.

**Editing a prefab is not editing an instance (2026-08-20, owner direction).**
Two verbs, deliberately different keys, because conflating them is what made v1's
prefab UX "not clear when editing a prefab or a scene":

- **`Enter` on an instance** opens it IN PLACE. You are editing THIS copy: the
  level stays around it, dimmed, and edits become overrides. Unchanged from the
  2026-08-02 decision that replaced v1's world swap.
- **`space e`** opens the PREFAB in a scene of its own, at its own origin, with
  the level parked. Changes here are the prefab, so every instance follows.

The level is CAPTURED and restored through the same snapshot machinery scene
save/load uses — it exists as one value the whole time, so coming back is
applying it rather than rebuilding it. That is the difference between this and
the world swap the owner rejected: the failure mode there was losing the level,
and a snapshot cannot half-restore.

While the template is open, **save, open, play and reset are refused out loud**.
Each would operate on the wrong world — saving a prefab over your level is
precisely the corruption a swap invites — and a refusal that says why is the
whole mitigation.

The status bar names which of the two you are in (`PREFAB ◆ BARREL` against
`EDITING ◆ BARREL` for an in-place instance), and `escape` is bound in the
`template` layer to come back. The layer is LAYERED rather than exclusive:
editing a prefab is ordinary editing, so move, rotate, the palette and undo all
keep their meaning; the layer exists only to give Escape a different job than
"clear the selection".
- **Baking: precompute heavy derived data into the prefab.** When load-time computation gets
  too heavy, a prefab can *bake* it at author/cook time and ship the result.
  **Invariant: bakes are caches, never source of truth.** A prefab's source data (entity
  hierarchy, components, asset references, parameters) must always be sufficient to
  re-derive every baked artifact from scratch — deleting all bake output and running
  `editor bake` must reproduce it bit-for-bit (given the same baker versions), and CI
  proves this on the fixture corpus. Bakers are therefore required to be deterministic
  (seeded randomness recorded as an input; the seed lives in source data, not the bake),
  and no editing operation may ever write *only* into baked output. Corollary: bake
  artifacts are safe to gitignore / exclude from VCS and regenerate on any machine:
  - Feature crates register **bake steps** through `editor_api` (`.bakers(...)`): convex
    decomposition / trimesh colliders, generated LOD chains, merged static meshes, spline
    road meshes, scatter placements resolved to concrete transforms, navmesh tiles for the
    prefab's geometry, particle warm-up states — anything currently rebuilt on every load.
  - Baked artifacts are stored alongside the prefab, **keyed by content hash of their inputs
    + baker version** — edit the source data and the bake is automatically stale; staleness
    is surfaced in the UI and problems panel, never silently served.
  - **The editor works live, the game loads baked.** In-editor editing uses the dynamic
    path (regenerate hooks) so iteration stays instant; the cook stage requires all bakes
    fresh (CI-enforceable), and game builds load baked artifacts with no derivation work.
    Per-bake-step policy on save: auto-rebake (cheap steps) or defer with a stale marker
    (expensive steps), with an explicit *bake now* verb and an `editor bake` batch CLI.
  - Bakes respect the override model: an instance override that invalidates a baked input
    (e.g. scaling a collider source) either falls back to the dynamic path for that instance
    or triggers a per-instance bake — a designed decision per baker, declared in its
    registration, never an accident.
- **Raw-to-ready flow** (the studio pipeline, end-to-end test this): drop `barrel.glb` in →
  auto-import + validation → create prefab from it (colliders, materials, gameplay
  components attached) → place 50 instances with overrides → artist re-exports `barrel.glb`
  → re-import flows through processing → prefab updates → all 50 instances update, overrides
  intact.

### Publish pipeline (raw project → playtestable package)

The pipeline's final stage: one verb that turns the project into something you can hand a
playtester. Available as `editor publish` (CLI, CI-friendly) and as an in-editor command
with progress UI.

- **Profiles** (data-driven, extensible): at minimum `playtest` (optimized build, debug
  overlays/cheats available, symbols kept) and `release` (stripped). Profiles select
  platform targets, cook settings (texture formats per platform), and feature flags.
- **What a publish run does**, failing loudly at the first gate:
  1. **Gate**: validation clean (or explicitly waived per-rule), all bakes fresh, no
     unsaved/dirty content, migrations current.
  2. **Cook**: full asset cook for the target platform (§ Cook) — game builds consume only
     cooked output.
  3. **Build**: compile the game binary *without* the `editor` feature (the §1 promise that
     release builds contain zero editor code is enforced here — CI asserts no editor
     symbols/egui in the artifact).
  4. **Package**: platform-appropriate bundle (macOS .app/dmg, Windows dir/zip/installer,
     Linux dir/AppImage) with cooked assets + manifest; layout is Steam-depot-friendly.
  5. **Stamp**: build metadata baked into the binary and a `build_info` file — git hash,
     editor/format versions, profile, timestamp — surfaced in-game (main menu corner) so
     playtest feedback is attributable to an exact build.
  6. **Smoke test**: headlessly boot the packaged build through `game_framework`'s lifecycle
     (Boot → MainMenu → load a designated smoke level) before declaring success.
- **Games extend it** through the same registry pattern: extra gates (e.g. "no placeholder
  assets tagged TODO"), extra artifacts (server build), post-steps (upload to Steam/itch or
  an internal share — pluggable, not built-in).
- `template_game` publishes in CI on every merge to main; the packaged artifact is retained
  so "grab the latest playtest build" is always one download away.

### Materials
- `MaterialDefinition` is a real Bevy `Asset` with an `AssetLoader` (one persistence
  location, hot reload, rename/delete via the asset system — v1 had split-brain storage).
  Keep Library-reference vs Inline-override semantics. Extension data is reflection-
  serialized, typed, and versioned — never RON-strings-inside-RON. Round-tripping must not
  lose texture references (v1 bug; write the test).

---

## 7. UI System (coherent by construction)

- **UI stack: `bevy_ui` + the official Bevy editor widgets** (`bevy_feathers` on the
  headless `bevy_ui_widgets` layer — the set built for the official Bevy Editor; ported
  to BSN and carrying first-class text input as of Bevy 0.19). Rationale: **one UI stack
  for game and editor** (the game UI kit and editor panels share foundations, and the
  in-editor UI authoring mode ultimately authors both), zero egui anywhere (kills v1's
  git-branch bevy_egui pin class of problem), and alignment with the official editor's
  trajectory — adopt its widgets as they mature rather than maintaining parallels.
  - *Risk containment*: feathers is young. The M0 spike validates it at editor scale
    (docking/tiling, virtual lists for big hierarchies, property grids, text editing);
    panels code against the `WidgetKit` trait (see `04-EDITOR-API.md` §9), so the
    backend is swappable — egui is the documented fallback behind the same seam if the
    spike fails. Gaps found in feathers are candidates for upstream contribution first,
    local widgets second.
- **Panel framework**: one `EditorPanel` trait (id, title, placement, keymap layer,
  `fn ui(&mut self, ctx: PanelCtx)`); a layout manager owns docking, pinning, focus, and
  keyboard panel-navigation — written once, on the feathers/bevy_ui foundation. Panels
  are registered by plugins; a panel cannot draw its own window chrome.
- **Reflection-first inspector**: one recursive reflection editor + a registry of
  type-override widgets (`TypeId → widget fn`). No per-component snapshot structs, ever.
  v1's `reflect_editor.rs` is the *design* seed — its Transform/Quat-as-Euler/Color
  overrides and immutability cache carry over as design; the egui code itself does not
  port (see quarantine note in §11).
- **Widget kit** (`editor_ui::widgets`): property rows (drag/color/vec3/checkbox), section
  headers, cards, empty-states — the blessed set, composed from feathers primitives;
  panels compose these, keeping look and keyboard behavior uniform. v1's `theme.rs`
  semantic-color system carries as a design (a feathers theme), not as code.
  **Standing visual bar (owner directive, M1 review): editor chrome is always modern,
  themed surfaces — full-width bars/panels with backgrounds and real layout, never bare
  text floating over the render — including interim milestones.** Corollaries, binding
  on every widget: (1) universal styling — all spacing/radius/color/type values come
  from one shared style scale; one-off inline values are a review-rejection; (2) padding
  on every surface, logical alignment, cohesive rounding, no unexplained gaps;
  (3) symbology over words — a nerd-font icon set ships with the editor and keys render
  as glyphs (⌃ ⇧ ⌥ ⌘ ␣ ⏎ ⎋); (4) every keypress gets feedback (unbound keys say so);
  (5) design decisions are made to a professional standard proactively — the owner is
  consulted for large-scale layout shape only.
  **Binding requirements from M0 spike findings F6–F8** (raw feathers controls are
  row-oriented and empty-state-hostile; the kit fixes each once, centrally): property
  rows encode label-above-controls layout with width caps internally; wrapped controls
  get column-safe flex defaults; text/number fields are never zero-size when empty,
  seed their display values, reset-to-default on empty blur, and focus on whole-box
  click; input frames style via background tokens, not borders (owner call); list
  widgets virtualize; field-level undo bridges to the `EditQueue` at the widget
  boundary (upstream text editing has none).
- **One palette engine**: port v1's `fuzzy_palette` trait design; every searchable list
  (commands, insert, find-object, components, assets, prefabs) is an instance of it —
  including the command palette itself (v1's flagship palette bypassed its own engine).
  Palette mode state is an enum with payload, not a field union. Matcher built once, results
  re-filtered only on query change.
  **Surface layout (owner decision, M1)**: search field on top; below it two panes —
  results list left, preview pane right. The preview shows visual previews for assets/
  prefabs/materials and documentation + metadata for non-visual items (components,
  actions). **v1 virtues are binding** (its palette was the best-designed part of v1's
  UI): trait-driven items (label, category, keywords, enabled, suffix), category
  grouping, pinned items requiring explicit navigation before Enter, typed `open_*`
  mode methods, and the polish details (eat leftover Enter on open-frame, etc. — see
  `01-REVIEW.md` keep-list).
- **UI reads state, emits edit commands** (§5). Change detection via `PartialEq` derives —
  serializing to strings for comparison is banned (v1 did RON×2 per frame).
- **Every panel keyboard-navigable**: focus model, j/k list navigation, field editing
  without the mouse. The status line always shows mode + available key hints.

---


**Model previews, and two chips that were never drawing (2026-08-20).** §7's
palette pane promises "visual previews for assets/prefabs/materials". Imported
models were the one placeable thing that reached the preview slot and left it
empty — import a kit and the palette is forty rows of `SM_Bld_Ruin_Wall_*` you
must already know by name. A model now previews from the SAME `MeshRef` the
scene uses, so the chip cannot drift from what placement produces.

Three things fell out of building it, all of them pre-existing:

- **The material and texture chips rendered nothing.** The texture sphere was
  never parented to the preview root, so it sat at the world origin 900 units
  from the preview camera; the material sphere was translated to preview home a
  second time under a root already there, putting it at −1800. Both panes had
  been blank. Nothing caught it because the only preview assertion in the suite
  reads the MATERIAL EDITOR's rig, which is a different camera — and because a
  check written as "a mesh exists on the preview layer" passes for geometry
  parked anywhere in the world. The probe now counts meshes on the layer AND in
  front of the camera, which is the thing anyone actually meant.
- **The preview must contain what a gltf brings with it.** Bevy's loader
  defaults to `load_cameras: true, load_lights: true`, and the camera it spawns
  is ACTIVE when no other active camera was found, pointed at the primary
  window. A Blender file saved with its default Sun and Camera would light — or
  take over — the real level, once per highlighted palette row. Preview
  containment walks down from the rig root: meshes join the preview layer and
  stop being pickable, lights are confined to the preview layer, and cameras are
  switched off.
- **Every subject starts from the same three-quarter pose.** The turntable never
  reset, so arrowing down a kit showed each piece at whatever yaw the previous
  one happened to reach. Forty walls that differ by a window or a broken corner
  can only be compared if they are shown alike — a flipbook, not forty unrelated
  pictures. It also makes the preview screenshot deterministic.

**The pane carries the numbers the picture cannot (2026-08-20).** A chip is fit
to frame, so a 0.4 m bolt and a 4 m wall render the same size — and "is this the
full wall or the half wall" is the question a kit poses. The pane prints the
asset path, the real size in metres and the triangle count, all from the bounds
the Process stage recorded at import (§6): `≥` and a dimmed tint when the record
is incomplete, an explicit "size unknown" when there is none. Never over the
render — the pane is ordinary UI, and the standing rule against floating text on
the viewport is untouched.

**The palette a newcomer can learn from (2026-08-20, owner testing).** The
owner, opening it fresh: "hard to learn from as a newb, make sure it has
sections and is super easy to understand." Three defects, each verified:

- **The row cap was applied BEFORE grouping**, and it was 50 — fewer than the
  editor's own action list. An alphabetical run of ~74 normal-mode actions was
  sliced at "Rotate Selection", so undo, save, every view, and all nine socket
  verbs were simply not on the first screen, with nothing saying so. Browsing
  could not teach what browsing could not show. The cut is now announced ("…
  N more · keep typing to narrow"), the cap is 200 — the list scrolls, and the
  cap is for the pathological case (a component palette offering hundreds), not
  the normal one.
- **Sections were filed by MODE, and only two modes exist**, so every
  normal-mode action landed in one "EDITOR" bucket and the header printed once.
  Actions now declare a `PaletteGroup` — PLACE, SOCKETS & KITS, SELECT & EDIT,
  PREFABS, MATERIALS, ANIMATION, VIEW & PANELS, SCENE & SESSION — defaulted from
  the id namespace so a feature that says nothing is still filed sensibly.
  Ordered by what a builder reaches for, never alphabetically.
- **Inside a section, order is REGISTRATION order.** `sort_by(label)` scattered
  each workflow across six letters (Chain… / Fill Run / Paint With Piece /
  Repeat Piece / Snap Socket… / Sockets:…), while the registration order in the
  feature is already the order its author teaches them in. Deleting one sort
  bought a curriculum.

Sections also survive a query now: they used to vanish the moment you typed,
which is exactly when the result set turns heterogeneous and the domain cue is
worth most.

**Settings froze the old default (2026-08-20).** Raising the cap changed nothing
at first, because settings are saved WHOLE: every default a user has ever run
with is written into their `editor-settings.ron` and frozen there. `load_user`
now runs a migrator, and the first entry retires the 50-row cap — nobody chose
50, the editor wrote it. A cap a user actually chose is left alone.
## 8. Engineering Standards (the studio bar)

The meta-lesson of v1: **every defect traced to an invariant enforced by convention.**
V2's rule: if a correctness property depends on "remembering to," redesign until forgetting
is a compile error or an impossibility. Concretely:

- **System ordering**: public `SystemSet`s (`Input → Tools → Edit → Sync → Render`),
  every system registered into one; no frame-delay hacks; run conditions over early returns.
- **Testing** (v1: 3 test files / 70k lines — unacceptable):
  - Unit tests for all math/geometry (drag math, solvers, splines).
  - Round-trip property tests: save→load→save byte-identical; undo→redo returns identical
    world state; prefab override apply/revert idempotent.
  - Migration corpus tests (§5); headless integration tests driving the editor via actions
    (the action layer makes the editor scriptable — use that for testing).
  - `template_game` compile + boot test in CI, editor feature on and off.
- **CI, tiered to fit a 1–2 dev budget** (guardrails that hurt get disabled — so make the
  fast lane fast):
  - *Per-PR fast lane*: fmt, clippy `-D warnings`, unit tests, architectural fitness
    greps, single platform (Linux), editor feature on.
  - *Pre-merge-to-main / nightly full matrix*: Windows + Linux (primary), macOS
    (dev-support only), feature-flag combinations, migration corpus, publish smoke boot,
    conformance suites.
  Dev environment must work on all three desktop platforms (v1's Nix flake was Linux-only
  — provide a platform-conditional flake *and* a plain rustup path).
- **Performance discipline**: no per-frame allocation in steady state; change-detection-
  gated systems; no O(world) scans per frame (v1 rebuilt the physics BVH every paused
  frame); budget: 1000-entity scene edits at 120fps on mid hardware.
- **Error handling**: no `unwrap`/`panic` on user data or registry lookups; every file
  operation surfaces typed errors to a problems/toast UI; logging is not user feedback.
- **Docs as deliverables**: architecture book (mdBook) covering the plugin contract, keymap
  design doc, "building a game on game_framework" guide, and a plugin-author tutorial.
  Docs are updated in the same PR as the code they describe.

### Bevy version policy
- **Start on the latest stable Bevy (0.19 as of August 2026) and stay current.** The
  workspace pins the exact minor version; upgrades happen as a dedicated PR at phase
  boundaries — full test suite green, zero feature work mixed in. Never build against a
  git branch of Bevy itself.
- Third-party dependencies are chosen only if they track stable Bevy promptly (within
  ~a month of a release). A dep that lags a Bevy cycle gets feature-gated or replaced —
  v1's git-branch pins (bevy_egui, forks) are the anti-pattern.

### Process guardrails (how this gets built without repeating v1)
v1 didn't fail because of Rust — it failed from unreviewed AI accretion. These rules govern
the *process*, CI-enforced wherever possible:

1. **Spec-first.** Every PR names the spec section it implements; behavior changes amend
   the spec in the same PR. "Not in the spec" means "doesn't exist" — new ideas go through
   a spec amendment before code. Nothing lands "while we're at it."
2. **Acceptance tests before implementation.** Each phase's exit criteria are turned into
   executable tests when the phase *starts*; the phase is done when they pass — not when
   the code looks done.
3. **Feature definition-of-done checklist** (PR-template enforced). A feature is mergeable
   only with: action-layer entry + keymap + palette registration; undo/redo/macro support
   via `EditQueue`; serialization registered with migrator; tests; docs updated. Miss any
   one and it's v1 again — half-integrated features that look finished.
4. **Architectural fitness tests in CI**: dependency-direction rules; banned-pattern greps
   (`ButtonInput<KeyCode>` outside the input resolver, `&mut World` in UI systems,
   serialize-to-compare change detection, absolute paths in manifests); release artifact
   contains no editor/egui symbols; feature-matrix builds; migration corpus; publish smoke
   boot.
5. **Adversarial review.** AI-generated changes get a separate adversarial pass (agent or
   human) explicitly briefed to refute correctness, hunt duplication against existing
   code, and verify the checklist. The author never reviews itself.
6. **Human playtest gate.** No editor-workflow feature merges until a human has used it
   hands-on in the editor. Ten minutes of use finds what review can't.
7. **Duplication budget: zero.** Before writing any widget/helper/system, search for the
   existing one. A PR introducing a second implementation of an existing concept must
   unify or be rejected — parallel-evolution copies were v1's most common defect.

---

## 9. Studio Systems (decisions from the gap review)

Settled with the project owner 2026-08-01; treat as requirements at the stated depth.

### Collaboration & content workflow (merge-first, 2–10 person team)
- **A level on disk is a directory, not a file**: spatially/logically partitioned cells and
  layers, one text file each, stably ordered (sorted UUIDs) so diffs are minimal and two
  designers editing different parts of a level touch different files. Git + LFS (binaries
  only) is the assumed VCS; no locking infrastructure.
- This is also the **streaming-ready commitment**: v2 ships discrete whole-level loading,
  but because cells are the on-disk unit, later streaming (async cell load, per-cell bake)
  is additive — no format migration. Design the level format and `LoadingLevel` service
  with that seam explicit.
- In-editor conflict surface: on load, structural conflicts (same entity edited twice)
  get a resolution UI; never silently last-write-wins.

### Data-driven gameplay authoring (the iteration-speed answer)
- Designers iterate through **hot-reloadable data, not code**: components, stat tables,
  curves, and event-condition-action rule assets are the gameplay authoring surface; Rust
  is for systems. Every such asset type round-trips through the pipeline, the inspector,
  and prefab overrides like any component.
- No scripting VM (keeps consoles/WASM simple). If the data model's expressiveness ceiling
  is hit, revisit post-1.0 — the decision point is recorded here deliberately.
- **Rust hot code reload in dev builds** (1.0-required per the DoD): systems-level
  iteration via hot-patching tooling (subsecond/dexterous-class), so even the Rust path
  avoids full rebuild-and-restart during prototyping. Data hot reload remains the designer
  path; code hot reload is the programmer path. Dev-build only — never in publish
  artifacts (also keeps the console constraint clean).


**Trigger volumes (2026-08-20).** The event leaf of "event-condition-action":
a named box that notices when a marked actor is inside it, and says so. The E
is data; the C and the A stay Rust — rule assets remain owed, and this clause
is not done because triggers exist.

It lives in `game_framework`, not the editor, so it fires in a release build.
That is the point as much as the feature is: it is a **partial payment on the
OWED runtime split below**, and the template for the rest — runtime half
game-side, authoring half (registration, gizmo, palette preset, toast) editor-
side, and the editor knowing the type only through reflection.

Decisions worth keeping:
- **Its size is its `Transform.scale`.** The volume is a unit cube. A separate
  `half_extents` field would be a second dial for one quantity, and the two
  disagree the first time anyone uses the scale gesture — which is this
  editor's primary blockout verb. This deliberately diverges from
  `BoxCollider`, whose extents are pre-scale because it hugs imported geometry
  it does not own; a volume owns its own shape.
- **An actor is a point at its entity origin**, so the reference game puts the
  marker on a child at the player's FEET rather than on the camera at the eye.
  Coverage is spelled with several child markers, not a second size.
- **`once` means one entry, the exit that closes it, then silence** — so every
  `TriggerEntered` has exactly one `TriggerExited` and a reader can keep
  occupancy without knowing what `once` means.
- **Occupancy is evaluated after transform propagation.** An entity spawned
  this frame reads as standing at the world ORIGIN until it propagates, so an
  `Update`-scheduled trigger fires a phantom entry on the frame the level
  loads. Running in `PostUpdate` also orders it unambiguously against the
  editor's play/pause handoff.
- **Nothing fires unless gameplay is stepping**: the game owns the world,
  virtual time is not paused, and we are in a level. `GameplayActive` is the
  narrow half of that (who owns the world) and moved into `game_framework`
  with this slice, because the crate that owns the lifecycle should own the
  play/pause seam. It is the interim stand-in for §3's `Session` sub-states,
  not a parallel fiction.
- **A name is a cue, not an identity.** Duplicates all fire it, `once` is per
  volume rather than per name, and the viewport colours a volume by hashing its
  cue so two volumes that fire different things look different.

Deliberate limits, documented rather than fixed: point-not-bounds occupancy;
once-a-frame sampling, so a volume thinner than one frame of motion can be
missed (the level validator warns below 0.5 m, and errors on a volume flattened
to nothing, which contains nothing rather than trusting a singular matrix);
`TriggerExited` has unit coverage but no game-side consumer yet.

**Gizmos while playing (2026-08-20, §7).** Feature gizmos vanished when the
editor handed the world to the game, which is right for furniture and wrong for
a widget that IS the object: a trigger volume was invisible exactly while you
were testing whether you could walk into it. `space t v` keeps them on screen
through a play session, the way collider wireframes already work.

**Pick proxies were never pickable (2026-08-20, §7).** The invisible click
target that makes a gizmo-only widget selectable was spawned
`Visibility::Hidden`, and bevy's mesh picking ray-casts `VisibleInView` by
default — so it had never once caught a click. A mesh with no material is drawn
by nothing, so the proxy is simply visible now. `PickProxy` also replaced the
bare `pick_radius`: a fixed sphere is right for a spawn point, and wrong for
anything whose size is authored, where the proxy is a unit box parented to the
widget so the click target IS the box at every size.

**Why sockets did not work (2026-08-20, owner testing).** The owner reported
"I cannot seem to snap objects to sockets" and "we can hardly use the sockets".
Four separate defects, each sufficient on its own:

- **Reach was measured from the moved piece's ORIGIN to the target socket.** A
  six-metre wall carries its sockets three metres out, so its origin only comes
  within a two-metre reach once the wall is buried in its neighbour — mating was
  arithmetically impossible for exactly the pieces kits are made of, and looked
  like a broken editor rather than a wrong number. Reach is now measured SOCKET
  TO SOCKET, which means the same thing at every scale, and is a setting
  (`viewport.socket_reach`, 1.5 m).
- **`template_sockets` read top-level records only.** Sockets authored the
  editor's own way — generate on a piece, then group it — are CHILD records, so
  a prefab built entirely inside this editor reported zero sockets and could
  never be mated to. It now walks the whole template, composing ancestor
  transforms, which is also what makes the frames genuinely root-relative.
- **Mating required a `PrefabInstance`** and returned in silence otherwise, so a
  socketed imported model — the normal M4 asset — could never snap. Socket
  GENERATION never had that requirement, so the editor cheerfully let you author
  sockets onto a piece that could not use them. Anything owning sockets mates now.
- **The exclusion of a piece's own sockets was implemented by REMOVING the
  `Socket` component from each of them and putting it back.** That re-fired the
  add-observer, and the observer spawns a child cone, so the viewport
  accumulated stacked gizmos. Own sockets are filtered in the query instead, the
  observer is idempotent, and a removed socket takes its cone with it.

And the refusals speak: "no compatible socket within 1.5m" instead of nothing.

The mate math is now one pure function (`sockets::best_mate`) that placement and
drag-commit both call, so the preview cannot promise a snap the commit declines
to make — the regression D9 exists for.

**The socket as a handle (2026-08-20, owner testing).** Three verbs, one idea:
the socket you are holding is the thing the editor should act on.

- **Pivot on the JOINT.** Rotating a piece that is attached should swing it
  about where it is attached — the joint stays put and the far end sweeps, which
  is how a corner is made and a curve walked round. Selecting a SOCKET names the
  point explicitly; selecting the PIECE finds its joint for you, which is the
  common case and needed no words. Connection is derived from geometry
  (coincident, opposed, same type) rather than recorded, because mating writes a
  transform and nothing else — so a joint a designer made by hand counts too,
  and `mate_transform`'s own output is tested to read back as a joint.
- **Spawn the next piece AT the selected socket.** Placement asks one shared
  question (`sockets::placement_for`): a selected socket wins over the cursor,
  then the best mate near the cursor, then the cursor itself. Pick the end of a
  run, pick a piece, and it arrives mated — no hunting for a hover position that
  happens to be within reach of the socket you meant. A piece with no sockets
  still lands AT the socket, because putting it where you pointed beats ignoring
  what you said.
- **`i` while holding a socket HANDLE offers pieces, not components.** The
  add-component grammar was blocking the ask outright: every attempt to place
  from a socket opened the component palette. A handle is a socket entity hanging
  off a piece; a PIECE that merely carries a `Socket` component is still a piece,
  and `i` on it still adds a component.

**Painted segments are capped (2026-08-20).** A click near the horizon projects
hundreds of metres onto the ground, and a stroke laid a piece every two of them:
411 walls from one click, in one transaction, with the editor stalled while it
happened. Capped at 128 per segment, and the cap SAYS so — a stroke that quietly
lays half of what was asked is worse than one that explains itself. The probe
had only ever asserted "more walls than before", which 411 satisfies.

**The two things that made the socket verbs unreachable (2026-08-20).**

- **A socket on a prefab instance could not be clicked.** A prefab selects as a
  unit — that is the point of the seal, so you cannot author on a member of
  something you have not stepped into — and a click on a socket's cone resolved
  to the instance root. So the objects whose sockets you COULD click were models
  and primitives, which (until this session) could not mate; and the objects
  that could mate hid their sockets behind an Open step. Sockets now carry
  `SelectionHandle`: a handle is clicked as itself, seal or no seal, because a
  socket is not part of the shape — it is the authoring handle ON the shape.
  The rule is one function with one test, called by both the picking observer
  and the world-side helper, since two copies of a selection rule is how a click
  starts meaning different things in different places.
- **Generated sockets stamped `"default"` while kits use their own type.** Type
  is the compatibility rule and it is invisible in the viewport: two sockets of
  different types are two identical cones that will never mate and never say so.
  Generation now inherits — a type the piece already has, else one from a piece
  in the same kit, else `"default"` — and the toast names it (`generated 2
  sockets · type wall`), because the moment of creation is the one moment the
  type is knowable without opening the inspector.

**Socket mode (2026-08-20, owner direction).** The owner's words: "let's make
`o` a mode — Tab or clicking a socket selects, then `i` inserts a new object on
that socket", and when asked whether an exclusive keyboard layer would break
`i`: "it won't break `i`, it needs to be a modal editor, so `i`'s context just
changes."

So `o` no longer places a piece; it puts you WHERE placing happens. It arms a
socket (the first free one on the selected piece) and raises the `socket`
keymap layer, in which:

| key | means |
|-----|-------|
| `tab` | the next socket round the piece — this is how you say WHICH socket the next piece uses |
| `i` | place a piece here (the palette, mating what you pick to the armed socket) |
| `o` | chain another of the same piece |
| `esc` | done |

The layer is exclusive, which is the modal answer rather than a problem: a mode
rebinds the keys that mean something else in it. What a mode must never be is
INVISIBLE, so the status bar now names the active keyboard layer the way it
names a gesture or an open prefab.


**Socket mode, refined by use (2026-08-20).** Two changes from watching it
being driven:

- **The first FREE socket is armed automatically**, and Tab toggles from there.
  An end with nothing on it is what you build from; Tabbing past the joints you
  already made to reach one is the friction the verb exists to remove. Occupied
  sockets stay in the ring, at the back, so a deliberate re-mate is reachable.
- **`i` places immediately** — no palette, no second key. It repeats the piece
  the armed socket belongs to, which is what "the next one" means while running
  a wall out, and then arms the NEW piece's free end so the chain walks forward
  on its own: Tab to pick the end, then `i`, `i`, `i`. Choosing a different
  piece is what leaving the mode is for.
**Focus follows what you are looking at (2026-08-20).** The inspector restores
field focus across its own rebuilds, so keyboard navigation survives a rebuild —
correct, and it kept doing so across a SELECTION CHANGE. A dead text field
stayed focused forever, `KeyCapture` follows focus, and every key after that
went to a text box nobody could see instead of to the resolver. Restoration is
now conditional on the inspector still showing the same object.

**Locking, and edits that mean the selection (2026-08-21, owner direction).**
Three asks, one shape: "I want to be able to lock objects to prevent further
editing", "batch lock / batch edit components", "and batch insert components".

- **`Locked` is enforced at `apply_edits`, and nowhere else.** There is no side
  door to the scene (§8), so a guard on the queue covers move, rotate, scale,
  delete, reparent, patch, socket mating, drop, painting — and every verb
  written after this one, with no per-verb check to forget. Refusal is PER OP:
  a transaction touching a locked object and an unlocked one moves the unlocked
  one, because a lock that cancelled the batch would make locking a floor mean
  "you can never box-select again". A transaction refused in full records no
  history entry — a phantom step would make the next undo unwind the edit
  before it. Removing `Locked` is the one edit a locked object accepts;
  refusing it would make the lock a trap rather than a tool.
- **It persists with the level.** `Locked` is a registered editor component, so
  a floor you locked is still locked when the file is reopened, which is the
  point of locking a floor.
- **It is visible in both places you look.** The hierarchy row carries a
  padlock, and a locked selection silhouettes in the warn tone
  (`viewport.locked_outline_color`) instead of the selection blue — "selected"
  and "selected but frozen" are different states and must not look the same.
  The refusal also SPEAKS ("2 locked objects · ␣l to unlock"): a verb that
  silently does nothing reads as a broken editor.
- **`Space l` locks the whole selection in one transaction**, and a mixed selection
  LOCKS rather than inverting each object — with some locked and some not, the
  intent of pressing lock is to end up with everything locked. It is on the
  LEADER, not bare `l`: the keymap design reserves `h j k l` for selection
  motion, and a rapid-layout verb is not worth spending a motion key on.
- **An inspector field edit means the SELECTION.** The panel shows the first
  selected object, but with several selected, "set roughness to 0.4" honestly
  means all of them; the alternative is selecting ten crates and editing one,
  which is the tedium selection exists to remove. Each target is recomputed
  from its OWN current value — sending the shown object's finished component to
  everyone would batch every other field with it, so nudging one crate's
  rotation would teleport nine crates onto it. Renames never batch: names are
  identities, and ten objects called "Barrel" is silently destructive. Editing
  something that is not selected (a pinned row) means exactly that one thing.
- **Batch insert/remove of components already worked**, one transaction across
  the selection, and stays as it was.

**The feedback channel moved to `editor_api` (2026-08-21).** `SceneIoFeedback`
lived in `editor_scene` because save/load was the first thing with something to
say. The kernel now refuses edits, and a crate cannot speak through a channel
defined above it. It is defined in `editor_api::feedback`, registered by
`EditorCorePlugin`, and re-exported from `editor_scene` under the name the
whole editor already uses.


**Hide, isolate, and `*` (2026-08-21).** The two verbs that make a big level
workable, and they are deliberately opposite in kind.

- **Hidden is a VIEW on the level, never part of it.** `Locked` is a serialized
  component enforced inside `apply_edits`, because it changes what the scene
  allows. Hidden changes what you are looking at: it never enters a
  `Transaction`, never touches `History`, never marks the scene dirty, and never
  reaches `level.ron`. `u` will not bring an object back, which is why every
  message names `␣u` and why the statusbar carries a persistent count — a flash
  is gone by the time you come back from lunch.
- **It lifts whenever the editor is inactive.** F5 shows the real level. Nobody
  should playtest against a level that is secretly missing its floor, and a game
  animating visibility during play must not have a second writer.
- **It is keyed by `SceneId`, not `Entity`.** Play/reset despawns and respawns
  every scene entity, so an entity-keyed set would evaporate on F7.
- **It drives the whole SUBTREE rather than setting a root and trusting
  inheritance.** An imported model's content is a spawned asset subtree that
  bevy's visibility propagation treats as its own root: hiding a prefab instance
  set the instance hidden and left its meshes lit, which is precisely "the
  editor says it worked and the viewport disagrees". Immediate-mode gizmos are
  invisible to propagation by construction and carry their own filter, so
  hiding a trigger volume does not leave a wire box you can see and cannot
  click.
- **`Visibility`, `InheritedVisibility` and `ViewVisibility` are EDITOR-OWNED**:
  denied from the save set by `EditorComponents::adopt` (the door a source-level
  check cannot watch, since `apply_scene` adopts whatever a file names), banned
  from registration by a fitness test over every workspace crate, and removed
  from the inspector. That last one closed a live bug: the `Visibility` row was
  editable, fanned across the selection, spent an undo step and dirtied the
  scene to set a value the file never kept.
- **Isolate EXITS by restoring, not by revealing.** Hides made before isolating
  survive it; unhide-all drops the restore set, or leaving isolate later would
  resurrect exactly what the user asked to see.
- **Hide deselects, and hidden objects are out of every selection path** — box
  select and `ctrl+a` both skip them. `space h` then `ctrl+a` then `d` deleting
  invisible geometry is the silent destruction the lock work exists to prevent.

**`*` is a registered ladder, not a hardcoded list.** The kernel cannot name a
prefab, a model or a game type — `editor_core` depends on `bevy`, `editor_api`,
`serde` and `ron`, and nothing else — so "what makes two objects the same thing"
is declared as data by the crate that owns the component
(`editor_api::identity`), and the kernel compares reflected values. A rung names
the whole component, one field, or mere presence: `Primitive` matches on `kind`,
because two cubes of different sizes are both cubes; a trigger volume matches on
presence, because one named "lift" and one named "pit" are still the same kind
of thing. First rung present wins, so a barrel that is both a prefab instance
and a mesh is a barrel. A bad rung is a startup panic — a key that stops
resolving is a `*` that stops working, and silence is how that ships.

`*` replaces the selection and unions the sources, which makes repeated presses
additive for free; it stops at the prefab seal, refuses on sockets (a handle
clicks as itself, so every socket in the file would be one family), skips
hidden, and says what it matched and what it left out.

Physically the binding is `shift+8`: `KeyCode` is a physical key, so there is no
`*` token to parse, exactly as `:` is `shift+semicolon`. The chrome renders both
back as `*` and `:` — a narrow, deliberate alias table, because a blanket
US-layout mapping would render the ortho-view bindings `shift+1/2/3` as `!@#`.

Hide, isolate and unhide-all sit on the LEADER (`␣h`, `␣⇧h`, `␣u`) for the same
reason lock does: the keymap design reserves `h j k l` for selection motion.


**Array and mirror (2026-08-21).** The two blockout multipliers, and both are
BAKES rather than modifiers: the copies are ordinary entities in one
transaction, individually editable afterwards, and one undo removes the run. A
live re-derivable array wants an authoring component and a preview that
provably spawns nothing; this is the version that can be right today, and the
parametric form stays deferred with a named follow-on.

- **The step is the piece's own extent, never the grid.** That is the whole
  value: a wall arrays flush against itself and the run comes out as a wall
  rather than a dotted line. Quantizing a 0.98 m piece to a 1 m grid seams every
  joint, and the grid is exactly what a designer is trying not to think about.
  The measurement skips socket subtrees — a socket's gizmo is a real mesh cone,
  and counting it would pad every kit piece by the width of something invisible
  in the game. Objects with no geometry at all (a light, a spawn point) fall
  back to the grid step, and the message says which it used.
- **Array refuses what it cannot copy losslessly.** `Op::Spawn` carries
  components and no parentage, so a copy is ONE entity — lossless only where
  everything below the root regenerates (a prefab instance's members are
  re-stamped, an import's gltf subtree carries no `SceneId` at all). A flattened
  model, a hand-built group or a loose socketed piece has real child content a
  single-entity copy would silently drop, so array refuses those out loud and
  points at `g`. Duplicate has the same hole today and does not refuse; making
  the copy hierarchy-aware needs one inverse op per spawned entity, which the
  edit engine cannot express yet. That is the deferred work, and array's gate is
  what makes shipping this first safe.
- **Mirror reflects PLACEMENT and conjugates ORIENTATION.** A plane reflection
  is improper — determinant −1 — and no `Transform` holds one without a negative
  scale, which flips winding, breaks lighting and confuses physics. So the
  position is a Householder reflection (exact) and the rotation is `R·M·R`,
  which is proper for every M and is the exact answer whenever the piece is
  symmetric about the plane's direction — every wall, floor, pillar and crate in
  a blockout. It does NOT flip chirality: an L-corner comes out rotated. Bounds
  cannot detect that (an L has symmetric bounds), so rather than guess, the
  feedback says "placement only, geometry is not flipped" every single time. The
  mirror-partner variant that would fix it properly is prefab kit metadata.
- **The plane rides the selection.** Its origin is the subjects' centroid, so
  mirroring a pair swaps them and mirroring one leaves it put. An arbitrary
  plane wants a pivot concept the editor does not have yet.
- **Both verbs skip locked and hidden roots and COUNT what they skipped.**
  `Op::Spawn` is not an edit to anything, so the queue's lock would never stop
  an array; and a layout verb must not be the one thing that materialises what
  you took out of the view. `editor_core::layout` holds that rule once, because
  two copies of "what does this selection name" is how `space h` and `*` start
  disagreeing.

### Rapid-prototyping toolkit (1.0-required, from the DoD)
The DoD's "idea → playable trial in minutes" demands these as first-class feature crates:
- **Assisted layout system** — the level-layout answer (in-editor mesh modeling explicitly
  is not; see Non-goals). Design principle: **the designer authors intent at the big-picture
  level; the system owns individual placements.** Nobody should ever hand-place and
  hand-rotate forty wall segments.
  - *Five placement paradigms*, one system. Each is a placement-solver family; a prefab/kit
    declares which paradigms it supports (via its layout metadata), tools compose them
    freely within one level, and every paradigm emits ordinary entities through the
    `EditQueue`:
    1. **Grid** — classic cell-based placement for tile-type assets: per-kit cell size,
       vertical/floor levels, rotation steps, auto-tiling rules (edge-matching picks the
       right tile variant). The fastest paradigm for dungeon/interior blockout.
    2. **True-shape snap** — socket/anchor and geometry-aware snapping for non-grid pieces
       ("custom-shaped lego"): typed sockets mate piece-to-piece with correct orientation;
       face/edge contact snapping for pieces without authored sockets.
    3. **Landscape** — a terrain substrate plus terrain-aware placement: heightmap terrain
       (sculpt + texture-paint, its own feature crate) as the conforming surface; placement
       snaps to ground with slope/altitude rules; splines project onto terrain (roads that
       follow the land — the spline crate's surface projection carries over); scatter
       respects slope/altitude/texture masks.
    4. **Procedural/generative** — seeded generators that *drive the other paradigms*:
       scatter in area/volume, distribution along splines, room/corridor generators
       emitting grid or true-shape placements. Parameters + seed persist as authoring
       data, so results re-derive; a generated result can be "detached" to hand-editable
       entities at any time.
    5. **Freeform** — classic transform tools with the general snap solvers
       (surface/center/aligned/vertex, edge snapping, grid/angle toggles). Always
       available; the fallback, never the primary prototyping path.
  - *Foundation — modular snap kits* (serve paradigms 1–2): kit pieces (walls, floors,
    corners, doors, props) declare sockets/anchors and snap piece-to-piece with correct
    orientation; grid and angular snapping; repeat-last-piece flow. Parametric blockout
    shapes (stairs/ramp/arch/wall with live-editable dimensions) are kit citizens like
    imported pieces.
  - *Architectural painting*: draw a wall as a path — segments, corners, and junctions
    auto-place and auto-join from the active kit; paint a floor as an area — auto-tiled;
    punch doors/windows into walls — the affected segment swaps for the doorframe piece
    automatically. Edit the high-level primitive (move the path, resize the area) and the
    pieces re-derive.
  - *Automation verbs*, all operating on the same high-level primitives: **scatter in
    area/volume** (density, seed, filters — the scatter crate's engine); **place along
    spline** (spacing, jitter, ground snapping via surface projection); **extrude along
    spline** (fences, pipes, roads — the spline crate's generation carries over); grid
    fill; array/repeat.
  - *Everything stays honest*: assists are authoring tools that emit ordinary entities /
    prefab instances through the `EditQueue` — output is individually editable afterward,
    undoable, macro-recordable, and serialized like anything hand-placed. High-level
    primitives (paths, areas) persist as editable authoring components so re-derivation
    stays available, per the same source-vs-derived rule as prefab baking.
  - *Layout metadata is prefab data, authored on the prefab.* Everything the assists need
    to place a piece correctly lives as ordinary registered components on the prefab:
    sockets/anchors (position, orientation, socket type), kit membership + role tags
    (straight wall / corner / door-frame / floor tile), tiling cell size, scatter footprint
    and ground-snap pivot, allowed rotations. Authored in **prefab edit mode with visual
    gizmos** — see sockets in the viewport, drag/snap them, type-match them — not by
    hand-editing numbers. The import pipeline **proposes** metadata automatically where it
    can (bounds-derived footprints, sockets from DCC naming conventions like `socket_*`
    empties); the designer refines. Because it's ordinary component data, it serializes,
    versions, migrates, and per-instance-overrides like everything else.
  - *Kits are assets too*: a kit definition groups prefabs sharing socket conventions;
    the validator registry checks kit coherence (mismatched socket types, missing
    counterpart pieces, incompatible cell sizes) so a broken kit is a problems-panel
    entry, not a mystery mis-snap.
  - *Keyboard grammar*: painting and automation verbs are modal tools under the standard
    keymap/action system — discoverable in the palette and which-key like every feature.
- **Terrain (`bevy_terrain`) — basic now, state-of-the-art prepared for.** Two explicit
  tiers; the 1.0 tier is deliberately modest, but its data model is shaped so the advanced
  tier is a renderer upgrade, not a format migration.
  - **1.0 tier — basic terrain + proc gen:**
    - *Data model (this is the preparation — get it right now)*: heightmap + splat data
      stored as **tiles on disk**, aligned with the level cell format (§9 collaboration);
      per-chunk mesh generation; per-chunk collider/navmesh/scatter re-derivation as bake
      steps reacting to terrain-change events.
    - *Proc gen first*: seeded noise-stack heightmap generation (layered
      noise/falloff/masks) as a generator under the procedural paradigm — parameters +
      seed persist and re-derive like all generative content. Hand-sculpting starts
      simple: basic raise/lower/smooth/flatten + splat paint brushes, edits landing as
      heightmap **tile deltas through the `EditQueue`** (undoable, macro-recordable,
      mergeable; one stroke = one undo entry).
    - *Rendering*: straightforward chunked mesh LOD (distance-based, crack-free
      stitching), splat-map material layering. Nothing clever; correct and shippable.
  - **Post-1.0 tier — the state-of-the-art upgrade (designed-for, not built):**
    view-centered geometry clipmaps or quadtree CDLOD; compute-driven adaptive
    subdivision (CBT/LEB-class — wgpu/WebGPU has no hardware tessellation stage, so
    compute is the modern path regardless); GPU-driven culling/LOD/indirect draws with
    baked min/max height pyramids; GPU compute brushes; clipmapped/virtual texturing.
    The 1.0 tile format, chunk streaming seam, and bake steps must be reviewed against
    these techniques *before* freezing (a design-review checklist item, not a spike) so
    none of them forecloses the upgrade.
  - *Portability at both tiers*: everything must run on WebGPU (WASM target); any
    desktop-only fast path documents an automatic fallback.
- **Screen/post-process effects**: a data-driven post-effects stack (bloom, vignette,
  color grading, screen shake, hit flashes) authorable in-editor, hot-reloadable.
- **Shader authoring**: in-editor shader editing (WGSL with live hot reload + error
  surfacing at minimum; a node-based front-end can come later), producing material assets
  that flow through the normal material/pipeline path.
- **Animation sequences**: the minimal timeline asset from the DoD (see the animation
  section below — this is the "orchestrator" layer).
- Particle effects (already covered by the VFX feature crate).

### Game-side services in `game_framework`
- **Game input**: the game gets the same action-layer treatment as the editor — named
  actions, data-driven bindings, controller support with per-platform glyphs, and a
  player-facing rebinding UI shipped in the UI kit. One input philosophy, two consumers.
- **Save games**: versioned save-slot service (same envelope + migrator discipline as
  editor formats), platform-abstracted storage backend.
- **Localization**: all user-facing text in framework/UI-kit goes through locale keys +
  Fluent-style assets *from day one* (retrofitting keys is the expensive part). Translation
  management tooling deferred.
- **Platform abstraction layer**: storage, achievements, input glyphs, and platform init as
  traits. **Priority order: Windows and Linux (+ Steam Deck) are the primary shipping and
  dev targets; macOS is supported for development only** (dev shells and dev builds work
  there, but it is not a publish-priority target). WASM and mobile are publish targets
  with their constraints (threading, asset budgets, texture formats) declared in publish
  profiles; console-shaped abstractions (no dynamic codegen, strict budgets) respected so
  an eventual console port is a backend, not a rewrite. The editor itself has no platform
  list — it is a game feature (§1) and exists wherever a game build enables it.
- **Networking — `lightyear`, decided** (proven in the owner's prior projects; tracks
  stable Bevy). `game_framework`'s session flow (Connect/Disconnect/Reconnect,
  RoundStart/End) is built on lightyear's real semantics, with replication registration
  riding the same component-registration seam. **Listen-server (one player hosts) is the
  required 1.0 topology** per the Definition of Done; dedicated server is a publish-profile
  artifact. `template_game` demonstrates a networked round loop. Editor tooling (network
  state inspector/debugger panel) is a later feature crate, but the framework's session
  states must be lightyear's states, not a parallel fiction. **Scope guard: networked
  play-in-editor is explicitly out of 1.0** — editing a live world under
  prediction/rollback is research-grade; the DoD needs local play-in-editor plus
  *published* listen-server playtests, and M6 must not wire the play button into a live
  lightyear session.
- **Audio**: Rust-native backend (`kira`) wrapped in a **data-driven audio-event layer** —
  events, buses, ducking, parameters as hot-reloadable assets authored in-editor; game code
  fires events by ID. Backend sits behind a service trait so middleware (FMOD/Wwise) can
  become an alternative backend crate if a project ever requires it.

### Game UI kit (feature crate, post-core phases)
- In-game UI (HUD, menus, settings) is **bevy_ui**, authored in the editor: a UI-document
  asset type, a layout-editing mode, and widget prefabs (reusing the prefab override
  system as widget templating). `game_framework`'s menu/settings/loading screens ship as
  themed UI-kit content games can restyle — the framework's opinionated flows and the UI
  kit are the same feature seen from both sides. Per the BSN-first policy (§5): a UI
  document **is a BSN scene** (widget prefabs = BSN templates, restyling = BSN patches) —
  the same substrate upstream feathers itself builds on, so editor-authored UI and
  upstream widgets stay one system.

### Playtest operations
- **Telemetry in `playtest` profile builds**: crash capture (panic + minidump where
  available) and structured session events, stamped with the publish build info, written
  locally and optionally uploaded to a configurable endpoint — no third-party lock-in.
- **Input-replay, best effort**: playtest builds record the action/input stream + seeds;
  a crash report carries the recent stream so bugs can be replayed in a dev build.
  Bit-exact determinism is a non-goal (Avian float variance tolerated); the replay is a
  repro aid and QA smoke tool (soak tests = replay long sessions headless), not a netcode
  or e-sports guarantee.

### Asset database at scale
- The asset DB (UUID-indexed) maintains a **dependency graph** both directions: "what does
  this prefab use" and "what uses this texture." Safe rename/move is a metadata operation
  (UUIDs don't move); **delete requires a reference check** with a redirector left behind
  for stragglers. Tagging, collections, and the fuzzy palette over all of it.
- **Shared derived-data cache**: cook/bake outputs are content-addressed (already required
  by §6); add an optional shared backend — a network directory or S3-compatible bucket —
  that teammates and CI read/write, so one machine's cook warms everyone's cache. No
  dedicated cache server at this team size.

### Project/editor version management
- A project pins its editor (and format) versions in project metadata; opening a project
  with a newer editor triggers the explicit upgrade flow (migrations + report), never a
  silent rewrite. CI can assert "project opens clean under pinned version."

### Animation — two-system architecture (decided 2026-08-01)
The industry-standard split, adopted deliberately: an **orchestrator** for authored time
and a **per-rig graph** for gameplay-driven time. One-directional command between them —
gameplay owns a rig's graph during play; a sequence may temporarily claim a rig via a
**cinematic slot** and hand it back with a blend. Two systems never fight over bones.

**Layer 1 — Sequencer (orchestrator).** A timeline asset (part of the rapid-prototyping
toolkit, no skeleton required) whose tracks can: keyframe **any reflected component
property on any entity** (reusing the inspector's reflection machinery), fire **events**
at timestamps (VFX, audio, gameplay triggers), and **command rigs to play clips** through
the cinematic slot. Hot-reloadable, scrubbing-friendly, playable from gameplay rules and
the effects layer. Full cinematic camera-cut suites remain a non-goal.

**Easing (2026-08-20).** Linear motion reads as machinery: constant speed,
instant starts, instant stops. A key carries how the segment LEAVES it —
linear, in, out, in-out, or hold — which is most of the difference between
something that moves and something that looks animated, for the price of one
enum on a key. `Space T C` cycles the keys sitting at the playhead, together,
because the three axes of one pose are one decision rather than three.

The ease belongs to the key the segment leaves, so "this key eases out into the
next" is a property of the key you are looking at when you decide. `Hold` does
not interpolate at all — the value stays and then jumps, which is what a switch
or a visibility flag needs. Every ease hits both its keys exactly: a curve that
misses its own endpoints is a bug with a nice name.

Defaulted in serde, so every timeline written before easing existed loads and
behaves exactly as it did.

**Events (2026-08-20).** The sequencer's second job in §9: fire events at
timestamps. `Space T E` marks the playhead with a NAME, asked for through the
one name prompt, because an event's whole content is its name — it is what the
game matches on. Games read `TimelineEvent` like any other message; the timeline
says when and what, and never what it means.

Events fire on CROSSING, half-open on the left, so a marker fires once as it is
passed rather than once per frame while the playhead rests on it. A loop is two
spans — off the end and back from the start — and a marker at zero is reachable
on the wrap. They fire during PLAYBACK only: scrubbing through a footstep should
not play forty footsteps, and dragging backwards should not play them in
reverse. While paused, the fired-through mark follows the playhead so resuming
does not replay everything that was skipped.

**Events are visible, and the game answers them (2026-08-20).** Markers get
their own row above the tracks — a bar rather than a diamond, so an event is
never mistaken for a key — labelled in place, because an event nobody can
identify is a tick mark. The reference game reacts by matching on the NAME and
nothing else: the timeline says `"spin"` happened and knows no more; the game
turns that into behaviour it already owns. No editor type crosses into gameplay
logic, and the probe checks the reaction the same way — by reflection, knowing
only a component name — because the editor must not depend on the game.

**OWED: the timeline ships with the editor, not with the game.** (Partially
paid 2026-08-20: trigger volumes landed runtime-side in `game_framework` as the
template for this split — see the trigger clause above. The timeline itself has
not moved.) `editor_scene`
is an optional dependency of the reference game, so a release build has no
sequencer, no tracks and no events — an authored animation cannot play in the
shipped artifact. §9 requires the opposite ("playable from gameplay rules"). The
runtime half (sampling, evaluation, crossing, the data) belongs in
`game_framework`; the authoring half (keying, the panel, the prompt) stays here.
This is NOT unique to animation: scene loading is editor-side too, so the split
is one piece of work for the whole runtime rather than something to do for the
timeline alone, and it is recorded here so it is not mistaken for done. It is
also why the reference game's reaction currently lives in the editor overlay.

**Any reflected property (2026-08-20).** Every numeric inspector row carries a
key diamond, so what can be keyed is whatever the inspector can show: a light's
intensity, a fog density, a scalar on a component a game defines. A position or
a scale gets ONE diamond for the row that keys all three axes, because nobody
means to key two axes of a position and leave the third behind. Spec §9's "any
reflected component property" is the addressing the tracks already used; this is
the surface that reaches it.

Keying reads the value through reflection, which needs whole-world access and
therefore cannot share a system with a mutable timeline — the press records the
ASK and an exclusive system performs it, the same request/perform split the rest
of the editor uses.

**Layer 1, first track (implemented 2026-08-20).** A `Timeline` of `Track`s, each
addressing ONE scalar field the way a patch does — a type path plus a reflect
path — with keys sorted by time, linear between them and HELD outside them (a
platform with two keys waits at each end; extrapolating would send it into
space). `Space K` records where the selection is at the playhead, `Space Space`
plays and pauses, `Space 0` rewinds. Position only so far: it is what a
prototype animates nine times in ten, and it needs no rotation decomposition to
be honest about.

**Resolved: evaluation is not history.** Authoring a key is an edit and undoes
like one. Moving the playhead is NOT, and writes straight to the component
rather than through `EditScope`. A scrub that pushed a transaction per frame
would bury every real edit under thousands of entries and quietly redefine undo
as "rewind time", which is a different verb. The keys are the source of truth;
what the playhead leaves on screen is a view of them, and the probe asserts that
scrubbing changes the undo depth by zero.

Evaluation also yields while a gesture is running, which the probe found the
hard way: a track that already drives a field would otherwise snap an object
back in the same frame the user moved it, so a second pose could never be keyed.

**Persisted as its own asset** (`timeline.ron`, format 1). Spec §9 calls this a
timeline ASSET, and a sidecar is what that means here: its own envelope, its own
format version, an atomic write with a backup — the shape the material library
already establishes. Keeping it out of the level's hand-written serde means a
timeline can gain fields without touching the scene format at all, and a FUTURE
version refuses loudly rather than silently dropping tracks it cannot read.

The status line shows where time is, because scrubbing had no on-screen presence
whatsoever: a playhead parked mid-animation was indistinguishable from a scene
somebody had moved by hand.

**The track view** (`Space T T`) shows a row per track — named after the entity
and the field, not the uuid — with a diamond at each key and a cursor at the
playhead. Pressing a strip scrubs there, because a timeline is a control and not
a readout. Rows rebuild when the tracks change; the cursor moves every frame and
is a position update rather than a rebuild, since rebuilding a panel sixty times
a second to move one line is how a UI starts dropping frames.

Its appearance is UNVERIFIED — a window screenshot of chrome is a black frame
whenever the terminal is in front. What IS verified is the part most likely to
be silently wrong: each mark's fraction equals its time over the duration, the
cursor's percentage matches the playhead, and the panel is laid out with a real
size, inside the window, clear of the status bar. That geometry check
immediately earned itself by catching a units bug — `ComputedNode` reports
physical pixels while the window reports logical, which on a retina display
differ by two.

**Layer 2 — Animation graph (per-rig).** Built on `bevy_animation`'s runtime
(`AnimationGraph` blend/mask nodes) — we build the authoring layer, not a pose engine.
- **Authoring**: the graph is a hot-reloadable data asset (states, transitions, blend
  spaces, parameters) edited through the reflection inspector, with a **live preview
  panel** (pick a rig, scrub parameters, watch the blended result). A visual node editor
  is a post-1.0 layer over the same asset.
- **Scope — full-featured for 1.0**: state machines with transition rules + blend times,
  1D *and* 2D blend spaces, N-layer masking, additive layers, sync markers (phase
  matching), and IK (foot placement, look-at). Build order within the crate: minimal core
  (states/transitions/1D blend/one mask split) must land and be dogfooded before the
  advanced tier; the advanced tier gates 1.0, not the first playable.
- **Movement: both models from day one, per-state.** Code-driven (gameplay/physics owns
  movement, graph matches pose — the prototyping default) *and* root motion (animation
  drives displacement) selectable per graph state. **Hard requirement**: root-motion
  displacement is extracted and fed through the movement/prediction pipeline as movement
  intent — never written directly to transforms — so lightyear prediction/rollback treats
  both models identically. This interaction gets a design doc and a de-risking spike
  before implementation; it is the hardest integration in the animation system.
- **Retargeting stays deferred** post-1.0; clips import through the pipeline as
  first-class assets (UUIDs, processing, references) from day one.

## 10. Milestones

Milestones are named for **what a game developer can do at the end** — not what
infrastructure got built. Every milestone is dogfooded on `template_game` (and, once one
exists, the team's real game); its exit criteria are executable acceptance tests written
when the milestone starts (§8 guardrails). Architecture work that doesn't move a
milestone's game-dev verb rides along inside one — it never gets a milestone of its own.

0. **"The bets are validated."** De-risking spikes, timeboxed: `EditQueue` +
   reflection-undo at scale; the cell-partitioned format surviving a real git merge of
   divergent edits; **bevy_ui/feathers as the editor shell** (docking, virtual lists,
   property grids, text editing at editor scale — egui behind the WidgetKit seam is the
   fallback if it fails); and the **BSN foundation spike** — prove the BSN-first policy's
   load-bearing claims (§5): prefab overrides expressed as BSN patches, variants as
   inheritance, `SceneId` riding inside BSN payloads, and the envelope wrapping a
   BSN-semantic payload; produce the initial BSN gap ledger. **M0 is a hard gate with a
   pre-written fallback tree** — the foundation stacks are young and their risks are
   correlated (BSN + feathers + bevy_ui_widgets churn together with each Bevy release),
   so each spike carries its fallback decision *written before the spike runs*: feathers
   fails at editor scale → egui behind the unchanged `WidgetKit` seam; BSN runtime
   wobbles → keep BSN *semantics* (patches/inheritance in our own envelope payload)
   without the BSN runtime until it matures. A bad spike week triggers a decision
   procedure, not a crisis. *A failed spike changes the spec before Phase 1 code
   exists.*
1. **"I can walk around my game, and the editor is inside it."** Workspace + `editor_api`
   skeleton; `game_framework` lifecycle boots `template_game` to a walkable stub; editor
   overlay toggles in and out; actions/keymaps/palette/which-key live; editor strips
   from release builds. *Game-dev outcome: a game binary with an editor inside — the
   product's identity, provable in the first milestone.*
2. **"I can graybox a level, play it, and iterate without fear."** Scene foundation
   (registration, UUIDs, versioned atomic save/load), `EditQueue` undo, selection,
   transform tools + gizmo state machine, primitives + insert with placement solvers,
   grid + freeform paradigms in basic form — **and play/pause/reset**, pulled forward
   because playing your level is the whole point. *Outcome: build a graybox arena, walk
   it, tweak it, undo mistakes, save it. The edit→play→edit loop exists.*
3. **"I can make it mine, and hand it to a friend."** Reflection inspector, hierarchy,
   material assets + library, data-driven gameplay components (stat tables/rules — the
   designer surface), **Rust hot code reload**, and a *minimal* publish (`editor publish`
   → runnable zip, single profile, no gates beyond boot). *Outcome: gameplay tuning
   without recompiles, styled levels, and the first build a friend can play. Publish
   value arrives one milestone early; full pipeline rigor comes in M4.*
4. **"I can turn real assets into prefabs and paint levels with them."** Asset pipeline
   (import→validate→process→cook), prefabs with overrides/nesting/variants/baking +
   layout metadata authoring (socket gizmos, kits), and the assisted-layout core: snap
   kits, architectural painting, true-shape paradigm. *Exit: the barrel workflow (§6)
   end-to-end, plus: import a wall kit, author its sockets, paint a building, play it.*
5. **"An idea becomes a playable demo in a day."** The rest of the prototyping toolkit as
   independent feature crates proving the `editor_api` contract (build matrix green with
   any subset): terrain (1.0 tier: proc gen + basic sculpt), splines, scatter, procedural
   paradigm generators, animation sequencer, VFX authoring, post-effects, shader
   authoring; animation graphs (minimal core first, advanced tier before 1.0).
   *Outcome: the DoD's inner loop — trial a game idea end-to-end in days, not weeks.*
6. **"My friends can playtest a round online."** lightyear session flow (listen-server),
   game UI kit + editor UI authoring (menus/HUD for template_game), audio event layer,
   full publish profiles + gates, playtest telemetry + input replay, shared derived-data
   cache, asset dependency graph/redirectors. *Outcome: a networked round loop published
   to real playtesters — the DoD's outer loop closes.*
7. **"Someone who isn't us ships a prototype."** The 1.0 gate: a developer outside the
   core team runs the entire Definition of Done scenario — assets → prefabs → level →
   play/iterate → publish to playtesters — **without editor source changes and without
   asking questions the docs can't answer.** Plus: settings UI, validation panel,
   cheat-sheet, migration tooling, performance pass, docs book, Bevy version upgrade
   executed under the phase-boundary policy. *Their friction list is the 1.0 punch list.*

---

## 11. Reference Material & the V1 Quarantine Policy

- `01-REVIEW.md` — the v1 post-mortem: full failure catalog and keep-list.
- Treat v1 *behavior* as a spec suggestion and v1 *code structure* as a cautionary tale.
  When in doubt: fewer features, finished; every invariant by construction.

### V1 quarantine policy (binding on every implementer, human or AI)

v1 is the most convenient reference and therefore the most dangerous one. These rules
prevent its patterns from leaking into v2:

1. **The spec is the only source of architecture.** v1 may be consulted for exactly two
   things: *interaction behavior* ("how did snapping feel," "what did the palette do on
   Enter") and the *explicit keep-list* below. If the spec is ambiguous about how to build
   something, the answer is a spec amendment (§8 guardrail 1) — **never** "look at how v1
   did it." An implementer who cannot decide without opening v1 source has found a spec
   gap; file it.
2. **No copy-paste across the boundary, ever.** Nothing outside the keep-list may be
   copied from v1, in whole or in part — not helper functions, not system shapes, not
   file layouts, not names. Fresh implementation against the v2 contracts only. (Names
   matter more than they look: importing v1's vocabulary imports its structure.)
3. **Keep-list imports go through a port gate.** The carried items —
   `bevy_spline_3d`, `bevy_outliner` (algorithm only), `bevy_vfx` (data model + GPU
   pipeline), grid/channel materials, `marks.rs` (design), the drag math
   (`gizmos/transform.rs:28-106`), the snap solvers (design) — enter v2 only via a port
   PR. The v1 UI keep-list (`theme.rs`, `fuzzy_palette.rs`, `reflect_editor.rs`) carries
   as **design only** — palettes/theming/reflection-editing patterns re-expressed on the
   bevy_ui/feathers stack (§7); their egui code never ports. Every port PR: deletes dead code and
   dead architectures; adds the tests v1 never had; replaces any editor integration with
   an `editor_api` implementation (v1 bridge patterns like `SplineEditPlugin` do not
   port); adds `PartialEq`/`Reflect` derives per v2 standards; feature-gates heavy deps;
   and passes the same adversarial review as new code. A keep-list item that can't clear
   the gate gets rewritten instead.
4. **The failure catalog is an executable blocklist.** Every anti-pattern named in
   `01-REVIEW.md` is a banned pattern in v2. The §8 architectural fitness greps are
   seeded from that catalog and grow with it — at minimum: raw `ButtonInput<KeyCode>`
   outside the resolver; `&mut World` in UI systems; serialize-to-compare change
   detection; snapshot-by-convention undo; per-entity-kind match arms outside the
   registry; name-string entity references; hand-maintained per-type lists; parallel
   implementations of an existing widget/helper. CI failure cites the review section
   that motivated the rule.
5. **v1 never enters the v2 workspace.** No git subtree/submodule of v1, no path deps
   into it, no v1 crate published-and-depended-on before its port gate. The v2 repo
   starts empty except for these three spec documents.
