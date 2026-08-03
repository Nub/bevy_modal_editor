# M3 Acceptance — "I can make it mine, and hand it to a friend"

> **PASSED 2026-08-02.** C1–C9 green (C8 as gate-authorized fallback; true
> hotpatching deferred — path documented in ledger #9). Owner verification ran as
> iterative hands-on rounds throughout the milestone: panel focus model, hierarchy
> nav/reparent, inspector (full-archetype, name-in-header, Tab across all widget
> kinds, drag-scrub), Spinner designer-surface, material library/assignment/live
> re-shade, publish artifact boot-verified, fast-relaunch probe-verified.

Written at milestone start (spec §8 guardrail 2). Scope from spec §10 milestone 3:
reflection inspector, hierarchy, material assets + library, data-driven gameplay
components (the designer surface), Rust hot code reload, and a *minimal* publish.
Enablers from §7 that must land first: the panel framework (`EditorPanel` trait +
layout manager), the widget kit (M0 F6–F8 findings baked in), panel-focused keymap
contexts, and the one palette engine (every searchable list an instance of it).

## Executable acceptance tests

| # | Test | Where |
|---|---|---|
| C1 | Panel framework: panels register via `FeatureRegistry` (`.panel(...)`), the layout manager owns docking/focus; panel focus is a keymap CONTEXT (focus target, not a mode); keyboard panel navigation cycles panels; a panel cannot draw its own window chrome | `editor_ui` |
| C2 | Hierarchy: lists every `SceneId` entity as a tree; selection syncs both ways (click row ↔ viewport outline); keyboard nav (j/k, gg/G jumps) in the hierarchy context; reparent flows through `EditScope` (`Op::Reparent`) and is undoable | `editor_ui` + headless |
| C3 | Inspector: ONE recursive reflection editor over the selection — no per-component snapshot structs; `TypeId → widget` override registry (Transform-as-Euler, Color at minimum); every field edit emits an `EditScope` patch (same path as spawn/load/undo/prefab); field-level undo lands one history entry per completed edit, not per keystroke | `editor_ui` + conformance |
| C4 | Widget kit: property rows (drag/number/vec3/color/checkbox), section headers, empty states — F6–F8 rules enforced centrally (label-above-controls, never-zero-size fields, reset-to-default on empty blur, whole-box focus, background-token framing); list widgets virtualize (hierarchy with 10k entities stays interactive) | `editor_ui` |
| C5 | Palette engine: trait-driven items (label/category/keywords/enabled/suffix), category grouping, fuzzy match built once and re-filtered on query change only; commands, insert, find-object, and the materials library are ALL instances of it (no bespoke lists) | `editor_ui` |
| C6 | Materials: a material is a versioned asset (envelope format) with a library palette; assigning to a selection is one undoable transaction; scene references materials by asset id and survives save→load→save byte-identical | `editor_scene` + owner |
| C7 | Data-driven gameplay components: `template_game` registers a plain reflected stat component (no editor code beyond registration); it appears in the inspector, edits are undoable, and it round-trips the scene format | `template_game` test |
| C8 | ~~Hot code reload~~ **Fallback invoked; true hot reload deferred by owner decision (2026-08-02).** Spike (`spikes/hot-reload`) found no viable path on Bevy 0.19 (ledger #9, re-evaluated each phase-boundary upgrade). Shipped: fast-relaunch (`editor.reload`, ctrl+shift+r) — save + session sidecar + restart + staged restore of scene/selection/camera/editor-state, probe-verified; pairs with `cargo watch`. Undo history is process-local and not preserved | fallback shipped |
| C9 | Minimal publish: `editor publish` (CLI) produces a runnable zip of `template_game` WITHOUT the editor feature (A6 stripping verified in the artifact), single profile, failing loudly at the first gate; CI publishes on merge | CLI test + CI |

## Owner hands-on checklist (closes the milestone)

- Open hierarchy + inspector panels; move focus between viewport/hierarchy/inspector
  by keyboard; Escape always returns to the viewport in Normal.
- Select a cube in the viewport → same row highlights in hierarchy; select a row →
  outline in viewport. Reparent a cube under another; undo it.
- Edit Transform in the inspector (drag + typed entry); Euler rotation editing; one
  undo per completed field edit. Edit a game stat component the same way.
- Create a material in the library, tweak its color in the inspector, assign it to
  boxes; save, relaunch, everything styled as left.
- Change a gameplay constant in Rust; hot reload (or fast-relaunch fallback) inside
  the session; verify the tweak while playing.
- Run `editor publish`, unzip on a clean directory, boot the game, walk the level.
- Hand the zip to a friend (or a second machine): it runs.

## Explicit non-goals for M3

Asset ingestion pipeline + prefabs (M4), assisted layout/snap kits (M4), terrain/
splines/scatter/VFX (M5), multiplayer (M6), publish gates beyond boot + multi-profile
packaging (M4+), inspector multi-select merged editing (nice-to-have, not gating).
