#!/usr/bin/env bash
# Dump/restore drill for PostgreSQL and MySQL (the CLI `backup`/`restore` are
# SQLite-only; for these backends the backup tool is the database's own).
#
#   scripts/db-restore-drill.sh postgres|mysql
#
# What it proves, in a throw-away Docker server:
#   1. a database populated by a real collaboration test target is dumped with
#      pg_dump / mysqldump (the commands an operator would run);
#   2. the dump restores into a second, empty database;
#   3. every table has the same row count and the same content checksum in both;
#   4. `ironforge migrate` against the restored database finds nothing to do and
#      the migration history is identical.
#   5. deploy/post-restore-quiesce.sql applies cleanly to the restored database and
#      leaves no live session, reset token, queued webhook/e-mail job or running CI.
# What it does NOT prove: Git repositories and blob storage (copy them with
# the file backup), server start-up on the restored database, point-in-time
# recovery, or anything about personal access tokens / SSH keys (untouched by
# design; review them by hand, see deploy/README.md).
#
# Needs Docker. Uses host networking like scripts/ci-db-tests.sh, so stop any
# local PostgreSQL/MySQL on 5432/3306 first.
set -euo pipefail

backend="${1:?usage: $0 postgres|mysql}"
name="ironforge-drill-${backend}"
case "$backend" in
  postgres) port=5432 ;;
  mysql) port=3306 ;;
  *) echo "unknown backend: $backend" >&2; exit 2 ;;
esac
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

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
    sql() { docker exec -i "$name" psql -p "$port" -U ironforge -d "$1" -v ON_ERROR_STOP=1 -At; }
    mkdb() { echo "DROP DATABASE IF EXISTS $1 WITH (FORCE); CREATE DATABASE $1" | sql postgres >/dev/null; }
    dump() { docker exec "$name" pg_dump -p "$port" -U ironforge -Fc ironforge; }
    load() { docker exec -i "$name" pg_restore -p "$port" -U ironforge -d ironforge_restored --no-owner; }
    tables() { echo "SELECT tablename FROM pg_tables WHERE schemaname='public' ORDER BY 1" | sql "$1"; }
    rows() { echo "SELECT count(*) FROM \"$2\"" | sql "$1"; }
    digest() { echo "SELECT COALESCE(md5(string_agg(x::text, '|' ORDER BY x::text)), '') FROM \"$2\" x" | sql "$1"; }
    url() { echo "postgres://ironforge:ironforge@127.0.0.1:${port}/$1"; }
    ;;
  mysql)
    docker run -d --name "$name" "${net[@]}" \
      -e MYSQL_ROOT_PASSWORD=root -e MYSQL_DATABASE=ironforge \
      -e MYSQL_USER=ironforge -e MYSQL_PASSWORD=ironforge \
      mysql:8.4 --port="$port" --log-bin-trust-function-creators=1 >/dev/null
    ready() { docker exec "$name" mysqladmin ping -h 127.0.0.1 -P "$port" -uroot -proot >/dev/null 2>&1; }
    sql() { docker exec -i "$name" mysql -h 127.0.0.1 -P "$port" -uroot -proot -N -B "$1"; }
    mkdb() { docker exec "$name" mysql -h 127.0.0.1 -P "$port" -uroot -proot \
      -e "DROP DATABASE IF EXISTS $1; CREATE DATABASE $1; GRANT ALL ON $1.* TO 'ironforge'@'%'" >/dev/null; }
    dump() { docker exec "$name" mysqldump -h 127.0.0.1 -P "$port" -uroot -proot \
      --single-transaction --routines --triggers --hex-blob ironforge; }
    load() { docker exec -i "$name" mysql -h 127.0.0.1 -P "$port" -uroot -proot ironforge_restored; }
    tables() { echo "SHOW TABLES" | sql "$1"; }
    rows() { echo "SELECT COUNT(*) FROM \`$2\`" | sql "$1"; }
    # CHECKSUM TABLE reads the live table; it is order-independent enough for a
    # restore comparison because both sides are computed the same way.
    digest() { echo "CHECKSUM TABLE \`$2\`" | sql "$1" | awk '{print $2}'; }
    url() { echo "mysql://ironforge:ironforge@127.0.0.1:${port}/$1"; }
    ;;
  *) echo "unknown backend: $backend" >&2; exit 2 ;;
