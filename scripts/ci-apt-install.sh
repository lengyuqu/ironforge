#!/usr/bin/env bash
# apt-get update + install with retries, for CI runners with flaky mirrors.
#
#   scripts/ci-apt-install.sh package...
#
# `Acquire::Retries` retries single downloads; the outer retry covers a mirror
# that is unreachable for longer than that.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
[ "$#" -gt 0 ] || { echo "usage: $0 package..." >&2; exit 2; }
export DEBIAN_FRONTEND=noninteractive
"$here/ci-retry.sh" -- sudo apt-get -o Acquire::Retries=5 update
"$here/ci-retry.sh" -- sudo apt-get -o Acquire::Retries=5 install -y "$@"
