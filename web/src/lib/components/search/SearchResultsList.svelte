<script lang="ts">
  // Search results list — renders repo / issue / wiki result cards with query
  // highlighting, plus the pagination bar. Presentational only.
  import { createT } from '$lib/i18n';
  import { highlightText } from '$lib/utils/search';
  import type { SearchResult } from '$lib/api/search';

  interface Props {
    results: SearchResult[];
    query: string;
    total: number;
    currentPage: number;
    totalPages: number;
    onPageChange: (page: number) => void;
  }

  let {
    results,
    query,
    total,
    currentPage,
    totalPages,
    onPageChange,
  }: Props = $props();

  const t = createT();

  let hasNext = $derived(currentPage < totalPages);
  let hasPrev = $derived(currentPage > 1);

  // Get issue state badge color
  function getStateBadgeClass(state: string | null | undefined): string {
    if (!state) return '';
    if (state === 'open') return 'state-badge-open';
    if (state === 'closed') return 'state-badge-closed';
    if (state === 'merged') return 'state-badge-merged';
    return '';
  }

  function stateLabel(state: string | null | undefined): string {
    return state ? t(`issues.state.${state}`, state) : '';
  }
</script>

{#each results as result (result.result_type + '-' + result.id)}
  {#if result.result_type === 'repo'}
    <a href={`/${result.repo_owner || ''}/${result.repo_name || result.title || ''}`} class="result-card gh-list-item">
      <div class="result-body">
        <div class="repo-name-row">
          <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor" class="type-icon">
            <path d="M2 2.5A2.5 2.5 0 0 1 4.5 0h8.75a.75.75 0 0 1 .75.75v12.5a.75.75 0 0 1-.75.75h-2.5a.75.75 0 0 1 0-1.5h1.75v-2h-8a1 1 0 0 0-.714 1.7.75.75 0 1 1-1.072 1.05A2.495 2.495 0 0 1 2 11.5Zm10.5-1h-8a1 1 0 0 0-1 1v6.708A2.486 2.486 0 0 1 4.5 9h8ZM5 12.25a.25.25 0 0 1 .25-.25h3.5a.25.25 0 0 1 .25.25v3.25a.25.25 0 0 1-.4.2l-1.45-1.087a.25.25 0 0 0-.3 0L5.4 15.7a.25.25 0 0 1-.4-.2Z"/>
          </svg>
          <span class="result-title">{result.repo_owner}/{result.repo_name || result.title}</span>
          <span class="result-kind">{t('search.repo_result')}</span>
        </div>
        {#if result.excerpt}
          <div class="result-excerpt">{@html highlightText(result.excerpt || '', query)}</div>
        {/if}
      </div>
    </a>
  {:else if result.result_type === 'issue'}
    <a href={`/${result.repo_owner}/${result.repo_name}/issues/${result.number}`} class="result-card gh-list-item">
      <div class="result-body">
        <div class="issue-header">
          {#if result.state}
            <span class="issue-state-badge {getStateBadgeClass(result.state)}">{stateLabel(result.state)}</span>
          {/if}
          <span class="issue-badge">#{result.number}</span>
          <span class="result-title">{@html highlightText(result.title || '', query)}</span>
        </div>
        <div class="result-meta">
          <span class="repo-path">{result.repo_owner}/{result.repo_name}</span>
        </div>
        {#if result.excerpt}
          <div class="result-excerpt">{@html highlightText(result.excerpt || '', query)}</div>
        {/if}
      </div>
    </a>
  {:else if result.result_type === 'wiki'}
    <a href={`/${result.repo_owner}/${result.repo_name}/wiki/${encodeURIComponent(result.title)}`} class="result-card gh-list-item">
      <div class="result-body">
        <div class="wiki-header">
          <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor" class="type-icon">
            <path d="M0 1.75A.75.75 0 0 1 .75 1h4.253c1.227 0 2.317.59 3 1.501A3.743 3.743 0 0 1 11.006 1h4.245a.75.75 0 0 1 .75.75v10.5a.75.75 0 0 1-.75.75h-4.507a2.25 2.25 0 0 0-1.591.659l-.622.621a.75.75 0 0 1-1.06 0l-.622-.621A2.25 2.25 0 0 0 5.258 13H.75a.75.75 0 0 1-.75-.75Zm7.251 10.324.004-7.073-.002.003A2.25 2.25 0 0 0 5.003 4.5H1.5v7.5h3.757a3.75 3.75 0 0 1 1.994.574Zm.004-8.073-.001.002-.003.002V12.7A3.75 3.75 0 0 1 12.493 12H14.5V4.5h-3.497a2.25 2.25 0 0 0-2.244 2.5Zm-1.504 8.073H1.5v1h3.757a3.75 3.75 0 0 1 1.994.574v-1.574Zm8.254-8.073H14.5v1h-3.497a2.25 2.25 0 0 0-2.244 2.5V4.5Z"/>
          </svg>
          <span class="result-title">{@html highlightText(result.title || '', query)}</span>
        </div>
        <div class="result-meta">
          <span class="repo-path">{result.repo_owner}/{result.repo_name}</span>
        </div>
        {#if result.excerpt}
          <div class="result-excerpt">{@html highlightText(result.excerpt || '', query)}</div>
        {/if}
      </div>
    </a>
  {/if}
{/each}

{#if totalPages > 1}
  <div class="pagination">
    <button class="page-btn" disabled={!hasPrev} onclick={() => onPageChange(currentPage - 1)}>&larr; {t('common.previous')}</button>
    <span class="page-info">{t('search.page_info', { page: currentPage, total: totalPages })}</span>
    <button class="page-btn" disabled={!hasNext} onclick={() => onPageChange(currentPage + 1)}>{t('common.next')} &rarr;</button>
  </div>
{/if}

<style>
  .result-card {
    padding: 14px 16px;
    color: var(--text-primary);
    text-decoration: none;
  }

  .result-card:hover {
    background: var(--bg-hover);
    text-decoration: none;
  }

  .result-body {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .type-icon {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .repo-name-row,
  .issue-header,
  .wiki-header {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .result-title {
    font-weight: 600;
    font-size: 15px;
    color: var(--accent);
  }

  .result-kind {
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    font-size: 11px;
    font-weight: 500;
    line-height: 18px;
    padding: 0 7px;
    margin-left: auto;
  }

  .issue-state-badge {
    display: inline-flex;
    align-items: center;
    padding: 2px 8px;
    border-radius: 12px;
    font-size: 12px;
    font-weight: 600;
    flex-shrink: 0;
  }

  .state-badge-open {
    background: #2da44e20;
    color: #2da44e;
    border: 1px solid #2da44e40;
  }

  .state-badge-closed {
    background: #8250df20;
    color: #8250df;
    border: 1px solid #8250df40;
  }

  .state-badge-merged {
    background: #8250df20;
    color: #8250df;
    border: 1px solid #8250df40;
  }

  .issue-badge {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .result-meta {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .repo-path {
    font-size: 13px;
    color: var(--text-secondary);
  }

  .result-excerpt {
    font-size: 13px;
    color: var(--text-muted);
    margin-top: 2px;
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    line-clamp: 2;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
    padding: 24px 0;
  }

  .page-btn {
    padding: 6px 14px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 13px;
    cursor: pointer;
  }

  .page-btn:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .page-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .page-info {
    font-size: 13px;
    color: var(--text-muted);
  }

  :global(.search-highlight) {
    background: #fef08a;
    color: #1a1a2e;
    padding: 1px 3px;
    border-radius: 2px;
    font-weight: 600;
  }
</style>
