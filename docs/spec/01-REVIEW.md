# bevy_modal_editor — Professional Review (v1 Post-Mortem)

Reviewed 2026-08-01 across four subsystem audits (editor core, scene/data, UI, support crates).
~70k lines of Rust, 200 files, 240 commits, 11 workspace crates. Verdict up front:

> **The interaction design is the project's real asset and should be treated as the spec.
> The implementation — accreted through iterative AI patching — is where a rewrite should
> diverge completely.**

Health signals: 3 files with tests out of 200; zero TODO/FIXME markers (nothing was ever
flagged as unfinished, yet much is); CLAUDE.md documents 7 editor modes while the code has 11;
the Nix dev shell only builds on Linux.

---

## What was designed (keep as spec)

These parts were *designed*, deliver real value, and transfer to v2:

- **The modal interaction grammar.** View → Edit/Insert/Inspector/Hierarchy/Blockout/Material
  modes, Escape home, mode-key-toggles-back, Shift bypasses the View-only gate. Home-row
  transform ops (Q/W/E/R/T), A/S/D axis constraints, J/K stepping. Coherent and learnable.
- **Camera marks as vim registers** (`src/editor/marks.rs`): 1–3 ortho views, 4–9 jump,
  Shift+4–9 set, backtick jump-back with a position stack.
- **`SnapSubMode` placement solvers** (Surface/Center/Aligned/Vertex, scroll-cycled) shared
  between Insert preview and snap-to-object. Aligned mode (AABB face mating) is genuinely good.
- **Screen-space 1:1 drag math** (`gizmos/transform.rs:28-106`) — pixel-accurate manipulation
  independent of distance/FOV.
- **Marker-driven serialization + regenerate philosophy**: save semantic components, rebuild
  runtime state on load. Files stay small and forward-compatible. `build_editor_scene` is a
  verified single choke point used by save, undo, prefabs, and play/reset.
- **`MaterialRef::Library | Inline`** semantics (shared preset vs per-entity override).
- **`bevy_editor_game` as the game/editor contract**, proven by three demo games exercising
  custom entities, validation, materials, and lifecycle events.
- **UI seeds worth keeping nearly verbatim**: `theme.rs` (semantic colors, frame helpers),
  `fuzzy_palette.rs` (trait-based fuzzy search widget), `reflect_editor.rs` (recursive
  reflection property editor — "the most professional file in the layer").
- **Crates**: `bevy_spline_3d` (closest to publishable; its `EditorSettings` host-integration
  pattern is the right shape for every editor-adjacent crate), `bevy_outliner`'s JFA
  render-graph architecture, `bevy_vfx`'s serializable module-stack data model + GPU buffer
  caching, `bevy_grid_shader`/`bevy_channel_mat` verbatim.
- **Library-bridging pattern** in `SplineEditPlugin`: disable a third-party crate's hotkeys and
  sync its settings from editor state instead of forking it.

## What accreted (the failure catalog)

### Live bugs found during review
- **Undo off-by-one**: nudge, J/K stepping, and all spline hotkeys queue the undo snapshot via
  deferred `Commands` but mutate immediately in the same system — the snapshot captures
  *post*-mutation state; first undo is a no-op (`operations.rs:275`, `transform.rs:623`,
  `spline_edit.rs:190+`).
- **Key conflicts**: Ctrl+R fires both redo and Place mode; U fires undo and a modeling op in
  Blockout mode; Shift+J both steps and switches to Effect mode.
- **Lossy copy/duplicate/paste**: clipboard stores only `kind + position + rotation` —
  discards scale, name, materials, edited components, children, parametric dims, spline
  geometry. GLTF/scene/prefab/decal/particle entities aren't duplicatable at all. (The
  full-fidelity serializer already existed; copy/paste just didn't use it.)
- **Silent dirty-flag gaps**: `detect_scene_changes` hardcodes 7 component types; edits to fog,
  decals, blockout shapes, VFX, and all game-registered components don't mark the scene
  modified → data loss on close-without-save.
