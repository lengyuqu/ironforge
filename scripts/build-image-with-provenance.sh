#!/usr/bin/env bash
# Build a local image whose capability endpoint identifies its source checkout.
# Usage: scripts/build-image-with-provenance.sh [image-tag]
set -euo pipefail
cd "$(dirname "$0")/.."
commit=$(git rev-parse --verify HEAD)
if [ -n "$(git status --porcelain --untracked-files=normal)" ]; then
  dirty=true
else
  dirty=false
fi
exec docker build --build-arg "IRONFORGE_BUILD_COMMIT=$commit" \
  --build-arg "IRONFORGE_BUILD_DIRTY=$dirty" \
  -t "${1:-ironforge:local}" .
