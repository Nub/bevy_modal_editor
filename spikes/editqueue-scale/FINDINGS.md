# Spike 1: editqueue-scale — FINDINGS

**Verdict: PASS. The RFC's `EditScope` design holds; the pre-written fallback (typed-ops-
only) is NOT needed.** Reflection-based inverse capture is not the bottleneck anywhere.

Run: Bevy 0.19.0, release profile, Apple Silicon macOS (dev machine), 1000-entity world
(`Transform` + custom reflected `Health`), 100 iterations per scenario.

| Scenario | mean | p99 | budget | result |
|---|---|---|---|---|
| Single reflection `Set` (inverse captured via `to_dynamic`) | <1µs | 6µs | 500µs | PASS ×83 |
| 1000-entity transaction (1000 captures + 1000 applies) | 243µs | 342µs | 8300µs | PASS ×24 |
| Drag frame (250 entities, coalesced Translate) | 4µs | 9µs | 4000µs | PASS ×440 |
| Gesture coalescing (60 frames → history entries) | 1 entry | — | 1 | PASS |
| Undo coalesced drag (restore exactness) | 5µs, max err 3.4e-4 | — | <1e-3 | PASS |
| Undo 1000-entity transaction | 148µs | — | — | — |

## What this proves

1. **Reflection inverse capture is essentially free** at editor scales: clone-old +
   apply-new through `ReflectComponent` costs ~0.25µs/entity. The RFC's "capture
   inverses generically, no manual inverse code for component ops" stands.
2. **Whole-scene transactions fit comfortably in-frame** (342µs p99 vs 8.3ms budget) —
   select-all edits, paste, and prefab propagation can be single transactions.
3. **Coalescing works as specced**: 60-frame drag = one history entry, applied at
   9µs/frame p99.

## Design findings (feed into RFC/implementation)

- **F1 — Gesture inverses should restore captured originals, not accumulated deltas.**
  Delta-accumulation undo showed max restore error 3.4e-4 (f32 accumulation, growing
  with coordinate magnitude and gesture length). Within tolerance here, but the correct
  pattern — already implied by the spec's gizmo state machine (`Dragging {
  original_transforms }`) — is: a coalescing gesture keeps the *first* captured
  old-value as its inverse ("first-old-value semantics"). Applies to `EditOp::coalesce`
  guidance in RFC §5: coalesce accumulates the *forward* op but never touches the
  original inverse. Restore then is exact by construction.
- **F2 — Bevy 0.19 API note**: `ReflectComponent::apply_or_insert` is now
  `apply_or_insert_mapped(entity, value, registry, &mut (), RelationshipHookMode::Run)`
  — `()` is the identity `EntityMapper`. Fine for editor use; relationship hooks run.
- **F3 — History memory**: full-scene transactions capture 1000 boxed dynamic values
  each (~100 runs left 199 entries without issue). Still worth a per-entry byte budget +
  history cap in the real implementation; not a blocker.

## Not covered here (deliberately)

Spawn/despawn inverses (need full entity capture — related to BSN spike), multi-user
merge of history, and `Edited`-event fanout cost. None block the RFC shapes.
