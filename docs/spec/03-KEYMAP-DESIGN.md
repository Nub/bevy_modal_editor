# Keymap Design — v2 Default Bindings

> Design deliverable required by `02-RECREATION-PROMPT.md` §4 — reviewed and resolved with
> the project owner 2026-08-01 (see final section). Ships in the new repo as
> `docs/keymap-design.md` and as the default keymap data file. Everything here is a *default*
> — all bindings are user-remappable data.

## Design principles

1. **Vim keys keep their meaning; context supplies the object.** `/` always means search —
   *what* it searches depends on focus (viewport → objects, hierarchy → tree filter,
   inspector → components). `i` always means insert, `d` delete, `y`/`p` yank/paste,
   `u` undo. A user who knows vim should be able to *guess* correctly.
2. **The selection is the text object.** Vim operators act on the object under/around the
   cursor; the editor's equivalent is the current selection. `d` deletes it, `y` yanks it,
   transform operators move it. Visual mode (`v`) grows it.
3. **Counts, registers, repeat, macros apply uniformly** because everything is an action:
   `3p` pastes three copies, `"ay` yanks into register *a*, `.` repeats the last edit,
   `q a … q` / `@a` records/replays.
4. **Industry conventions win where vim has no analogue.** RMB-held WASD flight, W/E/R
   transform tools (Maya/Unity/Unreal standard), scroll zoom. Deviations from *both* vim and
   industry require written rationale below.
5. **Chord depth ∝ rarity.** Single keys for per-minute actions, `g`-prefix and leader menus
   for per-session actions, the command palette for everything (`:` reaches all actions).

## Modifier philosophy

- **Shift** = variant/inverse of the same idea (`n`/`N`, `'x`/`'X` = ±axis view, `p`/`P`).
- **Ctrl** = window/system-level (redo, save-as, panel focus cycling).
- **Space** = leader; opens a which-key menu tree for discoverable, rarer commands.
- Numbers are **count prefixes only** — never bare commands (v1 used 1–9 for views/marks;
  those move to `'` marks, freeing counts).

---

## Global (all contexts)

| Key | Action | Vim analogue |
|---|---|---|
| `Esc` | Cancel gesture → clear pending count/register → walk home to Normal | `Esc` |
| `u` / `Ctrl-r` | Undo / redo | same |
| `.` | Repeat last edit action (with count) | same |
| `q{a-z}` … `q` / `@{a-z}` / `@@` | Record / replay / replay-last macro — **deferred** until actions are parameterized (gestures-as-data; see spec §macros); `q`/`@` stay reserved | same |
| `"{a-z}` | Register prefix for next yank/paste | same |
| `:` | Command palette (all actions, fuzzy; ex-style commands work: `:w` save, `:q` quit, `:e {scene}` open, `:w {name}` save-as) | ex commands |
| `/` | **Contextual search** (see per-context tables) | search |
| `n` / `N` | Next / previous search result | same |
| `?` | Cheat sheet / keymap browser for current context | (help) |
| `Space` | Leader menu (which-key): `Space f` find, `Space p` play controls, `Space t` toggles (grid, gizmos, physics debug, shading), `Space w` window/panel ops, `Space l` lock/unlock the selection, `Space h` / `Space H` hide / isolate the selection, `Space u` unhide all, `Space b` light the prefab (while editing one), `Space x` array & mirror (`x/y/z` array along that world axis, shifted mirrors across it) | leader |
| `m{a-z}` / `'{a-z}` | Set / jump camera mark (marks store position + orientation) | marks |
| `''` | Jump back to previous camera position (auto-stack) | `''` |
| `'x` `'y` `'z` / `'X` `'Y` `'Z` | Ortho view down ±axis (orthographic toggle included) | mark-like |
| `Ctrl-h/j/k/l` | Focus panel left/down/up/right (viewport ↔ hierarchy ↔ inspector …) | window nav |
| `F5` / `F6` / `F7` | Play / Pause / Reset (also `Space p p`, `Space p r`) | — |

**Rationale — marks over numbers**: v1 bound ortho views and camera marks to 1–9, which
kills count prefixes. Vim's own mark grammar is strictly better: 26 named slots, `''` for
jump-back, and `'x/'y/'z` reads as "go to the X view."

---

## Normal (viewport) — navigate & select

