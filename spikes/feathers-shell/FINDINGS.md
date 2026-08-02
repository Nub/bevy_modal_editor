# Spike 3: feathers-shell — FINDINGS

**Verdict: PASS (owner-confirmed 2026-08-02 after four interactive rounds). feathers is
the editor UI stack; the egui fallback is NOT needed.** Programmatic claims proven and
interactive feel accepted; footguns F6-F8 found during owner testing are fixed in-spike
and recorded as widget-kit requirements.

Built a runnable mini editor shell on bevy_ui + feathers (Bevy 0.19): hierarchy pane
with a 10,000-row virtualized list, hand-rolled draggable splitter, inspector pane
(axis-tinted number inputs, slider, checkbox, parley text input), themed, tab-focusable,
status bar proving widget→state value flow.

## Proven programmatically

| Claim | Evidence |
|---|---|
| Shell constructs and boots on feathers | clean startup, zero warnings/panics |
| 10k-row list virtualizes by hand | **86 UI node entities total**, constant while running (48 slot rows + spacers + chrome); slots re-bind on `ScrollPosition` change |
| Property grid from feathers controls | `FeathersNumberInput` (with X/Y/Z axis sigil tints — purpose-built for inspectors), `FeathersSlider`, `FeathersCheckbox`, `FeathersTextInput` all compose in `bsn!` |
| Splitter feasible on primitives | 30 lines: `Pointer<Drag>` observer + `EntityCursor` + theme token |
| Value flow | `ValueChange<T>`/`TextEditChange` observers → resource → status bar |

## Upstream gaps confirmed (source-verified, not blockers — ours to build)

1. **No virtualization anywhere** in feathers/bevy_ui_widgets — `FeathersListView`
   eagerly spawns all rows, and listbox keyboard nav walks all descendants per event.
   Hand-windowing works (this spike); the editor's hierarchy/asset lists implement
   virtualization + their own selection/active-row model in `editor_ui`, not on
   `ListBox`.
2. **No docking/splitters/tab-bars** — definitively absent. The tiling layout manager
   (spec §7) is ours on flexbox + `Pointer<Drag>`; this spike's splitter is the seed.
3. **Text editing is mature** (parley: selection, word-ops, clipboard, IME,
   `SelectAllOnFocus`, char filters) **but has no undo** and no placeholder text —
   field-level undo will need wiring into our EditQueue at the widget boundary.
4. Feathers is BSN-native (`SceneComponent` + props) — composing it from runtime data
   uses the same `Box<dyn SceneList>` splicing the BSN spike validated.
5. API drift notes: markers used in `bsn!` need `Default + Clone`; `EntityCursor` lives
   at `bevy::feathers::cursor`; pane token is `PANE_BODY_BG`.
6. **Layout footgun (hit during owner testing, then fixed): inline labels beside
   `EditableText`-based inputs overflow.** The inputs have large intrinsic min-widths
   (parley measure + flexbox `min_width: auto`), so a `[label | input | input | input]`
   row spills over its label and can overlap neighboring rows — overlapped widgets then
   eat each other's pointer events (this masqueraded as "slider doesn't work"). The
   feathers idiom (per the gallery) is label-above-controls plus `max_width` caps on
   number inputs. **Widget-kit consequence: the `editor_ui` property-row widget must
   encode this layout internally** so panel authors can never reproduce the bug — the
   exact "coherent by construction" argument from spec §7.
7. **Two more owner-testing footguns, same lesson (fixed):** (a) an *empty*
   `EditableText` without `visible_width` measures 0×0 — invisible and unclickable
   ("the Name field is just a label"); number inputs only escape by being seeded with
   "0.00". (b) `FeathersSlider`'s scene sets `flex_grow: 1.0` for row layouts — placed
   in a column with free space it grows *height* ("comically tall"); needs
   `flex_grow: 0.0` in column contexts. Both are context-dependent defaults a panel
   author must know — more evidence the widget kit wraps these controls with
   editor-correct defaults rather than exposing them raw.
8. **Owner-testing round 3 (all fixed in-spike; each is widget-kit backlog):**
   number inputs start EMPTY and empty text emits no `ValueChange` on blur (upstream
   `emit_value_change` early-returns) — seeded via `UpdateNumberInput` + a blur-reset
   observer; text inputs only focus on glyph hits, not the whole box — container-level
   focus-forwarding observer added; input frames need background-based styling (owner
   preference: borders read as buttons) — done via the custom theme-token API, which
   works exactly as documented; `FeathersScrollbar` composes over the hand-virtualized
   scroll area (spacers give it correct content height). Recurring theme across F6–F8:
   feathers controls are row-oriented and empty-state-hostile out of the box; the
   `editor_ui` widget kit owns fixing that once, centrally.

## Owner judgment (closed)

Four interactive rounds (scroll feel, splitter, focus traversal, text/number editing);
each round's issues fixed in-spike and recorded (F6–F8). Final verdict: **PASS** —
feathers is the shell; the F-list is the widget-kit requirements backlog plus feathers
upstream-contribution candidates.
