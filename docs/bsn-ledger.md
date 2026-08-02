# BSN Gap Ledger

Tracks every place we diverge from or gap-fill BSN (spec §5 BSN-first policy). Reviewed
at every phase-boundary Bevy upgrade. Each entry: what BSN lacks, what we built instead,
and the convergence plan.

| # | Gap (as of Bevy 0.19) | Our gap-filler | Convergence plan | Status |
|---|----------------------|----------------|------------------|--------|
| 1 | Asset-driven workflow (`.bsn` files on disk) not shipped — 0.19 is code-driven (`bsn!`) only | Versioned envelope on disk with a BSN-semantic payload (per-field patches, inheritance refs, no expanded trees) | When `.bsn` assets land: envelope wraps or references `.bsn`; migration is mechanical because payload semantics already match | Open — re-check each Bevy release |
| 2 | No format versioning or migration story | Envelope `format_version` + chained migrators (spec §5) | None — this is permanent architecture wrapping any payload, including future `.bsn` | Permanent by design |
| 3 | No stable-UUID entity identity (references are structural) | `SceneId` UUID component riding inside payloads; all editor references UUID-based | Adopt upstream identity if BSN grows one; otherwise permanent | Open |
| 4 | No editing model (no undo/transactions) | `EditQueue`/`EditScope` (spec §5); EditOps express changes as BSN-style patches | None expected — editing is our domain; shared patch representation is the bridge | Permanent by design |
| 5 | Cell partitioning / streaming layout | Level-as-directory-of-cells, each cell a BSN-semantic document (spec §9) | Cells could become `.bsn` files individually once #1 closes | Open |

| 6 | No scene hot reload or retained reconciliation — `AssetEvent::Modified` ignored, `ScenePatch::resolve` is destructive one-shot (spike 2) | Editor owns re-stamping prefab instances on source change via regenerate hooks (spec §6 already assumed this) | Adopt upstream reconciliation if BSN grows it | Open |
| 7 | BSN patches are closures — inherently unserializable; and no `Vec<S>: Scene` for runtime-counted patch lists (spike 2) | Serializable `ReflectPatch` is our delta source of truth; BSN closures generated from it via the registration fn-table; lists fold into nested boxed tuples | When `.bsn` ships, the same fn-table feeds its loader; revisit if upstream adds a dynamic patch type | Open |

| 8 | (Rendering, not BSN — recorded here as the upstream-gap ledger) **Bevy 0.19 removed the render graph**: `render_graph`/`ViewNode`/`Node3d` are gone; rendering is camera-driven schedules (`Core3d` schedule + `Core3dSystems::{Prepass, MainPass, EarlyPostProcess, PostProcess}` sets; custom passes are plain systems using `CurrentView`/`RenderDevice`) | v1 bevy_outliner carried through the port gate but parked (excluded from workspace) until its ~500 lines of graph plumbing are rewritten as schedule systems; shaders + JFA algorithm + extract/prepare patterns carry as-is. SAME work awaits the future bevy_vfx port (its GPU pipeline is graph-based) | Rewrite plumbing on the schedule API (next work item); watch upstream for helper abstractions | Open |

Add entries as spikes and milestones discover them. Deleting an entry requires the
upstream feature to be adopted and the gap-filler removed.
