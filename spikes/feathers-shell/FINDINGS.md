# Spike 3: feathers-shell — FINDINGS

**Verdict: PROVISIONAL PASS — programmatic claims proven; interactive feel awaits the
owner's hands-on judgment (the fallback decision is theirs).**

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

## Awaiting owner judgment (run: `cargo run -p spike_feathers_shell --release`)

- Scroll the 10k list: smoothness, no visual popping at rebind boundaries.
- Drag the splitter: responsiveness, cursor feedback.
- Tab through controls: focus rings, order sanity.
- Type/select/copy/paste in Name; type in X/Y/Z fields (watch the status bar).

If the feel is acceptable → PASS, feathers is the shell. If not → the pre-written
fallback (egui behind the `WidgetKit` seam) triggers, and this spike's gap list still
stands as the feathers upstream-contribution backlog.
