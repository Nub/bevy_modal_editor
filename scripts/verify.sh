#!/usr/bin/env bash
# The pre-handoff gate (spec §8 + ux-flow-audit): everything the CI fast lane
# runs, PLUS the windowed UX flow probes — real binary, injected keystrokes,
# user-VISIBLE outcome asserted (headless tests cannot catch invisible-success
# bugs; the probes exist because two of those reached the owner).
#
# Usage: ./scripts/verify.sh            (inside `nix develop` or any rust env)
# Every editor build handed to a human runs this first. No exceptions.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "── fmt"
cargo fmt --all --check
echo "── clippy"
cargo clippy --workspace --all-features -- -D warnings
echo "── tests (workspace, editor on)"
cargo test --workspace --features template_game/editor
echo "── editor strips"
cargo check -p template_game --no-default-features
echo "── build (editor on)"
cargo build -p template_game --features editor

# Flow probes: each drives one real user workflow end-to-end and exits nonzero
# unless the on-screen outcome holds. Add new probes to this list as flows land.
PROBES=(
  PREFAB_PROBE
)
for probe in "${PROBES[@]}"; do
  echo "── flow probe: ${probe}"
  env "${probe}=1" cargo run -p template_game --features editor
done

echo "VERIFY PASS — safe to hand off"
