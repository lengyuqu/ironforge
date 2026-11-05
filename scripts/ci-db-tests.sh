#!/usr/bin/env bash
# Database-backed Rust tests against a throw-away PostgreSQL or MySQL server
# started with Docker (used by the CNB pipelines; works anywhere Docker runs).
#
#   scripts/ci-db-tests.sh postgres|mysql
#
# What it runs, mirroring .github/workflows/regression.yml and adding the
# collaboration suites that the SQLite-only runs cannot judge:
#   1. the backend smoke test (migrations, CRUD, FTS) in rg-core;
#   2. every rg-db collab_* store/migration test;
#   3. every rg-http collab_* HTTP test.
# Each test target gets a FRESH database: several HTTP tests create "the first
# user" (an admin), so a database shared between targets makes later ones fail
# for reasons that have nothing to do with the code (CLAUDE.md, pitfall 23).
set -euo pipefail

backend="${1:?usage: $0 postgres|mysql}"
name="ironforge-ci-${backend}"
port=""
case "$backend" in
  postgres) port=5432 ;;
  mysql) port=3306 ;;
  *) echo "unknown backend: $backend" >&2; exit 2 ;;
esac

# Linux can share the host network; Docker Desktop (macOS/Windows) cannot, so
# there the port is published instead. Either way the tests reach 127.0.0.1:$port.
if [ "$(uname -s)" = "Linux" ]; then net=(--network host); else net=(-p "${port}:${port}"); fi

cleanup() { docker rm -f "$name" >/dev/null 2>&1 || true; }
trap cleanup EXIT
cleanup

case "$backend" in
  postgres)
    docker run -d --name "$name" "${net[@]}" \
      -e POSTGRES_USER=ironforge -e POSTGRES_PASSWORD=ironforge -e POSTGRES_DB=ironforge \
      postgres:17 -p "$port" >/dev/null
    ready() { docker exec "$name" pg_isready -U ironforge -d ironforge -p "$port" >/dev/null 2>&1; }
    reset() {
      docker exec "$name" psql -p "$port" -U ironforge -d postgres -v ON_ERROR_STOP=1 \
        -c "DROP DATABASE IF EXISTS ironforge WITH (FORCE)" -c "CREATE DATABASE ironforge" >/dev/null
    }
    url="postgres://ironforge:ironforge@127.0.0.1:${port}/ironforge"
    ;;
  mysql)
    # The FTS migration creates triggers; with binary logging on (the 8.4
    # default) a non-SUPER user needs log_bin_trust_function_creators.
    docker run -d --name "$name" "${net[@]}" \
      -e MYSQL_ROOT_PASSWORD=root -e MYSQL_DATABASE=ironforge \
      -e MYSQL_USER=ironforge -e MYSQL_PASSWORD=ironforge \
      mysql:8.4 --port="$port" --log-bin-trust-function-creators=1 >/dev/null
    ready() { docker exec "$name" mysqladmin ping -h 127.0.0.1 -P "$port" -uroot -proot >/dev/null 2>&1; }
    reset() {
      docker exec "$name" mysql -h 127.0.0.1 -P "$port" -uroot -proot \
        -e "DROP DATABASE IF EXISTS ironforge; CREATE DATABASE ironforge; GRANT ALL ON ironforge.* TO 'ironforge'@'%'" >/dev/null
    }
    url="mysql://ironforge:ironforge@127.0.0.1:${port}/ironforge"
    ;;
esac

echo "waiting for $backend ..."
for _ in $(seq 1 90); do
  if ready; then break; fi
  sleep 2
done
ready || { echo "$backend did not become ready" >&2; docker logs "$name" 2>&1 | tail -30 >&2; exit 1; }
# MySQL answers pings before its init scripts finish; give the user grant a moment.
sleep 5
export IRONFORGE_TEST_DATABASE_URL="$url"

run() {
  echo "::: $*"
  reset
  "$@"
}

run cargo test -p rg-core --test multi_backend_smoke -- --ignored --nocapture

for f in crates/rg-db/tests/collab_*.rs; do
  t="$(basename "$f" .rs)"
  run cargo test -p rg-db --test "$t"
done

for f in crates/rg-http/tests/collab_*_tests.rs; do
  t="$(basename "$f" .rs)"
  run cargo test -p rg-http --test "$t"
done
echo "all $backend suites passed"
