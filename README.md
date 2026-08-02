# bevy_modal_editor v2

A studio-grade, keyboard-driven modal level editor and game framework for
[Bevy](https://bevy.org) — "neovim for level editing," shipped as a plugin *inside your
game's binary* behind a compile-time `editor` feature flag.

> **This is the v2 rewrite branch.** It starts from an empty tree by design and will
> replace `main` when finished. v1 remains on `main` as reference material only — see
> the quarantine policy in `docs/spec/02-RECREATION-PROMPT.md` §11.

## What this will be

- **Editor-in-game**: no standalone editor executable; the editor runs wherever your
  game runs, and strips to zero bytes with the feature off.
- **Keyboard-first modal UX**: vim-grammar actions, data-driven remappable keymaps,
  macros, a fuzzy palette for everything (`docs/spec/03-KEYMAP-DESIGN.md`).
- **A real pipeline**: raw assets → validated/processed/cooked → prefabs with
  per-field overrides and baking → assisted level layout (grid, true-shape snap,
  landscape, procedural, freeform) → one-command publish to playtesters.
- **An opinionated game framework**: lifecycle states, level loading, settings, input,
  save games, lightyear-based session flow — patterns provided, not reinvented per game.
- **Ecosystem-native**: BSN-first scenes, bevy_ui + feathers UI, and a small
  semver-stable `editor_api` so *any* crate can provide editor features seamlessly
  (`docs/spec/04-EDITOR-API.md`).

## Status

**Milestone 0 — de-risking spikes** (`spikes/README.md`). Nothing here is usable yet;
the founding documents in `docs/spec/` are the authority on where this is going.

## Building

```sh
nix develop            # or: use rustup via rust-toolchain.toml
cargo run -p template_game
cargo run -p template_game --features editor   # editor opt-in (wired in M1)
```

## License

Dual-licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option, matching Bevy. Unless you explicitly state
otherwise, any contribution intentionally submitted for inclusion in the work by you,
as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
