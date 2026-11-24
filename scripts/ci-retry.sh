#!/usr/bin/env bash
# Run a command, retrying it when it fails.
#
#   scripts/ci-retry.sh [-n ATTEMPTS] [-d SECONDS] [-t SECONDS] -- command [args...]
#
# For steps that fail on the network rather than on the code: the git233
# runners reach apt mirrors and static.rust-lang.org only intermittently, and
# one dropped connection used to turn a whole run red. The delay grows with each
# attempt. The exit status is the last attempt's.
#
# `-t` bounds a single attempt. Without it a connection that hangs instead of
# failing — the common shape on a lossy link — blocks until the job's own
# timeout, which is how one run sat in `apt-get update` for 32 minutes and
# never reached its cache-save step either. 0 (the default) keeps the old
# behaviour.
set -u
attempts=4
delay=10
timeout_s=0
while getopts "n:d:t:" opt; do
  case "$opt" in
    n) attempts="$OPTARG" ;;
    d) delay="$OPTARG" ;;
    t) timeout_s="$OPTARG" ;;
    *) echo "usage: $0 [-n attempts] [-d seconds] [-t seconds] -- command [args...]" >&2; exit 2 ;;
  esac
done
shift $((OPTIND - 1))
[ "${1:-}" = "--" ] && shift
[ "$#" -gt 0 ] || { echo "usage: $0 [-n attempts] [-d seconds] [-t seconds] -- command [args...]" >&2; exit 2; }
case "$timeout_s" in
  ''|*[!0-9]*) echo "-t must be a number of seconds, got '$timeout_s'" >&2; exit 2 ;;
esac

n=1
while true; do
  if [ "$timeout_s" -gt 0 ]; then
    timeout "$timeout_s" "$@"
  else
    "$@"
  fi
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
