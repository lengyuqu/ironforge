#!/usr/bin/env bash
# Install the stable Rust toolchain with retries.
#
#   scripts/ci-install-rust.sh [components]    e.g. clippy,rustfmt
#
# The workflow uses dtolnay/rust-toolchain first; this is its second chance when
# the download from static.rust-lang.org drops (the runners' connection to it is
# intermittent). Idempotent: with rustup and the toolchain already present it
# only re-checks them.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
components="${1:-}"
export PATH="$HOME/.cargo/bin:$PATH"

if ! command -v rustup >/dev/null 2>&1; then
  tmp="$(mktemp -d)"
  "$here/ci-retry.sh" -- curl --proto '=https' --tlsv1.2 -sSf --retry 3 --retry-all-errors \
    https://sh.rustup.rs -o "$tmp/rustup.sh"
  "$here/ci-retry.sh" -- sh "$tmp/rustup.sh" -y --profile minimal --default-toolchain none
  rm -rf "$tmp"
fi

args=(toolchain install stable --profile minimal)
[ -n "$components" ] && args+=(--component "$components")
"$here/ci-retry.sh" -- rustup "${args[@]}"
rustup default stable

if [ -n "${GITHUB_PATH:-}" ]; then
  echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"
fi
rustc --version
cargo --version
