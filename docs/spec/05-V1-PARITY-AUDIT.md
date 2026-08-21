# 05 — V1 Parity Audit (2026-08-02)

**Why this exists.** Owner concern: *"I'm worried that your analysis and recreation
documents lack many features from v1."* Verified example: the palette preview pane
(carried in spec §7, shipped only as text docs until 2026-08-02). This document is
the systematic answer: v1's complete user-facing feature surface (mined from
`main`, ~90k lines across 14 areas) diffed against everything the four founding
documents + acceptance docs promise.

**Method.** Five parallel inventories: v1 modeling/gizmos/navigation, v1 ui/
(37 files), v1 editor/scene/prefabs/commands/selection, v1 materials/effects/vfx/
asset_libraries, and the v2 promised-feature map with milestone assignments.

**How to read.** Every v1 feature lands in exactly one bucket:

- **COVERED** — spec promises it with a milestone. Risk is scheduling, not omission.
- **DROPPED-CONFIRM** — spec explicitly cuts/defers it with a reason. Listed so the
  owner re-confirms each cut was intentional, knowing what it contains.
- **GAP** — v1 has it; the spec neither promises nor explicitly drops it. Each has
  a proposed disposition. **These are the regression candidates.**
- **PROMISED-UNSCHEDULED** — the spec promises it but no milestone carries it.

Dispositions marked ⚠ need an owner decision; the rest are proposals that ride
into the spec via the normal amendment process once triaged.

---

## 1. COVERED (spec promises it — abbreviated; risk is delivery, not omission)

| v1 feature | Where spec covers it | Milestone |
|---|---|---|
| Prefab create/spawn/edit (v1: no overrides, no close, no re-sync) | §6 — v2 exceeds v1: overrides, propagation, nesting, cycles | M4 (in flight) |
| Material editor (full PBR + textures + presets + preview + lighting rigs) | M4 D11 (owner rider) — v1 surface is the bar | M4 |
| Material library / inline-vs-reference / preset palette / auto-persist | §6, M3 C6 (delivered) | M3 ✓ |
| VFX authoring (emitters, curves, gradients, module stacks, 17 presets) | §11.3 port gate, 04 §11.2 | M5 |
| Splines (types, control-point editing, followers) | 04 §11.1 port gate | M5 |
| Procedural placement / scatter (v1 ProceduralPlacer) | §9 paradigm 4, bevy_scatter | M5 |
| Parametric blockout (stairs/ramp/arch/L-shape, live params) | §9 snap-kit foundation | M4 |
| Insert ghost preview, surface snapping, Shift-place-and-continue | §4, 03 Insert | M2 ✓ |
| Snap sub-modes Surface/Center/Aligned/Vertex + Alt edge-snap + guides | §4, 01 keep | M2 (partial — see §4.13) |
| Play/pause/reset with exact restore | §3, M2 B10 | M2 ✓ |
| Palette family incl. preview pane, pinned +New rows, find-object | §7 | M1–M3 (preview shipped 2026-08-02) |
| Reflection inspector + custom editors + docs preview | §7, M3 C3 | M3 ✓ / docs preview pending |
| Hierarchy tree, filter, reparent, two-way sync | §7, M3 C2 | M3 ✓ |
| Fly camera, frame selection, ortho views | 03 Normal | M2 ✓ |
| Camera marks as registers (`m{a-z}`, `'{a-z}`, `''`) | 01 keep, 03 global | carried, **never tested/implemented** — see §4.14 |
| Undo/redo (v2: transactional deltas vs v1 whole-scene RON) | §5 | M2 ✓ |
| Validation registry + problems surfacing (v1 popover) | 04 §7, M4 D2 | M4/M7 |
| Asset import/processing (v1 had ad-hoc GLTF libraries) | §6 pipeline | M4 |
| Grid + rotation snapping, settings persistence | §4 EditorSettings | M2 ✓ |
| Publish (v1 had none) | §6 | M3 min ✓ / M6 |
| Viewport shading cycle, grid/gizmo/physics-debug toggles | 03 `Space t` | M2 (verify all four exist) |
| Auto collider generation for imported models | §6 processors | M4 |
| Custom per-entity-type gizmos from game code | 04 §7 GizmoCtx | M2+ |

## 2. DROPPED-CONFIRM — explicit cuts the owner should re-ratify ⚠

Each cut was made in the spec, but the inventories show exactly how much user
surface each one contains. Confirm (or amend) each:

