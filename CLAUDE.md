# CLAUDE.md — v2 branch

This is the **v2 rewrite branch** of bevy_modal_editor: a from-scratch, studio-grade
modal level editor + game framework for Bevy. It will replace `main` when finished.
v1 lives on `main` and is **quarantined** — see the policy below before touching it.

## The four founding documents (read before designing anything)

- `docs/spec/02-RECREATION-PROMPT.md` — **the spec.** Vision, Definition of Done,
  architecture, studio systems, engineering standards, milestones. It wins every
  argument; if reality disagrees, amend the spec in the same PR.
- `docs/spec/04-EDITOR-API.md` — the `editor_api` contract RFC (with resolved decisions).
- `docs/spec/03-KEYMAP-DESIGN.md` — the keymap design (resolved).
- `docs/spec/01-REVIEW.md` — the v1 post-mortem: the failure catalog is a banned-pattern
  list, not history.

Also: `docs/bsn-ledger.md` (BSN gaps + convergence plans), `spikes/README.md` (M0).

## Hard rules (spec §8 guardrails + §11 quarantine — enforced, not advisory)

- **Spec-first**: every change names the spec section it implements; behavior changes
  amend the spec in the same PR. Nothing lands "while we're at it."
- **No side door**: all scene mutations flow through `EditScope` transactions; actions
  are data invoked via `ActionInvoked` events; no `ButtonInput<KeyCode>` outside the
  input resolver; no `&mut World` in UI systems; no serialize-to-compare change
  detection.
- **V1 quarantine**: never copy code, names, or file layouts from `main`. v1 may be
  consulted only for interaction behavior and the explicit keep-list (spec §11);
  keep-list code enters only via port-gate PRs. If you need v1 to answer an
  architecture question, you found a spec gap — file it instead.
- **BSN-first** (spec §5): adopt BSN semantics/implementation wherever possible; any
  divergence gets a `docs/bsn-ledger.md` entry with a convergence plan.
- **UI**: bevy_ui + feathers via the `WidgetKit` trait. No egui.
- **Bevy version**: exact pin, currently 0.19; upgrades only as dedicated
  phase-boundary PRs.

## Workspace (grows per spec §2)

- `crates/editor_api` — the feature contract (semver-stable ecosystem surface)
- `crates/editor_core` — the kernel: input resolver, modes, EditScope/undo
- `crates/editor_scene` — versioned atomic scene I/O, play/pause/reset
- `crates/editor_ui` — ALL editor chrome (palette, statusbar, which-key, style,
  embedded fonts); games add `EditorUiPlugin` + register content, never own chrome
- `crates/game_framework` — opinionated game patterns; never depends on editor crates
- `crates/template_game` — the reference game; every milestone demos here.
  `--features editor` is the editor opt-in (compile-time flag, never release opt.)
- `spikes/` — M0 de-risking spikes (throwaway code, durable findings)

## Build

Nix (`nix develop`, cross-platform incl. macOS) or plain rustup (`rust-toolchain.toml`).
`cargo run -p template_game` boots the reference game. CI: fast lane per PR, full
matrix nightly (spec §8).

## Current milestone

**M3 — "I can make it mine, and hand it to a friend"** (spec §10): reflection
inspector, hierarchy panel, material assets + library, data-driven gameplay
components, Rust hot code reload, minimal publish (`editor publish` → runnable zip).
M0 (spikes), M1 (kernel: modes/resolver/keymap/palette), and M2 (graybox loop:
EditScope/undo, scene I/O, selection/gestures/insert, play/reset —
`docs/M2-ACCEPTANCE.md` PASSED 2026-08-02) are complete.
