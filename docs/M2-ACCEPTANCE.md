# M2 Acceptance — "I can graybox a level, play it, and iterate without fear"

Written at milestone start (spec §8 guardrail 2). Scope from spec §10: scene foundation
(component registration, `SceneId` UUIDs, versioned atomic save/load), `EditScope`/undo,
selection, transform tools with the gizmo state machine, primitives + insert mode with
placement solvers, grid + freeform paradigms in basic form, and play/pause/reset.

## Executable acceptance tests

| # | Test | Where |
|---|---|---|
| B1 | `EditScope` transaction API per RFC §5: set/patch/insert/remove/spawn/despawn/reparent against `SceneId`; inverse capture generic (spike-1 shapes, first-old-value coalescing contract) | `editor_api`/`editor_core` unit |
| B2 | Undo/redo round-trip property: any transaction sequence → undo all → world state equals initial; redo all → equals final (reflected comparison) | `editor_core` |
| B3 | Every `IS_EDIT` action produces exactly one transaction (conformance rule, enforced headlessly) | conformance |
| B4 | Save→load→save byte-identical (versioned envelope, BSN-semantic payload, cell format rules from spike 4: block-per-paragraph, field-per-line, UUID-sorted) | `editor_scene` |
| B5 | Load is non-destructive: corrupt file leaves the current scene untouched, error surfaced; saves are atomic (temp+rename, .bak) | `editor_scene` |
| B6 | Selection: click-pick via raycast, `Selected` state drives outline/gizmo; multi-select extend; select-all/clear actions | headless where possible + owner |
| B7 | Transform gesture state machine: `Idle → Dragging{originals} → Commit/Cancel`; Esc mid-drag restores originals exactly; one undo entry per gesture; axis constraint keys during gesture | `editor_core` + owner |
| B8 | Insert mode: palette-picked primitive, ghost preview follows surface raycast, place + Shift-place-multiple, placement lands as one transaction | owner + test |
| B9 | Grid paradigm basics: grid snap toggle quantizes placement/translation; freeform snap solvers (surface/center) selectable | owner |
| B10 | Play/pause/reset: Play snapshots scene + enters game through the framework path; Reset restores exactly — selection, camera, undo history intact | `template_game` test + owner |
| B11 | Macros ride actions: record `q<reg>`, replay `@<reg>` reproduces the same edits as one coalesced undo entry (first real macro test — the M1 action layer pays off) | `editor_core` |

## Owner hands-on checklist (closes the milestone)

- F12 into the editor over the graybox; click-select a box (outline), shift-click a second.
- `w` → drag → commit; `w` → drag → `Esc` (exact restore); `u`/`ctrl+r` through history.
- `i` → pick "Cube" in the palette → ghost preview on surfaces → place several with Shift.
- Toggle grid snap; feel quantized vs freeform placement.
- Save (`:w` in palette), quit, relaunch, reload — scene identical; corrupt the file by
  hand and confirm the load fails loudly without destroying the open scene.
- Record a macro placing three cubes; replay it; undo once removes the whole replay.
- Play → walk into the placed cubes → Reset → editor state exactly as before play.

## Explicit non-goals for M2

Hierarchy/inspector panels (M3), materials (M3), asset pipeline/prefabs (M4), rotation/
scale gizmo *rendering* polish (functional first), BSN `.bsn` files (ledger #1 — our
envelope only).