- **Destructive load**: the whole scene is despawned *before* the new file is parsed; corrupt
  file ⇒ empty world. No atomic writes, no backup.
- **`SceneSource` double-load** on round trip; **material texture paths lost** on registry
  read-back; **CSG boolean geometrically wrong** (centroid-only classification); **VFX Ribbon**
  selectable in UI but has no renderer.

### Structural failures
- **No action/keymap layer.** ~40 systems poll raw `KeyCode`s; conflicts are invisible by
  construction; rebinding impossible; input guards inconsistent (some systems have none).
- **No system ordering.** Zero `SystemSet`s across ~35 Update systems; drag-vs-snapshot and
  selection-vs-drag are frame-order races; 3-frame startup delay hacks instead of ordering.
- **Undo is O(scene) text snapshots**, enforcement by convention at ~45 call sites, entity IDs
  invalidated on every undo.
- **N-places-per-type**: adding one entity kind touches ~6 locations across 3 files (spawn
  match, allow-list, register_type, regenerate block, dirty-flag query, copy/duplicate chains).
- **The generic solution and the manual solution coexist**: `reflect_editor.rs` obsoletes
  ~1,500 lines of hand-rolled per-component inspector code that was still being extended.
- **Change detection by serializing components to RON strings twice per frame** and comparing
  strings (vfx/effect/material panels) — because `PartialEq` was never derived.
- **No panel framework**: window anchoring/pin/displace logic copy-pasted 8×; 36 exclusive-
  world UI systems mutate components directly; most panels bypass undo entirely.
- **No versioning**: scene files keyed on full Rust type paths, zero schema version — any type
  rename or Bevy upgrade orphans every saved file.
- **Prefabs are expanded copies**, not references: no live link, no overrides, instance
  contents serialized into every scene.
- **Name-string entity references** resolved by first-match scan; renames silently break
  spline followers and scatter placers.
- **Scope creep as the defining trait**: 11 modes, a 10k-line mesh modeler (one tested file,
  untrusted half-edge boundary walk), GPU particles, an effect sequencer, gaussian splatting,
  navmesh AI, procedural scatter, roads — each 60–80% done, none 100%.
- **Copy-paste infrastructure**: preset disk persistence duplicated 3×; four independent Vec3
  editors; `rotation_from_normal` duplicated; AABB-height logic re-implemented 5×; insert-mode
  snap logic forked from snap-to-object; dead plugin + orphaned shader shipped in bevy_outliner.

### Meta-lesson
Every failure above shares one root cause: **invariants enforced by convention instead of by
construction.** "Remember to queue a snapshot," "remember to add the component to five lists,"
"remember which system owns Escape." A professional rewrite makes each of these impossible to
get wrong: edits flow through one API that snapshots automatically; components register once;
keys resolve through one table. That principle — *centralize every convention into a
registration point* — is the single most important directive for v2.

## Disposition of v1 code

| Carry into v2 | Rewrite from design | Cut / defer |
|---|---|---|
| bevy_spline_3d (minus bundled cameras/binary) | Editor core (actions, tools, undo) | Mesh modeler beyond blockout ops |
| bevy_grid_shader, bevy_channel_mat | Scene serialization (versioned, hooks) | Gaussian splatting (feature-gate) |
| bevy_outliner algorithm (delete dead plugin) | Prefabs (references + overrides) | Effect sequencer (until VFX stable) |
| theme.rs, fuzzy_palette.rs, reflect_editor.rs | All UI panels (framework + reflection) | Camera/Particle/AI/Effect modes as modes |
| bevy_vfx data model + GPU pipeline | bevy_procedural instance lifecycle | Road intersections (keep in spline crate) |
| bevy_editor_game contract (fix egui leak) | Preset persistence (one generic service) | |
| Modal grammar, marks, snap solvers, drag math (as spec) | | |
