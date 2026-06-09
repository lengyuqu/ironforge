<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { issues } from '$lib/api/client';
  import { createT } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  // Board columns
  const columns = [
    { id: 'open', label: t('board.columns.open') },
    { id: 'in_progress', label: t('board.columns.in_progress') },
    { id: 'closed', label: t('board.columns.closed') },
  ];

  // Issues grouped by column
  let boardIssues = $state<Record<string, any[]>>({
    open: [],
    in_progress: [],
    closed: [],
  });

  let loading = $state(true);
  let error = $state('');

  // Load issues for board
  async function loadBoard() {
    loading = true;
    error = '';
    try {
      const res = await issues.list(owner!, repo!, 'all', 1, 100);
      const allIssues = res.data || [];

      // Group by status (simplified - in real app, use labels or custom field)
      boardIssues = {
        open: allIssues.filter((i: any) => i.state === 'open'),
        in_progress: [], // Requires custom field or label
        closed: allIssues.filter((i: any) => i.state === 'closed'),
      };
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  // Initial load
  $effect(() => {
    loadBoard();
  });
</script>

<svelte:head>
  <title>Issue Board · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader owner={owner!} repo={repo!} activeTab="board" />

  <div class="page-header">
    <h1>{t('repo.tabs.board')}</h1>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="loading-text">{t('common.loading')}</p>
  {:else}
    <div class="board-container">
      {#each columns as col}
        <div class="board-column">
          <div class="column-header">
            <h2>{col.label}</h2>
            <span class="issue-count">{boardIssues[col.id].length}</span>
          </div>

          <div class="column-body">
            {#if boardIssues[col.id].length === 0}
              <p class="empty-column">{t('board.no_issues')}</p>
            {:else}
              {#each boardIssues[col.id] as issue (issue.id)}
                <div class="issue-card">
                  <a href="/{owner}/{repo}/issues/{issue.number}" class="issue-title">
                    {issue.title}
                  </a>
                  <div class="issue-meta">
                    <span class="issue-number">#{issue.number}</span>
                  </div>
                </div>
              {/each}
            {/if}
          </div>
        </div>
      {/each}
    </div>

    <p class="board-hint">{t('board.drag_hint')}</p>
  {/if}
</div>

<style>
  .repo-page {
    max-width: 1200px;
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

  .loading-text {
    color: var(--text-secondary);
    text-align: center;
    padding: 48px;
  }

  .board-container {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 16px;
    margin-bottom: 24px;
  }

  .board-column {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    display: flex;
    flex-direction: column;
    min-height: 400px;
  }

  .column-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }

  .column-header h2 {
    font-size: 14px;
    font-weight: 600;
    margin: 0;
  }

  .issue-count {
    background: var(--bg-tertiary);
    color: var(--text-muted);
    border-radius: 10px;
    font-size: 12px;
    font-weight: 600;
    padding: 2px 8px;
  }

  .column-body {
    padding: 8px;
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .empty-column {
    color: var(--text-muted);
    font-size: 13px;
    text-align: center;
    padding: 24px;
  }

  .issue-card {
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 12px;
    cursor: pointer;
    transition: border-color 0.15s ease;
  }
  .issue-card:hover {
    border-color: var(--text-muted);
  }

  .issue-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    text-decoration: none;
    display: block;
    margin-bottom: 6px;
  }
  .issue-title:hover {
    color: var(--accent);
  }

  .issue-meta {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .issue-number {
    font-size: 12px;
    color: var(--text-muted);
  }

  .board-hint {
    text-align: center;
    font-size: 12px;
    color: var(--text-muted);
  }

  @media (max-width: 768px) {
    .board-container {
      grid-template-columns: 1fr;
    }
  }
</style>
