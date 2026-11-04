#!/usr/bin/env bash
# Local gate: the checks CI runs, in one command.
#
#   scripts/check.sh            # formatting + clippy + rg-http/rg-core tests
#   scripts/check.sh --quick    # formatting + clippy only (seconds)
#   scripts/check.sh --full     # everything CI runs, including the whole workspace
#
# Run this before pushing. CI (.github/workflows/regression.yml) executes the
# same steps, so a green run here means the push should stay green there.
set -euo pipefail

cd "$(dirname "$0")/.."

mode="default"
case "${1:-}" in
    --quick) mode="quick" ;;
    --full) mode="full" ;;
    "") ;;
    -h|--help)
        sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *)
        echo "unknown option: $1 (try --quick, --full or --help)" >&2
        exit 2
        ;;
esac

started=$SECONDS
step() {
    step_started=$SECONDS
    printf '\n==> %s\n' "$*"
}
elapsed() { printf '    (%ss)\n' "$((SECONDS - step_started))"; }

step "cargo fmt --all --check"
cargo fmt --all --check
elapsed

step "cargo clippy --workspace --all-targets -D warnings"
cargo clippy --workspace --all-targets -- -D warnings
elapsed

if [ "$mode" != "quick" ]; then
    step "cargo test -p rg-http -p rg-core"
    cargo test -p rg-http -p rg-core
    elapsed
fi

if [ "$mode" = "full" ]; then
    step "cargo test --workspace"
    cargo test --workspace
    elapsed
fi

printf '\n==> checks passed in %ss (%s)\n' "$((SECONDS - started))" "$mode"