| Key | Action | Rationale |
|---|---|---|
| `h` `j` `k` `l` | Selection motion: `h`/`l` prev/next sibling, `k` parent, `j` first child | Spatial-ish tree walking without leaving the viewport; matches hierarchy keys |
| `v` | Visual mode: subsequent clicks/motions extend selection | visual mode |
| `V` | Box-select (drag) | visual-block flavor |
| `*` (physically `shift+8`) | Select all instances of the selected thing (same prefab / same kind). `KeyCode` is a PHYSICAL key, so there is no `*` token to bind — the same convention as `shift+semicolon` for `:`; the chrome renders both back | vim `*` = "find this word everywhere" |
| `gv` | Reselect previous selection | same |
| `gp` | Select parent group/prefab root | — |
| `gd` | Go to definition: open source prefab / asset of selection | vim `gd` |
| `gi` | Jump to last insert location and re-enter Insert | vim `gi` |
| `gg` / `G` | Select first / last root entity (hierarchy order) | same |
| `zz` | Frame selection (center view) | vim `zz` centers |
| `zf` | Frame whole scene | fold-ish mnemonic: "frame" |
| `i` | **Insert mode**: place new entities (palette picks what) | insert |
| `a` | Insert as *child of selection* ("append into") | vim `a` appends after |
| `d` / `x` | Delete selection (to register) | same |
| `Shift+D` | **Duplicate** selection and grab it (implemented 2026-08-19) | Blender; `d` is taken by delete, and duplicate-then-place is one motion in every DCC |
| `y` / `p` / `P` | Yank / paste at cursor raycast / paste in place | same |
| `o` | New sibling after selection (empty group / repeat last kind) | open line |
| `cw` | Rename selection | change word |
| `cr` | Change-replace: swap selected instance's prefab/kind, keep transform + overrides | `c` change namespace + `r` replace |
| `w` / `e` / `r` | Transform gestures: move / rotate / scale — see below | Maya/Unity/Unreal/Godot W/E/R, adopted exactly; entity-replace yields `r` to scale because scale is frequent and replace is rare |
| `Enter` | **Descend** into selection: group → group scope, prefab → isolated prefab edit (own undo scope), feature entity → its sub-editor (spline → control points). `Esc` ascends one level | the fractal rule: Enter goes inside, Esc comes out |
| `Tab` | Cycle gizmo tool shown on selection (move → rotate → scale) | — |
| RMB-hold + `WASDQE` + mouse | Fly camera (industry standard); scroll = speed | — |
| MMB / `Alt`+LMB | Orbit selection; scroll = dolly/zoom | industry |
| `/` | Find object palette (fuzzy over names/kinds/tags); Enter selects, `n`/`N` walk matches | search |

**Transform gestures (Blender-style modal, vim-friendly):** pressing `w`/`e`/`r` starts an
immediate modal gesture on the selection: mouse moves it; `x`/`y`/`z` constrain to axis
(double-tap = plane, i.e. exclude axis — Blender `Shift+X`); typed digits set an exact
amount (`w x 2.5 Enter` = move +2.5 on X); `Enter`/click commits, `Esc` cancels and restores.
One undo entry per gesture. Counts compose: `3.` repeats a nudge three times.

**Box select (implemented 2026-08-20, spec §9 layout throughput):** dragging in
the viewport selects everything the box covers; shift adds to the selection
instead of replacing it. Under five pixels of travel it is still a click, so a
wobble while clicking cannot turn into a selection gesture.

This moved the selection decision from the PRESS to the RELEASE. Selecting on
press made a box impossible to start anywhere the ground plane covers — which at
blockout scale is most of the viewport — because the press was consumed as a
click on the floor. Now a press only ARMS: the release decides whether it was a
click on what was under the cursor or a box that happened to start over it. A
plain click behaves exactly as before, and the probe asserts that specifically.

Entities are tested by their ORIGIN, not their bounds: a world AABB is not
available for every derived gltf subtree, and "the thing is in the box" reads
the same either way at blockout scale. Sealed containers resolve as a unit here
exactly as they do for a click, so a box over half a prefab takes the prefab.

**Duplicate (implemented 2026-08-19):** `Shift+D` copies every registered
component of the selection — the v1 post-mortem records ITS duplicate as lossy
(kind, position and rotation only), so this one goes through the same reflection
capture as yank and delete — spawns the copies as ONE transaction, selects them,
and hands them straight to a move gesture. The copies land exactly on their
originals, so without that grab a duplicate looks like nothing happened; `Esc`
cancels the move and leaves them in place, since the duplicate is its own
transaction. It deliberately does NOT touch the yank register: a designer yanks
a piece, lays a run of duplicates, and still expects `p` to paste the yank.





**Tab is bevy's key too (2026-08-20, owner testing).** `Tab` arms the next
socket, and `TabNavigationPlugin` independently moves UI focus to the first
focusable widget it can find — a text field in a panel. `KeyCapture` follows
focus, so cycling a socket silently handed the keyboard to an inspector box and
the NEXT key went there: the owner's report was "`i` inside socket mode doesn't
work". Arming a socket now takes the keyboard back. Working in the viewport
means the viewport has the keys.

