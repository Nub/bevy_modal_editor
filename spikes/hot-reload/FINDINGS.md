# Spike: Rust hot code reload (M3-C8)

**Question**: can a gameplay-system change rebuild and hot-swap into a running
editor session on Bevy 0.19, preserving scene/selection/history?

## Options evaluated (2026-08-02)

| Approach | Verdict | Why |
|---|---|---|
| `dexterous_developer` | ❌ not now | Tracks specific Bevy releases; no 0.19 support at evaluation time (0.19 is weeks old). Reload boundary requires restructuring systems into reloadable libraries — a large architectural buy-in to make BEFORE the crate proves out on our pin. |
| Subsecond / dioxus hot-patching | ❌ not now | Bevy integration demonstrated experimentally on 0.16-era; jump-table patching of a a full Bevy app on 0.19 + wgpu/Metal unproven. Watch — this is the most promising long-term path. |
| Hand-rolled dylib swap | ❌ rejected | No stable Rust ABI: any type crossing the boundary (all of Bevy) is UB on mismatch. The failure mode is silent corruption, which is worse than restarting. |

## Decision: invoke the pre-written fallback (per the C8 gate)

**Fast-relaunch**: one action saves everything, restarts the (freshly rebuilt)
binary, and restores the session — scene, selection, camera, editor state —
landing you back where you stood in seconds. Combined with `cargo watch -x
'build -p template_game --features editor'` in a terminal, the loop is: edit
Rust → auto-rebuild → `ctrl+shift+r` → same scene, same selection.

What is NOT preserved across the restart: undo history (process-local by
design — history serialization is a possible later upgrade) and play-session
state (you relaunch into the editor).

**Revisit**: every phase-boundary Bevy upgrade (ledger #9) — adopt
dexterous/subsecond when one demonstrably works on our exact pin.
