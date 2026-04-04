#!/usr/bin/env bash
# Run all subsecond test harnesses.
#
# Unit tests run automatically. Manual end-to-end tests require a running `dx serve` instance
# and are documented separately — this script covers what can be automated.
#
# Usage:
#   ./run-tests.sh             # build + unit-test everything
#   ./run-tests.sh --check     # cargo check only (faster)
#   DX=path/to/dx ./run-tests.sh --e2e   # also run manual harness instructions

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
DX="${DX:-cargo run --package dioxus-cli --}"

# ─── Colour helpers ──────────────────────────────────────────────────────────
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BOLD='\033[1m'
NC='\033[0m'

pass() { echo -e "${GREEN}✓${NC} $*"; }
warn() { echo -e "${YELLOW}!${NC} $*"; }
fail() { echo -e "${RED}✗${NC} $*"; exit 1; }
section() { echo -e "\n${BOLD}── $* ──${NC}"; }

# ─── Argument parsing ─────────────────────────────────────────────────────────
CHECK_ONLY=false
E2E=false
for arg in "$@"; do
  case "$arg" in
    --check) CHECK_ONLY=true ;;
    --e2e)   E2E=true ;;
    --help)
      echo "Usage: $0 [--check] [--e2e]"
      echo "  --check  cargo check only, skip tests"
      echo "  --e2e    print instructions for manual end-to-end tests"
      exit 0 ;;
  esac
done

# ─── 1. Cargo check ───────────────────────────────────────────────────────────
section "cargo check"
cd "$WORKSPACE_ROOT"

PACKAGES=(
  subsecond
  dioxus-cli-config
  dioxus-cli
  cdylib-basic
  cdylib-basic-host
  cdylib-tls
  cdylib-tls-host
  cdylib-autoconnect
  cdylib-autoconnect-host
  bin-basic
  bin-dep
  bin-multi-crate
  bin-dep-nested
  bin-dep-middle
  bin-transitive-dep
  cross-tls-crate
  cross-tls-crate-dylib
  subsecond-tls-harness
)

for pkg in "${PACKAGES[@]}"; do
  if cargo check -p "$pkg" --quiet 2>/dev/null; then
    pass "cargo check $pkg"
  else
    fail "cargo check $pkg"
  fi
done

if $CHECK_ONLY; then
  echo ""
  pass "All checks passed (--check mode, skipping tests)"
  exit 0
fi

# ─── 2. Unit tests ────────────────────────────────────────────────────────────
section "unit tests"

run_tests() {
  local pkg="$1"
  shift
  if cargo test -p "$pkg" "$@" --quiet >/dev/null 2>&1; then
    pass "cargo test $pkg"
  else
    # Re-run to show the failure output
    cargo test -p "$pkg" "$@" 2>&1 | tail -20
    fail "cargo test $pkg"
  fi
}

run_tests subsecond
run_tests dioxus-cli-config -- --test-threads=1
run_tests dioxus-cli

# ─── 3. cdylib builds ─────────────────────────────────────────────────────────
section "cdylib builds"

for pkg in cdylib-basic cdylib-tls cdylib-autoconnect; do
  if cargo build -p "$pkg" --quiet 2>/dev/null; then
    pass "cargo build $pkg"
  else
    fail "cargo build $pkg"
  fi
done

# ─── 4. Host builds ───────────────────────────────────────────────────────────
section "host binary builds"

for pkg in cdylib-basic-host cdylib-tls-host cdylib-autoconnect-host bin-basic bin-multi-crate bin-transitive-dep subsecond-tls-harness; do
  if cargo build -p "$pkg" --quiet 2>/dev/null; then
    pass "cargo build $pkg"
  else
    fail "cargo build $pkg"
  fi
done

# ─── 5. End-to-end instructions ───────────────────────────────────────────────
if $E2E; then
  section "end-to-end test instructions"
  cat <<'EOF'

These tests require a running devserver. Run each scenario in two terminals.

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
TEST 1 — Basic cdylib patch (cdylib-basic)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Terminal 1 (devserver):
  cargo run -p dioxus-cli -- serve --lib -p cdylib-basic --hot-patch

