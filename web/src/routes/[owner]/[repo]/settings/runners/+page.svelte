<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { runners } from '$lib/api/client';
  import { createT } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  let runnerList = $state<any[]>([]);
  let loading = $state(true);
  let error = $state('');
  let currentPage = $state(1);
  let totalPages = $state(1);

  // Form state
  let newRunnerName = $state('');
  let newRunnerLabels = $state('');
  let saving = $state(false);

  $effect(() => {
    loadRunners();
  });

  async function loadRunners() {
    loading = true;
    error = '';
    try {
      const res = await runners.list(currentPage, 20);
      runnerList = res.data;
      totalPages = res.pagination.total_pages;
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function handleRegister() {
    if (!newRunnerName.trim()) return;
    saving = true;
    error = '';
    try {
      await runners.register({
        name: newRunnerName,
        labels: newRunnerLabels ? newRunnerLabels.split(',').map((s: string) => s.trim()) : undefined,
      });
      newRunnerName = '';
      newRunnerLabels = '';
      await loadRunners();
    } catch (e: any) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  async function handleDelete(id: number) {
    if (!confirm(t('runners.delete_confirm', { name: id }))) return;
    try {
      await runners.delete(id);
      await loadRunners();
    } catch (e: any) {
      error = e.message;
    }
  }
</script>

<svelte:head>
  <title>Runners · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader owner={owner!} repo={repo!} activeTab="runners" />

  <div class="page-header">
    <h1>{t('repo.tabs.runners')}</h1>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <!-- Register new runner -->
  <div class="register-card">
    <h2>{t('runners.register')}</h2>
    <div class="form-group">
      <label for="runner-name">{t('runners.name')}</label>
      <input
        id="runner-name"
        type="text"
        bind:value={newRunnerName}
        placeholder="my-runner"
      />
    </div>
    <div class="form-group">
      <label for="runner-labels">{t('runners.labels')}</label>
      <input
        id="runner-labels"
        type="text"
        bind:value={newRunnerLabels}
        placeholder="linux,x86_64"
      />
    </div>
    <button
      class="btn-primary"
      onclick={handleRegister}
      disabled={saving}
    >
      {saving ? t('common.loading') : t('runners.register')}
    </button>
  </div>

  <!-- Runner list -->
  {#if loading}
    <p class="loading-text">{t('common.loading')}</p>
  {:else if runnerList.length === 0}
    <div class="empty">
      <p>{t('runners.no_runners')}</p>
    </div>
  {:else}
    <div class="runner-list">
      {#each runnerList as runner (runner.id)}
        <div class="runner-card">
          <div class="runner-header">
            <span class="runner-name">{runner.name}</span>
            <span class="runner-status" class:online={runner.status === 'online'}>
              {runner.status}
            </span>
          </div>
          {#if runner.labels && runner.labels.length > 0}
            <div class="runner-labels">
              {#each runner.labels as label}
                <span class="label-badge">{label}</span>
              {/each}
            </div>
          {/if}
          <div class="runner-meta">
            <span>v{runner.version || '?'}</span>
            <span>{runner.last_seen ? t('common.last_seen', { time: new Date(runner.last_seen).toLocaleString() }) : t('common.offline')}</span>
          </div>
          <div class="runner-actions">
            <button
              class="btn-danger btn-sm"
              onclick={() => handleDelete(runner.id)}
            >
              {t('common.delete')}
            </button>
          </div>
        </div>
      {/each}
    </div>

    {#if totalPages > 1}
      <div class="pagination">
        <button
          class="btn-outline"
          disabled={currentPage <= 1}
          onclick={() => { currentPage = currentPage - 1; loadRunners(); }}
        >
          {t('common.previous') || 'Previous'}
        </button>
        <span class="page-info">Page {currentPage} of {totalPages}</span>
        <button
          class="btn-outline"
          disabled={currentPage >= totalPages}
          onclick={() => { currentPage = currentPage + 1; loadRunners(); }}
        >
          {t('common.next') || 'Next'}
        </button>
      </div>
    {/if}
  {/if}
</div>

<style>
  .repo-page {
    max-width: 900px;
    margin: 0 auto;
    padding: 24px;
  }

  .page-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 24px;
  }

  h1 {
    font-size: 24px;
    font-weight: 600;
  }

  .error-banner {
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid var(--red-dim);
    color: var(--red);
    border-radius: var(--radius);
    padding: 10px 14px;
    font-size: 13px;
    margin-bottom: 16px;
  }

  .register-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 20px;
    margin-bottom: 24px;
  }

  h2 {
    font-size: 18px;
    font-weight: 600;
    margin: 0 0 16px 0;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 12px;
  }

  .form-group label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
  }

  .form-group input {
    padding: 6px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-primary);
    color: var(--text-primary);
    font-size: 14px;
  }

  .btn-primary {
    padding: 6px 16px;
    background: var(--orange);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    text-decoration: none;
  }
  .btn-primary:hover { background: #e09a1e; text-decoration: none; }
  .btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }

  .loading-text {
    color: var(--text-secondary);
    text-align: center;
    padding: 48px;
  }

  .empty {
    text-align: center;
    padding: 48px;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .runner-list {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .runner-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
  }

  .runner-header {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 8px;
  }

  .runner-name {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .runner-status {
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 12px;
    font-weight: 600;
    background: var(--bg-tertiary);
    color: var(--text-muted);
  }
  .runner-status.online {
    background: var(--green-dim);
    color: #fff;
  }

  .runner-labels {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    margin-bottom: 8px;
  }

  .label-badge {
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 12px;
    font-weight: 600;
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }

  .runner-meta {
    display: flex;
    gap: 16px;
    font-size: 13px;
    color: var(--text-muted);
    margin-bottom: 12px;
  }

  .runner-actions {
    display: flex;
    gap: 8px;
  }

  .btn-danger {
    padding: 5px 12px;
    background: var(--red-dim);
    border: 1px solid var(--red);
    border-radius: var(--radius);
    color: #fff;
    font-size: 13px;
    cursor: pointer;
  }
  .btn-danger:hover { background: var(--red); }

  .btn-sm {
    padding: 4px 10px;
    font-size: 12px;
  }

  .pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
    margin-top: 24px;
  }

  .btn-outline {
    padding: 5px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
  }
  .btn-outline:hover { background: var(--bg-hover); }
  .btn-outline:disabled { opacity: 0.5; cursor: not-allowed; }

  .page-info {
    font-size: 14px;
    color: var(--text-secondary);
  }

  @media (max-width: 600px) {
    .page-header {
      flex-direction: column;
      align-items: flex-start;
      gap: 12px;
    }
  }
</style>
