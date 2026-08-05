#!/usr/bin/env bash
# The verification gate (spec §8 + ux-flow-audit), two speeds (owner):
#   ./scripts/verify.sh        FAST dev loop — tests for CHANGED crates only
#                              (vs HEAD) + editor build; no probes
#   ./scripts/verify.sh full   everything: fmt, clippy, all tests, strip check,
#                              build, ALL windowed probes (pre-handoff / CI)
# Every editor build handed to a human runs `full` first. No exceptions.
set -euo pipefail
cd "$(dirname "$0")/.."
LEVEL="${1:-fast}"

if [ "$LEVEL" = "full" ]; then
  echo "== fmt"
  cargo fmt --all --check
  echo "== clippy"
  cargo clippy --workspace --all-features -- -D warnings
  echo "== tests (workspace, editor on)"
  cargo test --workspace --features template_game/editor
  echo "== editor strips"
  cargo check -p template_game --no-default-features
else
  # Changed crates only (working tree vs HEAD): the dev loop tests what you
  # are touching; the full suite is for handoff/CI. VERIFY_CRATES overrides
  # detection (stripped envs without a working git).
  CRATES="${VERIFY_CRATES:-}"
  if [ -z "$CRATES" ]; then
    CRATES=$( { git diff --name-only HEAD 2>/dev/null; \
                git ls-files --others --exclude-standard 2>/dev/null; } \
      | sed -n 's|^crates/\([a-z_0-9]*\)/.*|\1|p' | sort -u || true)
  fi
  if [ -n "$CRATES" ]; then
    for crate in $CRATES; do
      echo "== tests: ${crate}"
      cargo test -p "$crate"
    done
  else
    echo "== no crate changes, skipping tests"
  fi
fi
echo "== build (editor on)"
cargo build -p template_game --features editor

# Flow probes: each drives one real user workflow end-to-end and exits nonzero
# unless the on-screen outcome holds. Full/CI only (owner: dev speed).
if [ "$LEVEL" = "full" ]; then
  for probe in PREFAB_PROBE USER_PROBE KIT_PROBE BARREL_PROBE; do
    echo "== flow probe: ${probe}"
    env "${probe}=1" cargo run -p template_game --features editor
  done
fi

echo "VERIFY PASS (${LEVEL})"
