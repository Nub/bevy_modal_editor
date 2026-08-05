#!/usr/bin/env bash
# The verification gate (spec §8 + ux-flow-audit), two speeds (owner):
#   ./scripts/verify.sh        FAST dev loop — build + the user-session probe
#   ./scripts/verify.sh full   everything: fmt, clippy, all tests, strip check,
#                              build, ALL probes (pre-handoff / CI parity)
# Every editor build handed to a human runs `full` first. No exceptions.
set -euo pipefail
cd "$(dirname "$0")/.."
LEVEL="${1:-fast}"

if [ "$LEVEL" = "full" ]; then
  echo "── fmt"
  cargo fmt --all --check
  echo "── clippy"
  cargo clippy --workspace --all-features -- -D warnings
  echo "── tests (workspace, editor on)"
  cargo test --workspace --features template_game/editor
  echo "── editor strips"
  cargo check -p template_game --no-default-features
fi
echo "── build (editor on)"
cargo build -p template_game --features editor

# Flow probes: each drives one real user workflow end-to-end and exits nonzero
# unless the on-screen outcome holds. Add new probes to this list as flows land.
if [ "$LEVEL" = "full" ]; then
  PROBES=(PREFAB_PROBE USER_PROBE)
else
  PROBES=(USER_PROBE)
fi
for probe in "${PROBES[@]}"; do
  echo "── flow probe: ${probe}"
  env "${probe}=1" cargo run -p template_game --features editor
done

echo "VERIFY PASS (${LEVEL})"