esac

echo "waiting for $backend ..."
for _ in $(seq 1 90); do ready && break; sleep 2; done
ready || { echo "$backend did not become ready" >&2; exit 1; }
sleep 5

# 1. Populate with a real test target (it migrates the schema and writes data).
mkdb ironforge
IRONFORGE_TEST_DATABASE_URL="$(url ironforge)" \
  cargo test -p rg-http --test collab_condition_tests

# 2. Dump and restore into a second database.
mkdb ironforge_restored
dump | load
echo "dump restored into ironforge_restored"

# 3. Same tables, same rows, same content.
src_tables="$(tables ironforge)"
dst_tables="$(tables ironforge_restored)"
[ "$src_tables" = "$dst_tables" ] || { echo "table lists differ" >&2; diff <(echo "$src_tables") <(echo "$dst_tables") >&2 || true; exit 1; }
count=0
nonempty=0
while IFS= read -r t; do
  [ -n "$t" ] || continue
  a="$(rows ironforge "$t")"; b="$(rows ironforge_restored "$t")"
  [ "$a" = "$b" ] || { echo "row count differs in $t: $a vs $b" >&2; exit 1; }
  da="$(digest ironforge "$t")"; db="$(digest ironforge_restored "$t")"
  [ "$da" = "$db" ] || { echo "content differs in $t" >&2; exit 1; }
  count=$((count + 1))
  [ "$a" != "0" ] && nonempty=$((nonempty + 1))
done <<<"$src_tables"
echo "$count tables identical ($nonempty with rows)"
[ "$nonempty" -gt 10 ] || { echo "drill is too weak: only $nonempty tables hold rows" >&2; exit 1; }

# 4. The restored database needs no migration and has the same history.
cargo run -q -p rg-cli -- migrate --db-url "$(url ironforge_restored)"
hist_a="$(echo "SELECT version FROM seaql_migrations ORDER BY version" | sql ironforge)"
hist_b="$(echo "SELECT version FROM seaql_migrations ORDER BY version" | sql ironforge_restored)"
[ "$hist_a" = "$hist_b" ] || { echo "migration history differs after migrate" >&2; exit 1; }
echo "migration history identical ($(echo "$hist_a" | wc -l | tr -d ' ') migrations)"

# 5. The post-restore quiesce (deploy/post-restore-quiesce.sql) applies cleanly and
#    leaves nothing live behind. The populated database has un-ended sessions, so
#    the "before" count makes the check meaningful.
live_sessions() { echo "SELECT COUNT(*) FROM agent_sessions WHERE ended_at IS NULL" | sql ironforge_restored; }
before="$(live_sessions)"
[ "$before" -gt 0 ] || { echo "drill is too weak: no live session to quiesce" >&2; exit 1; }
sql ironforge_restored < deploy/post-restore-quiesce.sql >/dev/null
after="$(live_sessions)"
[ "$after" = "0" ] || { echo "quiesce left $after live sessions" >&2; exit 1; }
left="$(echo "SELECT (SELECT COUNT(*) FROM password_reset_tokens) + (SELECT COUNT(*) FROM pipelines WHERE status IN ('pending','running')) + (SELECT COUNT(*) FROM pipeline_jobs WHERE status IN ('pending','running')) + (SELECT COUNT(*) FROM background_jobs WHERE status IN ('pending','running') AND task_type IN ('webhook.deliver','email.send'))" | sql ironforge_restored)"
[ "$left" = "0" ] || { echo "quiesce left $left live rows" >&2; exit 1; }
echo "post-restore quiesce: $before live sessions ended, nothing live left"
echo "$backend dump/restore drill passed"
