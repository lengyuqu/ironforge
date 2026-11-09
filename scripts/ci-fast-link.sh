#!/usr/bin/env bash
# Use the mold linker for the rest of the job when it can be installed and works.
#
#   scripts/ci-fast-link.sh
#
# A workspace with ~130 integration-test binaries links a lot, and the default
# GNU ld is slow at it. This is an optimisation, never a requirement: if mold
# cannot be installed, or a trial link with it fails, the job keeps the default
# linker and says so. On success it appends RUSTFLAGS to $GITHUB_ENV.
set -u
here="$(cd "$(dirname "$0")" && pwd)"

if ! command -v mold >/dev/null 2>&1; then
  "$here/ci-apt-install.sh" mold || { echo "::warning::mold could not be installed; using the default linker"; exit 0; }
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
echo 'fn main() { println!("ok"); }' > "$tmp/main.rs"
if rustc -C link-arg=-fuse-ld=mold -o "$tmp/probe" "$tmp/main.rs" 2>"$tmp/err" && [ "$("$tmp/probe")" = "ok" ]; then
  if [ -n "${GITHUB_ENV:-}" ]; then
    echo "RUSTFLAGS=-C link-arg=-fuse-ld=mold" >> "$GITHUB_ENV"
  fi
  echo "linking with mold $(mold --version | head -1)"
else
  echo "::warning::a trial link with mold failed; using the default linker"
  sed 's/^/  /' "$tmp/err" >&2
fi
exit 0
