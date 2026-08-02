# Spike 4: cell-merge — FINDINGS

**Verdict: PASS. The merge-first collaboration design (spec §9) holds with the tested
format rules; the fallback (entity-per-file / structural merge tool in M4) is NOT
needed.**

Setup: git repo, level = directory of cell files, 3 cells × 20 entities; entity blocks
sorted by UUID, one field per line, blank line between blocks. Scenarios via real
branch/merge (`run.sh`).

| Scenario | expected | got |
|---|---|---|
| S1 different cells edited on two branches | clean | clean ✓ |
| S2 same cell, different entities | clean | clean ✓ |
| S2b same cell, **adjacent** entities | clean | clean ✓ |
| S3 same cell, both branches add (different sort positions) | clean | clean ✓ |
| S3b same cell, both add at the **same** sort position | conflict | conflict ✓ |
| S4 same entity edited on both branches | conflict | conflict ✓ |
| S5 delete vs edit of the same entity | conflict | conflict ✓ |

Conflict quality (manually verified): markers confine to the single changed line inside
the entity block, with the entity's UUID visible directly above — human-resolvable at a
glance and trivially machine-parseable for a future in-editor conflict UI.

## Format rules that made this work (bind these in the real serializer)

1. **One entity block per paragraph** — blank line between blocks gives git's diff
   algorithm clean anchor points; adjacent-entity edits merged cleanly because of it.
2. **One field per line, stable field order** — conflicts collapse to the actual
   changed field, not the whole component.
3. **Blocks sorted by UUID** — concurrent additions land at different positions and
   merge cleanly unless UUIDs happen to sort adjacent (S3b: rare, and the conflict is
   an append-position ambiguity a human resolves in seconds).
4. **UUID on the block's first line** — makes every conflict self-identifying.

## Notes / residual risks

- S3b (same-sort-position concurrent adds) is the only "false" conflict class; with
  random v4 UUIDs at realistic cell sizes it is rare. Acceptable; revisit only if it
  shows up in practice.
- Git's rename detection was not tested (cells don't rename; entities move between
  cells as delete+add — that's S5-shaped if concurrently edited, which is correct).
- The spike used a RON-ish placeholder syntax. The real format is the versioned
  envelope with BSN-semantic payload (spec §5); these four format rules are
  serializer requirements, not syntax choices, and transfer to whatever the payload
  looks like — recorded as such for the BSN spike/ledger.
