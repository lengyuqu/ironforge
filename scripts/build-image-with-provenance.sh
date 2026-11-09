#!/usr/bin/env bash
# Build a local image from the exact committed tree, not a mutable working copy.
# Usage: scripts/build-image-with-provenance.sh [image-tag]
set -euo pipefail
cd "$(dirname "$0")/.."
if [ -n "$(git status --porcelain --untracked-files=normal)" ]; then
  echo 'Commit or otherwise resolve the worktree changes before building.' >&2
  exit 1
fi
commit=$(git rev-parse --verify HEAD)
# The archive fixes the build context at this commit even if files change
# while Docker is building; ignored and untracked files cannot enter it.
git archive "$commit" | docker build \
  --build-arg "IRONFORGE_BUILD_COMMIT=$commit" \
  --build-arg IRONFORGE_BUILD_DIRTY=false \
  -t "${1:-ironforge:local}" -
