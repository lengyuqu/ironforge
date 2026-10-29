<script lang="ts">
  // Landing public repositories section — vertical repo list fed by the
  // explore API, with loading / error / empty states.
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { createT, formatDate } from '$lib/i18n';
  import type { ExploreRepo } from '$lib/types/entities';

  interface Props {
    repos: ExploreRepo[];
    loading: boolean;
    error: string;
    onRetry: () => void;
  }

  let { repos, loading, error, onRetry }: Props = $props();

  const t = createT();
</script>

<section class="public-repos">
  <div class="repos-container">
    <div class="repos-header">
      <h2>{t('home.public_repos')}</h2>
      <a href="/explore" class="view-all">{t('home.explore.view_all')} →</a>
    </div>

    {#if error}
      <ErrorBanner message={error}>
        <button class="btn btn-sm" onclick={onRetry}>{t('common.retry')}</button>
      </ErrorBanner>
    {/if}

    {#if loading}
      <LoadingState />
    {:else if repos.length === 0}
      <EmptyState title={t('explore.empty')} />
    {:else}
      <div class="repo-list">
        {#each repos as repo (repo.id)}
          <a href={`/${repo.owner_name || 'unknown'}/${repo.name}`} class="repo-item">
            <div class="repo-icon">📂</div>
            <div class="repo-info">
              <div class="repo-name">
                {repo.owner_name || 'unknown'}/{repo.name}
              </div>
              <div class="repo-desc">{repo.description || t('common.no_description')}</div>
              <div class="repo-meta">
                ⭐ {repo.stars_count || 0} · {t('common.updated', { date: formatDate(repo.updated_at) })}
              </div>
            </div>
          </a>
        {/each}
      </div>
    {/if}
  </div>
</section>

<style>
  .public-repos {
    padding: 80px 24px;
    background: var(--bg-primary);
  }

  .repos-container {
    max-width: 1200px;
    margin: 0 auto;
  }

  .repos-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 24px;
  }

  .repos-header h2 {
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
  }

  .view-all {
    font-size: 14px;
    color: var(--green);
    text-decoration: none;
  }

  .view-all:hover {
    text-decoration: underline;
  }

  .repo-list {
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  .repo-item {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 16px 20px;
    border-bottom: 1px solid var(--border-light);
    text-decoration: none;
    color: var(--text-primary);
    transition: background 0.2s;
  }

  .repo-item:hover {
    background: var(--bg-secondary);
  }

  .repo-item:first-child {
    border-top: 1px solid var(--border-light);
    border-radius: 8px 8px 0 0;
  }

  .repo-item:last-child {
    border-radius: 0 0 8px 8px;
  }

  .repo-icon {
    font-size: 20px;
    margin-top: 2px;
  }

  .repo-info {
    flex: 1;
  }

  .repo-name {
    font-weight: 600;
    font-size: 15px;
    color: var(--accent);
    margin-bottom: 4px;
  }

  .repo-desc {
    font-size: 13px;
    color: var(--text-secondary);
    margin-top: 2px;
  }

  .repo-meta {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 4px;
  }

</style>
