<script lang="ts">
  // Login attempts section — self-contained: loads the audit list with
  // filters (username/provider/status/time range) and pagination.
  import { admin, type LoginAttemptEntry } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';

  interface Props {
    initialAttempts: LoginAttemptEntry[];
    initialTotal: number;
    initialPage: number;
  }

  let { initialAttempts, initialTotal, initialPage }: Props = $props();

  const loginAttemptsPerPage = 20;

  let attempts = $state<LoginAttemptEntry[]>([]);
  let total = $state(0);
  let page = $state(1);
  let loading = $state(false);
  let usernameFilter = $state('');
  let providerFilter = $state('');
  let statusFilter = $state<'all' | 'success' | 'failure'>('all');
  let startTime = $state('');
  let endTime = $state('');

  // Snapshot initial values (component mounts once per page load).
  attempts = initialAttempts;
  total = initialTotal;
  page = initialPage;

  const pages = $derived(Math.max(1, Math.ceil(total / loginAttemptsPerPage)));

  async function loadLoginAttempts(nextPage = 1) {
    try {
      loading = true;
      const result = await admin.listLoginAttempts({
        page: nextPage,
        per_page: loginAttemptsPerPage,
        username: usernameFilter.trim() || undefined,
        auth_provider: providerFilter.trim() || undefined,
        success: statusFilter === 'all' ? undefined : statusFilter === 'success',
        start_time: startTime ? new Date(startTime).toISOString() : undefined,
        end_time: endTime ? new Date(endTime).toISOString() : undefined,
      });
      attempts = result.attempts;
      total = result.total;
      page = result.page;
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      loading = false;
    }
  }

  function formatLoginTime(value: string) {
    return new Date(value).toLocaleString();
  }
</script>

<div class="section">
  <div class="section-heading">
    <div>
      <h2>Login Attempts</h2>
      <span class="text-secondary">{total} matching events</span>
    </div>
    <button class="btn-secondary" type="button" disabled={loading} onclick={() => loadLoginAttempts(page)}>
      {loading ? 'Loading...' : 'Refresh'}
    </button>
  </div>
  <div class="login-filters">
    <input aria-label="Filter login attempts by username" placeholder="Username" bind:value={usernameFilter} />
    <input aria-label="Filter login attempts by provider" placeholder="Provider (password, ldap...)" bind:value={providerFilter} />
    <select aria-label="Filter login attempts by status" bind:value={statusFilter}>
      <option value="all">All results</option>
      <option value="failure">Failed only</option>
      <option value="success">Successful only</option>
    </select>
    <input aria-label="Login attempts start time" type="datetime-local" bind:value={startTime} />
    <input aria-label="Login attempts end time" type="datetime-local" bind:value={endTime} />
    <button class="btn-secondary" type="button" disabled={loading} onclick={() => loadLoginAttempts(1)}>Apply</button>
  </div>
  {#if attempts.length === 0}
    <p class="text-secondary">No matching login attempts.</p>
  {:else}
    <div class="login-attempt-list">
      {#each attempts as attempt (attempt.id)}
        <div class="login-attempt-row">
          <span class="attempt-status" class:success={attempt.success}>{attempt.success ? 'Success' : 'Failed'}</span>
          <div class="attempt-identity">
            <strong>{attempt.username}</strong>
            <span>{attempt.auth_provider}{attempt.failure_reason ? ` · ${attempt.failure_reason}` : ''}</span>
          </div>
          <span title={attempt.user_agent || ''}>{attempt.ip_address || 'Unknown IP'}</span>
          <time datetime={attempt.created_at}>{formatLoginTime(attempt.created_at)}</time>
        </div>
      {/each}
    </div>
    <div class="login-pagination">
      <button class="btn-secondary" type="button" disabled={loading || page <= 1} onclick={() => loadLoginAttempts(page - 1)}>Previous</button>
      <span>Page {page} of {pages}</span>
      <button class="btn-secondary" type="button" disabled={loading || page >= pages} onclick={() => loadLoginAttempts(page + 1)}>Next</button>
    </div>
  {/if}
</div>

<style>
  h2 { font-size: 16px; margin: 0 0 12px; }

  .section {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
    margin-bottom: 16px;
  }

  .section-heading { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .section-heading h2 { margin-bottom: 2px; }

  .login-filters { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 8px; margin: 14px 0; }
  .login-filters input,
  .login-filters select {
    padding: 7px 9px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-primary);
    color: var(--text-primary);
  }

  .login-attempt-list { border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; }

  .login-attempt-row {
    display: grid;
    grid-template-columns: 64px minmax(150px, 1fr) minmax(110px, 0.6fr) auto;
    align-items: center;
    gap: 12px;
    padding: 9px 10px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
  }
  .login-attempt-row:last-child { border-bottom: 0; }

  .attempt-status { color: #cf222e; font-weight: 600; }
  .attempt-status.success { color: #1a7f37; }

  .attempt-identity { display: flex; flex-direction: column; min-width: 0; }
  .attempt-identity span,
  .login-attempt-row time { color: var(--text-secondary); }

  .login-pagination {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 10px;
    color: var(--text-secondary);
    font-size: 12px;
  }

  .btn-secondary {
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-primary);
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
  }
  .btn-secondary:disabled { opacity: 0.6; cursor: wait; }

  @media (max-width: 720px) {
    .login-filters { grid-template-columns: 1fr; }
    .login-attempt-row { grid-template-columns: 64px 1fr; }
  }
</style>
