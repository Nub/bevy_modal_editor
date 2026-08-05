# M4 Acceptance — "I can turn real assets into prefabs and paint levels with them"

Written at milestone start (spec §8 guardrail 2). Scope from spec §10 milestone 4
and §6: the ingestion pipeline (import→validate→process→cook), prefabs (BSN-layered
instances/overrides/variants/nesting, prefab edit mode, baking as caches), layout
metadata authoring, and the assisted-layout core (snap kits, architectural painting,
true-shape). Riding along by owner direction: the full material editor
(memory: material-editor-vision).

## Executable acceptance tests

| # | Test | Where |
|---|---|---|
| D1 | Import: dropping/importing a GLTF assigns a stable asset UUID + `.meta` sidecar (settings, content hash, pipeline version); re-import PRESERVES the UUID (references survive); evaluate/wrap Bevy's asset-processing before building parallel machinery (ledger entry if we diverge) | `editor_assets` |
| D2 | Validate: validator registry (games extend it) runs at import — units sanity, missing materials at minimum; failures land in a problems surface, never silently pass | `editor_assets` |
| D3 | Process: hash-keyed cached outputs, deterministic (same input+settings ⇒ byte-same output, CI-proven on a fixture corpus); processor version bump invalidates | `editor_assets` |
| D4 | Prefab asset: versioned envelope; entity hierarchy + component values + asset references BY UUID + named parameters; instances serialize as `{prefab_id, transform, overrides}` — NEVER the expanded tree; stamping through the regenerate-hook path | `editor_prefabs` |
| D5 | Overrides: per-field deltas as BSN-style patches; inspector shows overridden fields distinctly; revert-override and apply-to-prefab verbs; source edits propagate live to all non-overriding instances | `editor_prefabs` + owner |
| D6 | Nesting + variants: prefabs compose prefabs; variant chains inherit and override; cycle creation is a typed error surfaced at author time | `editor_prefabs` |
| D7 | Prefab edit mode: isolated edit context with its OWN undo scope, modal and keyboard-driven; save propagates to instances | `editor_ui` + owner |
| D8 | Baking: bakes are caches, never source of truth — delete all bake output, `editor bake` reproduces it bit-for-bit (CI on fixtures); artifacts keyed by input content hash + baker version; staleness surfaced, never silently served | `editor_api` bakers + CLI |
| D9 | Layout metadata: sockets/kit tags/footprints author-able on a prefab (socket gizmos in-viewport); snap-to-socket placement works with keyboard+mouse | `editor_prefabs` + owner |
| D10 | Assisted layout: a wall kit with authored sockets paints a building (run placement, corner resolution); true-shape snap uses actual geometry, not AABBs | owner |
| D11 | Material editor v2 (owner): dedicated editor surface with visual preview, full StandardMaterial coverage (textures via the pipeline, emissive, alpha modes), feathers color-picker widgets; asset-history undo for material edits | `editor_ui` + owner |
| D12 | THE BARREL WORKFLOW (spec §6, the milestone exit): drop `barrel.glb` → auto-import + validation → create prefab (colliders, materials, gameplay components) → place 50 instances with overrides → re-export the GLB → re-import → processing → prefab updates → all 50 instances update, overrides intact | end-to-end + owner |

## Status (2026-08-05)

D1–D12 all implemented with executable coverage: unit/property tests per
crate plus five session probes in `verify.sh full` + CI (PREFAB, USER 27,
KIT 10, BARREL 21 — the D12 exit flow end-to-end incl. flatten-to-entities
+ collider/gameplay config — and MATERIAL 17 — the D11 editor with
asset-scoped undo). Remaining to CLOSE the milestone: the owner hands-on
checklist below.

## Owner hands-on checklist (automated: HANDSON_PROBE, plus per-row probes)

Every row is now driven end-to-end by probes in `verify.sh full` + CI; the
column notes which probe holds the coverage.

- Import a real GLB; watch it validate/process; inspect its meta identity.
  — BARREL (import/identity/validation), HANDSON (texture import too)
- Build a prefab from it; place instances; override a few fields; edit the
  prefab source and watch non-overridden instances follow. — HANDSON
  (inspector-committed per-field member override survives a template edit
  that propagates to non-overriding instances)
- Author sockets on a wall kit; paint a building span; play it. — KIT
  (painting), HANDSON (socket authoring via add-component + gizmo,
  play/reset with authored content intact)
- Full barrel workflow (D12) without touching a config file. — BARREL
- Material editor: build a textured material end-to-end with undo. — MATERIAL
  (editor + asset undo), HANDSON (texture bound by real chip click, preview,
  undo/redo)

## Explicit non-goals for M4

Terrain/splines/scatter/VFX/sequencer (M5), networking (M6), publish profiles/
gates beyond M3's minimal publish (grows with cook in late M4 only if needed),
LOD/texture-compression processors beyond the minimum needed for the barrel
workflow (registry must support them; shipping them all is M5+).