Two related rules came out of the same hunt:

- **A hidden field cannot capture the keyboard.** Capture is derived from focus
  landing on an editable text widget; a closed palette hides its root but keeps
  its input entity, so focus parked there made the resolver stand down forever.
  The rule now requires the field to be VISIBLE.
- **Overlay contexts can LAYER instead of grabbing.** A gesture overlay is
  exclusive on purpose — a stray `u` mid-drag must not undo. A working layer
  like socket mode is not: it wins the keys it declares (`tab`, `i`, `o`, `esc`)
  and lets everything else fall through to the mode, so arming a socket does not
  also take away move, undo and the palette.
**Push and pull, and drop to surface (implemented 2026-08-20, owner testing).**
A free move drag moves in the CAMERA PLANE — right and up relative to the view —
which is two of the three dimensions and no way to say "further away". The WHEEL
supplies the third during a move: it pushes the object along the view axis, and
the camera stands down for the duration, so the same gesture that slides a crate
across the screen can also send it to the far wall. It feeds the same motion
channel the cursor does, so grid snap, axis constraints, the coalesced
transaction and the single undo entry all apply unchanged.

`Space d` **drops the selection onto whatever is beneath it** — the floor, a
table, the piece below — instead of leaving it clipping through. Resting is
computed from bounds rather than triangles: for blockout that is the same answer
nearly always, it is instant and deterministic, and true surface/vertex/edge
snapping is the freeform paradigm's own slice rather than something to fake
here. A support ABOVE the object's base still counts, up to half a metre,
because the common case is a prop that has sunk INTO a floor and needs lifting
out; anything higher is a wall you are standing beside, not a floor you are on.
**The wheel zooms (implemented 2026-08-20, owner testing).** Nothing handled the
scroll wheel at all: getting closer to a piece meant holding the right button and
flying there. A perspective view DOLLIES along its forward axis rather than
narrowing the field of view, because fov-zoom warps the perspective and makes the
wall you are lining up look like a different shape; an orthographic view scales
instead, since it has no distance to give. Over a panel the wheel belongs to that
panel's scrollbar. Kernel-owned locomotion like fly-nav, not a bindable action.
**Gizmos while playing (implemented 2026-08-20, spec §7):** `Space t v` keeps
feature gizmos on screen after the editor hands the world to the game. Gizmos
are furniture and normally vanish on play — but a widget with no geometry IS
the object, and a trigger volume you cannot see while walking into it makes
"nothing happened" impossible to diagnose. Same shape as `Space t p` for
collider wireframes: a development view, not a mode.
**Angle snap (implemented 2026-08-19, spec §9 grid/angle toggles):** `Space a`
toggles quantization of a rotate DRAG to `viewport.angle_step` (15° by default —
it divides 30, 45 and 90, the turns a level is actually built from). It is a
separate toggle from grid snap because the two are wanted separately: laying a
run on the grid while dialling a free angle, and the reverse. Typed angles are
exempt for the same reason typed distances are — `e 37 ⏎` means 37. Both toggles
show in the status line while they are on.

**Scale specifics (implemented 2026-08-19):** a typed amount is the **factor itself**, not a
delta — `r 2 Enter` is exactly twice as big — so the gesture's accumulator starts at 1×
rather than 0. Unconstrained scale is uniform; an axis constraint makes it **one axis only**
(`r x 4 Enter` turns a cube into a wall), which is how greybox blocks get their proportions
without opening the inspector. The factor is floored just above zero: a zero scale makes
degenerate colliders and NaN normals, and a mirror is not something a drag should be able to
produce by accident — negative/mirror scale, if wanted, needs its own deliberate verb.

**Mirror specifics.** That deliberate verb now exists, and it does NOT use a
negative scale. `Space x ⇧x/⇧y/⇧z` reflects the selection's PLACEMENT across
the plane through its own centre and CONJUGATES its orientation (`R·M·R`, which
is proper for every rotation), so scale is never touched and winding, lighting
and physics are never flipped. It is exact for anything symmetric about the
plane's direction and does not claim to flip chirality — the feedback says so
every time.

**Multi-select pivot:** rotate/scale gestures pivot on the **selection median** by default
(DCC convention); `,` mid-gesture — or `Space t p` globally — toggles individual-origins
(each entity transforms in place, e.g. spinning 50 trees). Current pivot mode is always
visible in the status line.
**Rationale**: this merges vim's verb-grammar ("operator, then refinement, then commit")
with Blender's proven modal manipulation, and it's exactly the gizmo state machine the spec
mandates (Hover → Drag → Commit/Cancel).

