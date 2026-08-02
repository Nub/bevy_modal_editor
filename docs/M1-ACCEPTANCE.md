# M1 Acceptance — "I can walk around my game, and the editor is inside it"

> **CLOSED — owner-accepted 2026-08-02.** All eight executable tests green; owner
> hands-on drove seven fix rounds (font-path asset resolution, wgpu debug-profile hang,
> which-key popup redesign, typography/elevation design system) before acceptance.
> Chrome now runs on the shared `ui_style` scale per the standing design bar.

Written at milestone start (spec §8 guardrail 2). M1 is done when every test below is
green and the owner has walked the level and toggled the editor by hand — not before.

## Executable acceptance tests

| # | Test | Where |
|---|---|---|
| A1 | Keymap parser round-trips: `"ctrl+z"`, `"g g"`, `"space p p"`, `"shift+4"` parse, format back, reject garbage with useful errors | `editor_api` unit |
| A2 | Conflict detection: two actions binding the same sequence in the same context is a load error naming both actions; same sequence in different contexts is fine; a prefix that shadows a longer sequence in the same context is an error | `editor_api` unit |
| A3 | Registry validation: duplicate `ActionId`/`ModeId` across features → hard error naming both features; unknown `ContextId` in a binding → hard error | `editor_api` unit |
| A4 | Resolver: headless app + synthetic key input in mode M emits exactly one `ActionInvoked` with the right id; keys not bound in the active context emit nothing; mode switch changes what resolves | `editor_core` integration (headless) |
| A5 | Keymaps are data: default bindings load from a RON asset; a user-layer file overrides one binding; the override wins; removing the file restores defaults | `editor_core` integration |
| A6 | Editor strips: `cargo tree -p template_game --no-default-features` contains no `editor_api`/`editor_core`/`editor_ui`; the binary builds | CI (`ci.yml` extend) |
| A7 | `template_game --features editor` builds and boots headless through Boot→MainMenu→LoadingLevel→InGame (lifecycle drive test) | `template_game` test |
| A8 | Which-key data derives from the registry: for a given mode, the pending-key hint list equals the registered bindings (no hand-maintained hint tables — v1 anti-pattern) | `editor_core` unit |

## Owner hands-on checklist (closes the milestone)

- Boot `template_game`, click through menu → level, walk with WASD + mouse look.
- Toggle the editor overlay (default: `F12` dev toggle — final key TBD in keymap file):
  game input suspends, editor statusline shows current mode, `Esc` walks home to Normal.
- Open the palette (`:` / `Space`), fuzzy-find an action, invoke it.
- Hold a prefix key → which-key popup lists continuations.
- Rebind one key in the user keymap file, relaunch, confirm the new binding works and
  the cheat-sheet/palette shows it.

## Explicit non-goals for M1

Scene editing, selection, EditQueue (M2); panels beyond the shell/statusline/palette;
macros (need EditQueue semantics); gamepad; any real game content beyond a graybox
floor.
