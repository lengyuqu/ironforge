<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let queryRef = $derived($page.url.searchParams.get('ref') || '');
  let ref = $state('');
  let commits = $state<any[]>([]);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    if (ref !== queryRef) {
      ref = queryRef;
    }
    loadCommits();
  });

  async function loadCommits() {
    loading = true;
    error = '';
    commits = [];
    try {
      const logResult = await repos.log(owner!, repo!, ref || undefined);
      commits = logResult.commits || [];
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function commitHref(sha: string) {
    return `/${owner}/${repo}/commits/${sha}${ref ? `?ref=${encodeURIComponent(ref)}` : ''}`;
  }

  function repoHref() {
    return `/${owner}/${repo}${ref ? `?ref=${encodeURIComponent(ref)}` : ''}`;
  }
</script>

<div class="page-container">
  <RepoHeader owner={owner!} repo={repo!} activeTab="commits" />

  <div class="commits-page-header">
    <div>
      <h1>{t('repo.tabs.commits')}</h1>
      <p class="muted">
        {ref ? `Viewing ${ref}` : 'Viewing default branch'}
        <a href={repoHref()} class="jump-link">(browse files)</a>
      </p>
    </div>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {:else if loading}
    <p class="loading-text">{t('common.loading')}</p>
  {:else if commits.length === 0}
    <div class="empty">No commits found.</div>
  {:else}
    <div class="commit-list">
      {#each commits as commit}
        <a href={commitHref(commit.sha)} class="commit-card">
          <div class="commit-message">{commit.message}</div>
          <div class="commit-meta">
            <span>{commit.author}</span>
            <span>·</span>
            <time datetime={commit.date}>{formatDate(commit.date)}</time>
            <span class="sha">{commit.sha?.slice(0, 7)}</span>
          </div>
        </a>
      {/each}
    </div>
  {/if}
</div>

<style>
  .commits-page-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 20px;
  }

  h1 {
    margin: 0 0 6px;
  }

  .muted {
    margin: 0;
    color: var(--text-secondary);
    font-size: 14px;
  }

  .jump-link {
    margin-left: 8px;
    color: var(--accent);
    text-decoration: none;
  }

  .jump-link:hover {
    text-decoration: underline;
  }

  .loading-text {
    color: var(--text-secondary);
    text-align: center;
    padding: 32px;
  }

  .empty {
    text-align: center;
    color: var(--text-secondary);
    padding: 40px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-secondary);
  }

  .commit-list {
    display: grid;
    gap: 8px;
  }

  .commit-card {
    display: block;
    padding: 12px 14px;
    border: 1px solid var(--border);
    background: var(--bg-secondary);
    border-radius: var(--radius);
    color: inherit;
    text-decoration: none;
  }

  .commit-card:hover {
    background: var(--bg-hover);
    border-color: var(--accent);
  }

  .commit-message {
    font-weight: 500;
    margin-bottom: 6px;
  }

  .commit-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .sha {
    color: var(--accent);
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  }
</style>