1. **Mesh modeling — the whole 16k-line suite.** Vertex/edge/face selection (8
   selection paradigms incl. flood-fill, UV-space, freeform lasso), extrude/inset/
   push-pull/bevel/bridge/edge-loop/weld/dissolve, mirror/smooth/subdivide/
   Catmull-Clark/fill-holes/plane-cut/QEM-simplify/remesh, auto-smooth normals +
   hard edges, box/planar/cylindrical UV projection, seam-based unwrap, 2D UV
   editor. Spec: cut entirely, "DCC owns modeling"; returns post-1.0 as a feature
   crate. *Note: v1's CSG boolean was implemented but never reachable.*
2. **Gaussian splats.** Insert .ply/.splat/.gcloud, hot-swap source, cloud
   settings, auto splat camera. Spec: non-goal/defer. ⚠ **The owner actively
   works with splats on main (LFS-tracked, GaussianScenePlugin fixes in recent
   history) — this cut looks wrong for this project. Propose: return as an M5
   feature crate proving the `asset_kind` + custom-render extension seams.**
3. **Effect sequencer** (7 trigger types × 15 action types, rule cards, timeline
   strip, presets). Spec: "cut until VFX is stable" — **no return milestone
   named.** Propose: M5 rider after the VFX port, reusing its card UI.
   **Updated 2026-08-20:** the timeline strip landed early on M4 (spec §9
   animation) and the first TRIGGER TYPE — a named volume — landed with it as
   an M4 rider. The rule cards and the action catalogue are still M5; what
   exists is one trigger and one action shape (fire a named effect), which is
   the seam the rest hangs off rather than the feature itself.
4. **11 modes → 3 + contexts.** Camera/Particle/AI/Effect/Material/Inspect/
   Hierarchy modes become panels/contexts. Already validated by M1–M3.
5. **Road/intersection authoring** stays inside the spline crate (M5).
6. **Networked play-in-editor, scripting VM, standalone editor** — confirmed cuts.

## 3. GAP — v1 features the spec neither promises nor drops ⚠

The dangerous bucket. Proposed dispositions:

