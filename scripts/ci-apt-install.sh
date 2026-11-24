#!/usr/bin/env bash
# apt-get update + install with retries, for CI runners with flaky mirrors.
#
#   scripts/ci-apt-install.sh package...
#
# `Acquire::Retries` retries single downloads; the outer retry covers a mirror
# that is unreachable for longer than that.
#
# The timeouts are the point: apt's own defaults allow a stalled connection to
# hold a job for many minutes, which is how a Clippy job once spent 32 minutes
# here and timed out before it could save its cache. `ForceIPv4` avoids the
# same symptom when an IPv6 route exists but goes nowhere, a common shape on
# these runners.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
[ "$#" -gt 0 ] || { echo "usage: $0 package..." >&2; exit 2; }
export DEBIAN_FRONTEND=noninteractive
apt_opts=(
  -o Acquire::Retries=5
  -o Acquire::http::Timeout=30
  -o Acquire::https::Timeout=30
  -o Acquire::ForceIPv4=true
)
# One attempt gets 5 minutes; four attempts plus backoff stays under 25.
"$here/ci-retry.sh" -t 300 -- sudo apt-get "${apt_opts[@]}" update
"$here/ci-retry.sh" -t 300 -- sudo apt-get "${apt_opts[@]}" install -y "$@"
