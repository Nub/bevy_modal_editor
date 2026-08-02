# Spike: Rust hot code reload (M3-C8)

**Question**: can a gameplay-system change rebuild and hot-swap into a running
editor session on Bevy 0.19, preserving scene/selection/history?

## Options evaluated (2026-08-02)

| Approach | Verdict | Why |
|---|---|---|
| **Bevy built-in `hotpatching`** (subsecond, official since 0.17 — owner-supplied correction: bevy.org/news/bevy-0-17 "hot-patching systems in a running app") | ✅ EXISTS on our 0.19 pin (`bevy/hotpatching` feature, `bevy_app/hotpatch.rs`, official example `hotpatching_systems`) | Patches SYSTEM BODIES in a running app via the dioxus `dx` CLI toolchain. Limits: function-body changes only — struct/layout/schedule changes still require a restart. **Deferred by owner decision to a later milestone**; adoption path is concrete: enable `bevy/hotpatching` under the editor feature + document the `dx`-driven dev loop. |
| `dexterous_developer` | ❌ not now | Tracks specific Bevy releases; reload boundary requires restructuring systems into reloadable libraries — superseded by the built-in path above. |
| Hand-rolled dylib swap | ❌ rejected | No stable Rust ABI: any type crossing the boundary (all of Bevy) is UB on mismatch. The failure mode is silent corruption, which is worse than restarting. |

## Decision: fast-relaunch now; built-in hotpatching later (owner call)

The original write-up wrongly dismissed subsecond as unproven — Bevy has shipped
it as the official `hotpatching` feature since 0.17. It remains deferred (owner,
2026-08-02) because it needs the `dx` toolchain in the dev loop and covers only
system-body edits; fast-relaunch below covers the rest (layout changes, plain
`cargo watch` workflows) and stays valuable alongside it.

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
