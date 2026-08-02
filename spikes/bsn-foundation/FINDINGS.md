# Spike 2: bsn-foundation — FINDINGS

**Verdict: PASS. The BSN-first policy's load-bearing claims all hold on Bevy 0.19; the
fallback (BSN semantics without the BSN runtime) is NOT needed.** All six checks green,
first compile, against the real API.

| Claim | Check | Result |
|---|---|---|
| Prefab instance overrides = BSN per-field patches | spawn `(barrel(), Health::patch(max=200))` → `current` kept, `max` overridden | PASS |
| Prefab variants = inheritance-by-inclusion | base → variant → instance chain, last-write-wins per field | PASS |
| `SceneId` UUIDs ride BSN | `template_value(SceneId(uuid))` round-trips through spawn | PASS |
| Envelope wraps BSN-semantic payload | RON `{format_version, entities: [{id, prefab, overrides}]}` → registries → BSN spawn | PASS (D1–D3) |

## Key API facts (Bevy 0.19, verified in source + executed)

1. **Runtime construction is first-class.** `bsn!` is sugar over public types. The
   editor's paths: `Component::patch(|template, ctx| ...)` (`PatchFromTemplate`),
   `template_value(value)`, `SceneFunction(|ctx, scene| ...)` (fully general, includes
   the type-erased `get_or_insert_erased_template(TypeId, fn() -> Box<dyn ErasedComponentTemplate>)`),
   and hand-built `ResolvedScene`/`ResolvedSceneRoot`.
2. **BSN has ZERO serialization** — patches are closures; `.bsn` on disk is explicitly
   left to the community with a documented loader hook (`ScenePatch::load_with`).
   Consequence for "one delta language" (spec §5): **our serializable `ReflectPatch` is
   the source of truth; BSN patch closures are *generated from it* at load/stamp time.**
   The spike's D-path is exactly this pattern and it works.
3. **Registries are fn-pointer tables** — prefab name → `fn() -> Box<dyn Scene>`, and
   (component, field) → patch-applier. BSN's erased API also wants plain `fn` pointers,
   so `editor_api`'s one-call component registration can generate the whole table (this
   is also precisely what a future `.bsn` asset loader needs — we build it once, both
   consumers use it).
4. **Inheritance = inclusion + document order, last-write-wins per field.** No
   inheritance keyword; multiple includes compose as tuples; diamond composition is safe
   (one template instance per `TypeId`).
5. **Component bounds**: `Default + Clone (+ Unpin)` gets the blanket `FromTemplate` —
   or derive `FromTemplate`, **never both** (deliberate specialization hack). Editor
   components should standardize on `Default + Clone` and reach for the derive only
   when a field needs spawn context (`Handle<T>`, `Entity`).
6. **No `Vec<S>: Scene` for one entity** — a runtime-counted patch list folds into
   nested boxed tuples (`fold(base, |acc, p| Box::new((acc, p)))`); works fine.
7. **No hot reload / retained reconciliation**: `AssetEvent::Modified` is ignored;
   `ScenePatch::resolve` is destructive (one-shot). The editor owns re-stamping prefab
   instances on source change — which the spec already assumed (stamping through
   regenerate hooks). Ledger entry added.
8. Legacy reflect+serde scene system survives as `bevy_world_serialization`
   (`DynamicWorld`, `.scn.ron`) — available as a serialization utility, not needed for
   our envelope.

## Consequences fed forward

- RFC §5/§6: `ReflectPatch` stays our type; add "compiles to a BSN patch closure via
  the registration-generated applier table" to its contract. No RFC shape changes.
- Prefab stamping (spec §6) = exactly the spike's D-path: envelope record → prefab fn +
  override patches → `spawn_scene`. Variants chain via inclusion before instance
  overrides.
- `docs/bsn-ledger.md` updated: entry #1 confirmed-with-nuance (closures unserializable),
  new entries #6 (no hot-reload/retained reconciliation) and #7 (no Vec<Scene>).