**Snap & placement**: during any gesture or insert preview — `s` cycles snap sub-mode
(Surface/Center/Aligned/Vertex), `Alt` holds edge-snap with guides, `Ctrl` holds grid snap.

---

## Insert mode — place entities

Entered via `i`/`a`/`o`/`gi`. Ghost preview follows the surface raycast.

| Key | Action |
|---|---|
| `i` or `/` | Reopen picker palette (change what's being placed) |
| LMB | Place; **Shift+LMB place-and-continue** (v1 behavior, kept) |
| `Enter` | Place at current preview and stay in Insert |
| `[` / `]` | Rotate preview 90° CCW/CW around surface normal (count-able: `3]`) |
| `s` / scroll | Cycle snap sub-mode |
| `Esc` | Back to Normal |

---

## Hierarchy panel (focus context)

| Key | Action | Vim analogue |
|---|---|---|
| `j` / `k` | Down / up | same |
| `h` / `l` | Collapse / expand (or jump to parent when leaf) | tree-plugin convention |
| `gg` / `G` / `{count}G` | Top / bottom / go to line | same |
| `zo` `zc` `za` / `zR` `zM` | Open/close/toggle fold; open/close all | folds |
| `/` | Filter tree (live); `n`/`N` walk matches | search |
| `Enter` | Select in viewport; `zz` then frames it | — |
| `dd` / `yy` / `p` / `P` | Delete / yank row's entity / paste as sibling / paste as child | line ops |
| `o` / `O` | New sibling below / above | open line |
| `i` | Insert child under cursor row | contextual insert |
| `cw` | Rename | change word |
| `>` / `<` | Reparent: indent into previous sibling / outdent to grandparent | indent |
| `J` / `K` | Move entity down/up among siblings | (visual move) |
| `v` | Visual row-range selection | same |

---

## Inspector panel (focus context)

| Key | Action | Vim analogue |
|---|---|---|
| `j` / `k` | Next / previous field or component header | same |
| `h` / `l` | Collapse / expand component section | tree convention |
| `i` or `Enter` | Edit focused field (Esc/Enter leaves field edit) | insert |
| `/` | Search fields & components of this entity | search |
| `a` | Add component (palette) | append |
| `dd` | Remove focused component | delete line |
| `yy` / `p` | Copy component values / paste onto matching component | line ops |
| `gd` | Go to definition (open component's docs/source info) | same |
| `J` / `K` on numeric field | Nudge value down/up (count-able, `Ctrl` = fine step) | — |
| `gr` | Revert focused field's prefab override; `ga` apply override to prefab | — |

---

## Feature-crate contexts (pattern, not exhaustive)

Feature crates register their own layers through `editor_api`, following the same grammar.
Example — spline editing (entered by `Enter` on a selected spline, "go into it"):
`h`/`l` prev/next control point, `i`/`a` insert point before/after, `d` delete point,
`w` move point (same modal gesture), `Tab` cycle spline type, `s` toggle closed,
`Esc` back out to object level. Blockout, VFX, scatter follow suit. The which-key popup
and `?` cheat sheet make each layer self-documenting; CI rejects a feature keymap that
conflicts with a reserved global key.

---

## Reserved / deliberately unbound

- Bare number keys (count prefixes only).
- `f`/`F`/`;`/`,` — reserved for a future "hop to entity by label" motion (vim `f` char-hop;
  likely an avy/leap-style overlay). Don't spend them.
- `c` beyond `cw`/`cr` — reserved as the change-operator namespace.
- `Z` — reserved (`ZZ` save-and-quit is tempting; decide later).

## Resolved decisions (owner review, 2026-08-01)

Formerly the open questions; all four settled:

1. **Transform keys: exact W/E/R** (move/rotate/scale), matching every major DCC.
   Entity-replace moved to `cr` in the change namespace — frequency beats mnemonic purity.
2. **Viewport `hjkl` = selection-tree motion**, not camera. Camera control is already rich
   (RMB-fly, orbit, marks, `zz`); mouseless selection is the flagship keyboard-first win.
   Flagged for playtest validation — if it doesn't earn its keys in practice, revisit.
3. **Multi-select pivot: median by default**, individual-origins behind `,` (mid-gesture)
   / `Space t p` (global), pivot mode shown in the status line.
4. **The fractal descend rule is universal**: `Enter` goes inside whatever is selected
   (group scope, prefab isolation edit, feature sub-editors); `Esc` ascends one level, and
   only walks mode/scope levels it entered — it never discards uncommitted work without
   the gesture-cancel semantics defined above. Prefab edit mode needs no dedicated key.
