<script lang="ts">
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { createT, formatDate } from '$lib/i18n';
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { buildBlobHref, buildHistoryHref } from '$lib/utils/repoUrls';

  const t = createT();
  
  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  // Optional filters from the URL: ?ref=<branch|tag|sha>&path=<file or directory>.
  // Without a ref the server resolves HEAD (the default branch).
  let refName = $derived($page.url.searchParams.get('ref') || '');
  let filterPath = $derived($page.url.searchParams.get('path') || '');
  
  let commits = $state<any[]>([]);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    if (!owner || !repo) return;

    loading = true;
    error = '';
    
    repos.log(owner, repo, refName || undefined, filterPath || undefined)
      .then(r => {
        if (r && r.commits && Array.isArray(r.commits)) {
          commits = r.commits;
        } else {
          error = 'Invalid response format from server';
        }
      })
      .catch((e: any) => {
        error = e.message || t('errors.load_failed');
      })
      .finally(() => {
        loading = false;
      });
  });
</script>

<svelte:head>
  <title>{owner}/{repo} · {t('repo.tabs.commits')} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="commits" />

  {#if filterPath}
    <h1>{t('repo.commits_path_title', { path: filterPath })}</h1>
    <p class="filter-links">
      <a href={buildBlobHref(owner, repo, refName, filterPath)}>{filterPath}</a>
      <span aria-hidden="true">·</span>
      <a href={buildHistoryHref(owner, repo, refName, '')}>{t('repo.commits_clear_filter')}</a>
    </p>
  {:else}
    <h1>{t('repo.tabs.commits')}</h1>
  {/if}
  
  {#if error}
    <ErrorBanner message={error}>
      <button class="btn btn-sm" onclick={() => window.location.reload()}>{t('common.retry')}</button>
    </ErrorBanner>
  {/if}

  {#if loading}
    <LoadingState />
  {:else if commits.length === 0}
    <EmptyState title={filterPath ? t('repo.commits_path_empty') : t('repo.commits_empty', 'No commits yet.')} />
  {:else}
    <div class="commits-list">
      {#each commits as commit}
        <div class="commit-item">
          <div class="commit-icon">📝</div>
          <div class="commit-body">
            <div class="commit-message">
              <a href={`/${owner}/${repo}/commits/${commit.sha}`}>{commit.message}</a>
            </div>
            <div class="commit-meta">
              <span class="commit-author">{commit.author}</span>
              <span class="commit-date">{formatDate(commit.date)}</span>
              <span class="commit-sha">{commit.sha.substring(0, 7)}</span>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  h1 {
    font-size: 24px;
    margin-bottom: 16px;
  }

  .filter-links {
    display: flex;
    gap: 8px;
    margin: -8px 0 16px;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .commits-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .commit-item {
    display: flex;
    gap: 12px;
    padding: 12px 16px;
    border: 1px solid var(--border);
    border-radius: 6px;
    transition: border-color 0.15s;
  }

  .commit-item:hover {
    border-color: var(--primary);
  }

  .commit-icon {
    font-size: 24px;
    flex-shrink: 0;
  }

  .commit-body {
    flex: 1;
    min-width: 0;
  }

  .commit-message {
    font-weight: 500;
    margin-bottom: 4px;
  }

  .commit-message a {
    color: inherit;
    text-decoration: none;
  }

  .commit-message a:hover {
    color: var(--primary);
  }

  .commit-meta {
    display: flex;
    gap: 12px;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .commit-sha {
    font-family: monospace;
    background: var(--bg-secondary, #f5f5f5);
    padding: 2px 6px;
    border-radius: 3px;
    font-size: 12px;
  }
</style>
