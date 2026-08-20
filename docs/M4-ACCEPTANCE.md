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
| D9 | Layout metadata: sockets/kit tags/footprints author-able on a prefab (socket gizmos in-viewport); snap-to-socket placement works with keyboard+mouse | `editor_prefabs` + owner — PLACE_PROBE asserts a palette placement MATES with no `o` press (2026-08-19: it computed the mate and spawned at the raw cursor point, announcing a snap it had not made; KIT_PROBE missed it by chaining afterwards, which mates through a different path) |
| D10 | Assisted layout: a wall kit with authored sockets paints a building (run placement, corner resolution); true-shape snap uses actual geometry, not AABBs | owner — `fill_measures_its_step_from_the_chained_piece` puts three unrelated instances in the scene and chains from a socket, so the fill's step cannot come from whichever instance a query returned last (2026-08-19: it did, and the original fixture's single instance hid it) |
| D11 | Material editor v2 (owner): dedicated editor surface with visual preview, full StandardMaterial coverage (textures via the pipeline, emissive, alpha modes), feathers color-picker widgets; asset-history undo for material edits | `editor_ui` + owner |
| D12 | THE BARREL WORKFLOW (spec §6, the milestone exit): drop `barrel.glb` → auto-import + validation → create prefab (colliders, materials, gameplay components) → place 50 instances with overrides → re-export the GLB → re-import → processing → prefab updates → all 50 instances update, overrides intact | end-to-end + owner |

### D11 resolved semantics: where an assigned material LANDS (2026-08-19)

A `MaterialRef` sits on the scene entity, but an imported model's geometry does
not: a placed `MeshRef` carries no `Mesh3d` — its meshes live in the derived
gltf subtree (spec §6), and Bevy materials do not inherit down a hierarchy.
Resolved: **an assignment overrides every mesh from the referencing entity down,
stopping at any descendant that is a scene entity in its own right (`SceneId`)
or carries its own `MaterialRef`.** The displaced material is remembered per
mesh (`SourceMaterial`) so removing the reference restores what the artist
exported rather than a guess. Because a GLB resolves asynchronously, the
override is applied on three arrivals — reference changed, library changed, and
meshes appearing (first load or re-import respawn).

Resolution lives in `editor_scene::materials` (it is editor behavior, not game
behavior) and is ordered AFTER the model resolvers, which write the same
component. Covered by MATERIAL: *"the assigned material reached EVERY mesh in
the model subtree"*.

### D11 texture slots: colour space belongs to the SLOT (2026-08-19, format 2)

A material had exactly one texture slot, and every texture loaded with
`is_srgb = true`. That is correct for a base-colour or emissive map and wrong
for every other kind: a normal map holds vectors and a metallic-roughness or
occlusion map holds scalars, and gamma-decoding those on load corrupts every
value in them. Adding slots without also declaring their colour space would
have shipped that corruption silently — the render looks plausible and is
simply wrong.

Resolved: **the slot is declared, and the slot decides the colour space.**
`TextureSlot` (BaseColor, Normal, MetallicRoughness, Occlusion, Emissive)
carries `is_srgb()`, and `to_standard_material` loads each map through it, so
the contract cannot be forgotten at a call site. `MaterialDef.textures` is a map
keyed by slot rather than a field per map, so the panel renders its rows from
`TextureSlot::ALL` and a new slot costs one enum variant.

Materials also carry `uv_tiling`/`uv_offset` (one `uv_transform` for the whole
material — an artist tiles a surface, not a map): a wall kit is unusable when
each piece shows one stretched copy of its texture.

Format 1 files load unchanged: `MaterialDef::migrate` folds the old
`base_color_texture` into the BaseColor slot on read and clears it, so nothing
downstream sees two sources of truth.

**Two things had to change underneath for any of this to be true of the render
rather than only of the data**, both found by review before this landed:

1. **The first loader of a path wins.** The asset server keys handles by path,
   so `load_with_settings` on a path something already requested returns the
   existing handle and DISCARDS the settings. The import was eagerly loading
   every texture with `ImageLoaderSettings::default()` (`is_srgb: true`,
   ClampToEdge), which meant the per-slot colour space and the repeat wrapping
   were inert: normal maps were still gamma-decoded and tiling smeared the edge
   texels instead of repeating. Imported textures are no longer preloaded — the
   material is the first loader, so the slot's settings are the ones that reach
   the GPU. Models still preload; a Gltf has no equivalent setting.
   *Known limitation:* one file used in two slots of different colour space
   still resolves to a single decoded image, whichever slot loaded first.
   Authoring the same bitmap as both a colour and a data map is unusual enough
   to defer; when it matters, the fix is a distinct `AssetPath` per colour space.
2. **Normal maps need tangents.** Bevy compiles the entire normal-mapping branch
   out unless a mesh carries `ATTRIBUTE_TANGENT` (`#ifdef VERTEX_TANGENTS`), and
   no primitive builder emits one — so a normal map on a greybox cube, on a
   sphere, or in the material preview was discarded in silence. Primitives are
   now built through `primitive_mesh`, which generates tangents (one in
   `editor_scene::materials` for editor surfaces, one in `game_framework` for
   games, because a game crate must not depend on an editor crate). glTF meshes
   already arrive with tangents from the importer.

Covered by MATERIAL, and the coverage deliberately reads the GPU-facing state —
*"the normal map is loaded LINEAR, not sRGB"* and *"the sampler REPEATS"* — over
`Assets<Image>`, because a def-level assertion cannot fail for either bug above
and would have reported the feature green while the render was wrong. Also
`format_1_textures_migrate_into_the_slot_table` and `only_colour_slots_are_srgb`.

## Status (2026-08-05)

D1–D12 all implemented with executable coverage: unit/property tests per
crate plus five session probes in `verify.sh full` + CI (PREFAB, USER 27,
KIT 10, BARREL 21 — the D12 exit flow end-to-end incl. flatten-to-entities
+ collider/gameplay config — and MATERIAL 19 — the D11 editor with
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
