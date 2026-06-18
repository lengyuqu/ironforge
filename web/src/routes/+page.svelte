<script lang="ts">
  import { repos } from '$lib/api/client.svelte';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let repoList = $state<any[]>([]);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    repos.explore(1, 24).then(r => {
      repoList = r.data;
    }).catch((e: any) => {
      error = e.message;
    }).finally(() => {
      loading = false;
    });
  });
</script>

<svelte:head>
  <title>IronForge · {t('explore.title')}</title>
</svelte:head>

<div class="page-container">
  <div class="page-header">
    <h1>{t('explore.title')}</h1>
    <p class="subtitle">
      {#if !loading}
        {t('explore.subtitle', { count: repoList.length })}
      {/if}
    </p>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">{t('common.loading')}</p>
  {:else if repoList.length === 0}
    <div class="empty">
      <p>{t('explore.empty')}</p>
    </div>
  {:else}
    <div class="repo-grid">
      {#each repoList as repo}
        <a href="/{repo.owner_name}/{repo.name}" class="repo-card">
          <div class="rc-icon">📂</div>
          <div class="rc-body">
            <div class="rc-name">{repo.owner_name}/{repo.name}</div>
            <div class="rc-desc">{repo.description || t('common.no_description')}</div>
            <div class="rc-meta">
              {repo.stars_count || 0} ⭐ · {t('common.updated', { date: formatDate(repo.updated_at) })}
            </div>
          </div>
        </a>
      {/each}
    </div>

    <div class="explore-footer">
      <a href="/explore" class="view-all-btn">{t('home.explore.view_all')} →</a>
    </div>
  {/if}
</div>

<style>
  .page-header {
    margin-bottom: 24px;
  }

  h1 { font-size: 24px; margin-bottom: 4px; }

  .subtitle {
    font-size: 14px;
    color: var(--text-secondary);
    min-height: 20px;
  }

  .empty {
    text-align: center;
    padding: 60px 24px;
    color: var(--text-secondary);
  }

  .repo-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
  }

  @media (max-width: 900px) {
    .repo-grid { grid-template-columns: repeat(2, 1fr); }
  }
  @media (max-width: 600px) {
    .repo-grid { grid-template-columns: 1fr; }
  }

  .repo-card {
    display: flex;
    gap: 12px;
    padding: 16px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    text-decoration: none;
    color: var(--text-primary);
    transition: border-color 0.15s;
  }
  .repo-card:hover {
    border-color: var(--accent);
    text-decoration: none;
  }

  .rc-icon { font-size: 20px; flex-shrink: 0; }
  .rc-body { flex: 1; min-width: 0; }

  .rc-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--accent);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rc-desc {
    font-size: 12px;
    color: var(--text-secondary);
    margin-top: 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rc-meta {
    font-size: 11px;
    color: var(--text-muted);
    margin-top: 8px;
  }

  .explore-footer {
    text-align: center;
    margin-top: 32px;
  }

  .view-all-btn {
    display: inline-block;
    padding: 8px 24px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--accent);
    font-size: 14px;
    font-weight: 500;
    text-decoration: none;
  }
  .view-all-btn:hover {
    background: var(--bg-hover);
    text-decoration: none;
  }
</style>
