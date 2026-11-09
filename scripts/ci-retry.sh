#!/usr/bin/env bash
# Run a command, retrying it when it fails.
#
#   scripts/ci-retry.sh [-n ATTEMPTS] [-d SECONDS] -- command [args...]
#
# For steps that fail on the network rather than on the code: the git233
# runners reach apt mirrors and static.rust-lang.org only intermittently, and
# one dropped connection used to turn a whole run red. The delay grows with each
# attempt. The exit status is the last attempt's.
set -u
attempts=4
delay=10
while getopts "n:d:" opt; do
  case "$opt" in
    n) attempts="$OPTARG" ;;
    d) delay="$OPTARG" ;;
    *) echo "usage: $0 [-n attempts] [-d seconds] -- command [args...]" >&2; exit 2 ;;
  esac
done
shift $((OPTIND - 1))
[ "${1:-}" = "--" ] && shift
[ "$#" -gt 0 ] || { echo "usage: $0 [-n attempts] [-d seconds] -- command [args...]" >&2; exit 2; }

n=1
while true; do
  "$@" && exit 0
  status=$?
  if [ "$n" -ge "$attempts" ]; then
    echo "::error::'$*' failed after ${n} attempts (last exit status ${status})" >&2
    exit "$status"
  fi
  wait=$((delay * n))
  echo "::warning::attempt ${n}/${attempts} of '$*' failed (exit ${status}); retrying in ${wait}s" >&2
  sleep "$wait"
  n=$((n + 1))
done