Terminal 2 (host):
  # Copy the path printed by the CLI above (e.g. .../desktop-dev/libcdylib_basic.so)
  export CDYLIB_PATH=<path printed by CLI>
  export DIOXUS_DEVSERVER_IP=127.0.0.1
  export DIOXUS_DEVSERVER_PORT=8080   # adjust if CLI printed a different port
  cargo run -p cdylib-basic-host

Expected: output shows "version = 1" each second.
Patch:    edit cdylib-basic/src/lib.rs — change `1` to `2` in get_version().
Expected: output changes to "version = 2" without restarting the host.

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
TEST 2 — TLS preservation (cdylib-tls)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Terminal 1:
  cargo run -p dioxus-cli -- serve --lib -p cdylib-tls --hot-patch

Terminal 2:
  export CDYLIB_PATH=<path printed by CLI>
  export DIOXUS_DEVSERVER_IP=127.0.0.1
  export DIOXUS_DEVSERVER_PORT=8080
  cargo run -p cdylib-tls-host

Expected: counter increments each second: "tick v1: counter = 1", "= 2", ...
Patch:    edit cdylib-tls/src/lib.rs — change "tick v1" to "tick v2".
Expected: counter continues from where it left off ("tick v2: counter = N+1").
Failure:  counter resets to 1 after the patch — this indicates a TLS bug.

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
TEST 3 — Auto-init via #[ctor] (cdylib-autoconnect)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Terminal 1:
  cargo run -p dioxus-cli -- serve --lib -p cdylib-autoconnect --hot-patch

Terminal 2:
  export CDYLIB_PATH=<path printed by CLI>
  export DIOXUS_DEVSERVER_IP=127.0.0.1
  export DIOXUS_DEVSERVER_PORT=8080
  cargo run -p cdylib-autoconnect-host

Expected: output shows "compute() = 42" each second.
Patch:    edit cdylib-autoconnect/src/lib.rs — change `42` to `99`.
Expected: output changes to "compute() = 99" without any explicit init call.

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
TEST 4 — Basic single-crate binary (bin-basic)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  cargo run -p dioxus-cli -- serve -p bin-basic --hot-patch

Expected: "Hello from bin-basic v1" each second.
Patch:    edit bin-basic/src/main.rs — change "v1" to "v2".
Expected: output changes without restarting.

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
TEST 5 — Multi-crate binary (bin-multi-crate)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  cargo run -p dioxus-cli -- serve -p bin-multi-crate --hot-patch

Expected: "compute(21) = 42" each second.
Patch:    edit bin-multi-crate/src/main.rs — change compute(21) to compute(99).
      OR  edit bin-dep/src/lib.rs — change x * 2 to x * 3.
Expected: output changes without restarting.

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
TEST 6 — Transitive-dependency binary (bin-transitive-dep)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  cargo run -p dioxus-cli -- serve -p bin-transitive-dep --hot-patch

Chain: bin-transitive-dep → bin-dep-middle → bin-dep-nested
Expected: "quadruple(7) = 28" each second.
Patch A:  edit argument to quadruple() in this file.
Patch B:  edit bin-dep-middle/src/lib.rs (direct dep).
Patch C:  edit bin-dep-nested/src/lib.rs (transitive dep — not in Cargo.toml of tip).
Expected: output changes for all three without restarting.

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
TEST 7 — cross-tls (existing bin-target test, unmodified)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  cargo run -p dioxus-cli -- serve -p subsecond-tls-harness --hot-patch

See cross-tls-test/README.md for full instructions.

EOF
fi

# ─── Done ─────────────────────────────────────────────────────────────────────
echo ""
pass "All automated checks and tests passed."
if ! $E2E; then
  warn "Run with --e2e to see manual end-to-end test instructions."
fi

# Test 1 success

# Test 2 fails: TLS is reset on patch

# Test 3 inconclusive; works but dioxus_devtools::connect_subsecond() is called on patch

# Test 4 fails on patch; something in our changes breaks patching
# 14:48:54 [dev] Thread tokio-rt-worker panicked at packages/cli/src/build/request.rs:2513:14:

#                failed to resolve patch symbols: InvalidModule("ASLR reference is less than the module's base address. 0 < 1852a0") 
# 14:48:54 [dev] Build failed: Build panicked! JoinError::Panic(Id(42), "failed to resolve patch symbols: InvalidModule(\"ASLR reference is less than the module's base address. 0 < 1852a0\")", ...) 