| # | v1 feature (source inventory) | Proposal |
|---|---|---|
| 3.1 | **Decals** — clustered + forward, 3 texture slots, depth fade, volume gizmo | ADD M5 (feature crate `editor_decals`, kind + inspector editor + processor). Games need decals; nothing in spec mentions them. |
| 3.2 | **Volumetric fog volumes** — insertable kind + full inspector editor | ADD M5 alongside decals (both are "render-component kinds with editors") |
| 3.3 | **Light editing UX** — point/sun insertable kinds, color/intensity/range/shadows/volumetric editors, light gizmos (bulb/arrow/range sphere), meshless-selection circle | ADD M4 rider or early M5: template_game registers light kinds; editor ships light gizmos + inspector overrides. A level editor that can't place lights isn't one. |
| 3.4 | **Editor camera render settings** — tonemapping/exposure presets/bloom/AA/SSAO/DOF/color grading/fog panel + apply-to-game-cameras | PARTIAL: §9 has the *game* post-effects stack (M5). ADD the editor-camera settings panel to that same M5 item; the §9 data-driven stack is the mechanism. |
| 3.5 | **Entity locking** (`Locked`: excluded from selection/transform, padlock UI) | ADD M4 (small: a core marker + selection/gesture gates + hierarchy glyph) |
| 3.6 | **Scene dirty guards** — Save/Discard/Cancel prompts before load/generate/quit; window-title dirty star | ADD M4: SceneIoLock exists, statusbar shows dirty; the *prompt* flow is missing from spec. Cheap, prevents data loss. |
| 3.7 | **Arrow-key nudge** by grid step | COVERED-ISH by counts+`hjkl` design (unscheduled, §4.6 below); fold into that decision |
| 3.8 | **Distance measurements** (`Shift+M` chained readout) | DEFER post-1.0 unless owner wants it; tiny feature, real utility |
| 3.9 | **Asset browser as file browser** — recursive fuzzy over assets/, directory grouping, save-as-new-file flow, 7 operation flavors | RESOLVED as a SPLIT (2026-08-21). The BROWSE half lands in M4 as spec §6 "Asset browser (`editor_ui::assets`)": a docked panel over `ModelLibrary`, directory grouping, per-asset pipeline state. Searching stays in the palette by §7's one-engine rule rather than growing a second matcher. The SAVE-AS half stays on `:e`/`:w`, where the real blocker is that `scene.open`/`scene.save` take no argument — parameterized actions, not a browser. A browser that mixed them would offer "open" on a `.glb`. |
| 3.10 | **Demo scene / museum generator** (labeled showcase grid of all content) | DROP formally (template_game's graybox level is the successor); note in spec non-goals |
| 3.11 | **World tab** (debug all-entities browser) | DROP formally; `bevy_remote` / external tooling covers it |
| 3.12 | **BRP remote protocol + FPS overlay** | DROP formally or ADD M7 one-liner (bevy_remote plugin is nearly free) |
| 3.13 | **Material copy/paste across entities** (`Y`/`P`) | FOLD into M4 D11 scope explicitly |
| 3.14 | **Preview mode** (hide all gizmos/chrome for screenshots) | FOLD into `Space t` toggle family (one compound toggle), M4 rider |
| 3.15 | **Per-mode panel pinning** | DROP formally — v2's dock + panel-focus model supersedes it; confirm |
| 3.16 | **Physics-aware editing** — sleep-while-dragging, physics sim toggle, avian integration in template_game | RESOLVED (owner, 2026-08-06, landed in M4 tail): avian3d 0.7 lives GAME-side in template_game; the editor authors DATA (`BoxCollider {half_extents, offset}`, `PhysicsBody Static/Dynamic`) and avian components derive at runtime. Simulation pauses whenever the editor owns input (the drag-sleep pattern for free — nothing simulates while editing). `game.fit-collider` sizes colliders from visual bounds (asset prep). PHYSICS_PROBE covers pause/fit/fall/reset. |
| 3.17 | **Spline proximity picking** (click near curve, no collider) | FOLD into M5 spline port (pick-arbitration already specced) |
| 3.18 | **Name deduplication** on rename ("Crate" → "Crate 2") | Obsolete as *correctness* (UUIDs fixed name-refs) but ADD as M4 polish — duplicate names still confuse humans |

## 4. PROMISED-UNSCHEDULED — spec promises with no milestone ⚠

(From the spec coverage sweep; each needs a milestone or an explicit defer.)

1. `bevy_navmesh_kit` (recast/landmass) — in crate list, in **no** milestone. v1 had working navmesh baking + wireframe + agent params. Propose M5.
2. `editor_materials` grid + channel shader extensions ("carried verbatim") — never scheduled. Propose M4 D11 rider (grid shader is the blockout default material in v1).
3. Play-mode ephemeral edits + "apply to scene" verb — designed §3, tested nowhere. Propose M5.
4. Undo history UI — descriptions are retained for it; no panel scheduled. Propose M7.
5. Merge-conflict resolution UI on load — §9 promise, unscheduled. Propose M6.
6. Counts / registers / `.` repeat — designed M1, implementation unassigned. Propose M5 (after parameterized actions).
7. Game input layer: controller glyphs + player rebinding UI — implied M6; make it explicit.
8. Save-game slot service — unscheduled. Propose M6.
9. Localization "from day one" — M1–M4 shipped without it. ⚠ owner: accept retrofit cost or start at M5?
10. Asset tagging + collections — propose M6 with the asset DB.
11. Project/editor version pinning + upgrade flow — propose M7.
12. `editor migrate` CLI — propose M7 (with the standing rule it ships same-release as any break).
13. Aligned/Vertex placement solvers + Alt-edge-snap **visual guides** — spec-carried; M2 only tested Surface/Center. Propose M4 verify-or-implement gate.
14. **Camera marks / ortho views / jump-back** — carried three times over (01 keep, 02 §4, 03), implemented nowhere, tested nowhere. Propose M4 (small, keyboard-first identity feature).
15. Reconcile §1 non-goals ("build on egui_dock…") with §7 zero-egui mandate — editorial fix, this PR.

## 5. Standing lesson

Two of the owner's three reported regressions to date (insert palette empty;
palette preview missing) were **carried-in-spec, never-scheduled-or-tested**
items — bucket 4 failures, not bucket 3. The fix is structural: every "carried
from v1" line in the spec must name a milestone and an acceptance check, and
`docs/spec/02` §10 milestone lists are amended by this audit once the owner
triages the ⚠ items. Flow-level probes (PREFAB_PROBE pattern) are the
enforcement layer for "the user can actually see it."
