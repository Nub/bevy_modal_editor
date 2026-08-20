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


### D11 library verbs and what is still owed (2026-08-19)

The library only ever grew: the registered material actions were exactly new,
assign, edit and rename, so a mis-created material was permanent and
"duplicate this and tweak it" — the most common operation in any DCC — did not
exist. `material.duplicate` (`Space Shift+D`) copies the open or selected
material, names it *"<source> copy"*, and makes the COPY current, because
duplicating is how a variant starts and every edit after it belongs to the
variant.

`material.delete` has **no binding on purpose**. It is not undoable through the
asset history, so it is reached from the palette where it must be chosen by
name rather than by muscle memory, and it REFUSES while anything still wears the
material — deleting one out from under a shaded object would leave it silently
unpainted. What it does delete is exactly the case that motivated it: the
material nothing uses.

Also fixed here: `MaterialLibrary::get_mut` bumped the generation BEFORE the
lookup succeeded, so every miss marked the library dirty and the autosave
rewrote `materials.ron` for an edit that never happened (spec §8: no per-frame
work at rest).

**Still owed — inheritance.** Every material is a full copy with no link back
(no `base` field; `MaterialRef` is a bare uuid), so a late art-direction change
still means re-editing N materials by hand. Spec §6:492-497 mandates keeping
library-reference *and* inline-override semantics, and only the reference half
exists. Inheritance is deliberately NOT built here: it wants base-plus-patches
over one shared patch type, and this repo already carries three delta languages
that disagree. Adding a fourth ad-hoc one to land inheritance a slice earlier
would move the architecture backwards. It rides the delta-language
consolidation instead.

### Patches: what landed, and what is still two languages (2026-08-19)

Spec §5 declares patches THE one delta language. `Op::Patch { target,
type_path, path, value }` now exists in the kernel: it addresses one leaf by
reflect path, and its inverse carries that leaf's PREVIOUS value, so a history
entry for a slider drag holds an `f32` rather than a whole `Transform` per
frame. Coalescing is op-agnostic and needed no change. An unresolvable path is
skipped and records nothing — the kernel is the one place that validates, so no
UI can smuggle a bad delta past it.

The inspector routes numbers and bools — the two kinds that dominate history
volume — through it. Three kinds stay component-granular ON PURPOSE, which is
the "fall back to Set when a leaf cannot be compared" case: `Name` rebuilds
through its constructor because its hash is derived, a Euler degree is one of
three fields feeding a single quaternion, and the enum cycle needs the registry
to resolve the next variant.

**Still two representations, deliberately.** `PrefabOverrides` keeps its RON
string value. The kernel patch is the IN-MEMORY undo path, and serializing a
value on every frame of a drag is exactly the per-frame work §8 forbids and the
serialize-to-compare pattern the v1 post-mortem bans. What is shared is the
ADDRESSING — a type path plus a reflect path — and the apply/revert semantics.
Unifying the value representation is not obviously correct and should not be
done for tidiness; the honest next step is to derive a prefab override from the
patch op that produced it, rather than diffing whole components afterwards,
which this op finally makes possible.

### D11 material inheritance (2026-08-20, materials format 3)

Spec §6:492-497 mandates keeping BOTH library-reference and inline-override
semantics, and only the reference half existed: every material was a full copy
with no link back, so a late art-direction change meant re-editing N materials
by hand.

A material may now name a `base` and carry the set of fields it has claimed.
Everything unclaimed resolves from the base, live and transitively, so editing a
base re-shades every instance in the same pass. `material.new-instance`
(`Space Shift+I`) makes a child that owns NOTHING — it *is* its base until a
field is edited, and editing a field claims exactly that field.
`material.detach` bakes the resolved values in and stops the following; it must
never change what the surface looks like, and there is a test that says so.

`overridden` is a closed enum rather than reflect paths. A material's fields are
known at compile time, so the compiler checks that resolution stays exhaustive
when a field is added — the concept shared with prefab overrides is "a base plus
the fields you took ownership of", which is the semantics, not the encoding.

`MaterialLibrary::resolved` is what everything that RENDERS calls — scene sync,
the panel preview, the palette chip — while `get` still returns what is STORED,
which is what an editor edits. A cycle resolves rather than hanging: refusing to
render is worse than a base chain that stops early.

Delete now also refuses while other materials inherit from the target, not just
while objects wear it — orphaning an instance would silently change its
appearance.

Format 3, because a format-2 reader would drop the wiring and flatten an
instance to its own sparse values, which is a different material.

The panel says where each value comes FROM: on a material that follows a base,
an inherited row's label is dimmed, and a claimed row carries a revert glyph that
hands the field back. Reverting goes through the same asset-history path as any
other material edit, so one Ctrl+Z puts the claim back — and it is a change of
OWNERSHIP, not an undo: the value that appears is whatever the base says now.

**Owed:** the revert affordance is a glyph rather than a hit-tested button, which
is a deliberate density choice in a sixteen-row panel but is a small target.
Worth revisiting the first time it annoys someone.

### D11 texture picking (2026-08-20)

The texture chip cycled: press it and the slot advanced to the next imported
texture, name only, wrapping round to none. That was tolerable with ONE slot and
became untenable at five — finding a particular map meant pressing a chip until
its name went past, five times over, with no way to see what you were choosing.
The slot table made this worse, so it belongs to the same work.

A chip now opens the command palette filtered to imported textures: type part of
a name, see it previewed unlit on the same sphere the material preview uses, and
press Enter. `none` is the first entry, so clearing a slot is a choice in the
same list rather than a separate gesture to remember.

The binding goes through `edit_material`, the same asset-history path a slider
takes — the first version wrote straight to the library and silently cost the
texture binding its undo, which the hands-on probe caught. It also claims the
Textures field on an inheriting material, exactly as moving a slider claims its
own field.
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
