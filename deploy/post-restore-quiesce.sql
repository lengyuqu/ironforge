-- Post-restore quiesce for PostgreSQL and MySQL (also valid SQLite).
--
-- A restored database is a snapshot of the past: run this ONCE, after
-- `ironforge migrate` against the restored database and BEFORE starting the
-- server. It is the same policy `ironforge restore` / `restore-db` apply to
-- SQLite by default:
--
--   * collaboration sessions are ended (clients register new ones);
--   * password-reset tokens are deleted;
--   * queued webhook / e-mail jobs are marked dead (they may already have
--     gone out from the live instance);
--   * pending / running CI pipelines, stages and jobs are cancelled / failed
--     (a restored runner would otherwise re-run work the live instance did).
--
-- Deliberately NOT touched: personal access tokens, SSH / deploy keys, runner
-- registrations, mirror credentials and the merge queue. The backup cannot say
-- which were revoked or changed afterwards - review or rotate them by hand.
--
--   psql  "$DATABASE_URL" -v ON_ERROR_STOP=1 -f deploy/post-restore-quiesce.sql
--   mysql ... ironforge < deploy/post-restore-quiesce.sql
-- Run it in a transaction if your tooling supports it.

UPDATE agent_sessions SET ended_at = CURRENT_TIMESTAMP WHERE ended_at IS NULL;

DELETE FROM password_reset_tokens;

UPDATE background_jobs
   SET status = 'dead',
       locked_by = NULL,
       locked_at = NULL,
       last_error = 'abandoned at restore: may already have been delivered',
       updated_at = CURRENT_TIMESTAMP
 WHERE status IN ('pending', 'running')
   AND task_type IN ('webhook.deliver', 'email.send');

UPDATE pipelines
   SET status = 'canceled', finished_at = CURRENT_TIMESTAMP
 WHERE status IN ('pending', 'running');

UPDATE pipeline_stages
   SET status = 'failed', finished_at = CURRENT_TIMESTAMP
 WHERE status IN ('pending', 'running');

UPDATE pipeline_jobs
   SET status = 'failed', finished_at = CURRENT_TIMESTAMP
 WHERE status IN ('pending', 'running');
