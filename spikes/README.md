# M0 — De-risking Spikes

M0 is a **hard gate** (spec §10): each spike carries a fallback decision written *before*
the spike runs. A failed spike changes the spec before Phase 1 code exists. Spikes are
timeboxed, live as throwaway crates under `spikes/` (added to the workspace as each
begins), and their findings are recorded in this file — code is disposable, conclusions
are not.

| # | Spike | Proves | Pre-written fallback |
|---|-------|--------|----------------------|
| 1 | `editqueue-scale` | `EditScope` transactions + reflection-based inverse capture stay responsive at 1000-entity scenes (spec §5, §8 perf budget); gesture coalescing works | Narrow inverse capture to typed built-in ops (patch/insert/remove) and require custom `EditOp`s to supply inverses manually |
| 2 | `cell-merge` | Two divergent edits of one level (cell-partitioned, stably-ordered text files) merge in git with no conflicts when touching different cells, and a survivable conflict when touching the same entity (spec §9 collaboration) | Smaller cells + entity-per-file layout; if still hostile, in-editor structural merge tool moves up from "later" to M4 |
| 3 | `feathers-shell` | bevy_ui + feathers/bevy_ui_widgets can carry the editor shell: docking/tiling, virtual list (10k-row hierarchy), property grid, text editing, focus/keymap integration (spec §7) | egui behind the unchanged `WidgetKit` seam; feathers gaps filed upstream; re-evaluate each Bevy release |
| 4 | `bsn-foundation` | BSN patches express prefab instance overrides; scene inheritance expresses variants; `SceneId` UUID components round-trip inside BSN payloads; versioned envelope wraps a BSN-semantic payload (spec §5 BSN-first policy) | Keep BSN *semantics* (per-field patches, inheritance refs) in our own envelope payload without the BSN runtime until it matures; ledger the divergence |

Deliverables per spike: a `FINDINGS.md` in its directory (what was tried, what
broke, verdict vs. fallback), plus updates to `docs/bsn-ledger.md` where relevant.

Exit criteria for M0 overall: all four verdicts recorded; spec amended where any
fallback was taken; `editor_api` RFC (`docs/spec/04-EDITOR-API.md`) confirmed or amended
against spike findings; initial BSN gap ledger populated.

---

## M0 CLOSEOUT — gate passed (2026-08-02)

| # | Spike | Verdict | Fallback used? |
|---|---|---|---|
| 1 | editqueue-scale | PASS (24–440× headroom) | No |
| 2 | bsn-foundation | PASS (6/6 checks, first compile) | No |
| 3 | feathers-shell | PASS (owner-confirmed, 4 rounds) | No |
| 4 | cell-merge | PASS (7/7 scenarios) | No |

All four architectural bets validated with zero fallbacks. Spec amendments from
findings: first-old-value coalescing contract (RFC §5); widget-kit requirements from
F6–F8 (spec §7); serializer format rules (spike 4); BSN ledger entries 6–7 (spike 2).
The `editor_api` RFC shapes are confirmed against executed code. **M1 begins.**
