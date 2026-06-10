<script lang="ts">
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { createT } from '$lib/i18n';
  import { search, type SearchResult } from '$lib/api/client';

  const t = createT();

  let query = $state('');
  let activeType = $state('all');
  let loading = $state(false);
  let results = $state<SearchResult[]>([]);
  let total = $state(0);
  let currentPage = $state(1);
  let perPage = 20;
  let hasSearched = $state(false);

  let totalPages = $derived(Math.ceil(total / perPage) || 1);
  let hasNext = $derived(currentPage < totalPages);
  let hasPrev = $derived(currentPage > 1);

  // Sync from URL on mount and on URL changes
  $effect(() => {
    const url = $page.url;
    const q = url.searchParams.get('q') || '';
    const type = url.searchParams.get('type') || 'all';
    const pg = parseInt(url.searchParams.get('page') || '1', 10);

    if (q !== query || type !== activeType || pg !== currentPage) {
      query = q;
      activeType = type;
      currentPage = pg;
      if (q) {
        performSearch(q, type, pg);
      }
    }
  });

  async function performSearch(q: string, type: string, pg: number) {
    try {
      loading = true;
      hasSearched = true;
      const response = await search.search(q, type, pg, perPage);
      results = response.results;
      total = response.total;
      currentPage = response.page;
    } catch (err: any) {
      results = [];
      total = 0;
    } finally {
      loading = false;
    }
  }

  function doSearch() {
    if (!query.trim()) return;
    currentPage = 1;
    goto(`/search?q=${encodeURIComponent(query.trim())}&type=${activeType}`);
  }

  function setType(type: string) {
    activeType = type;
    currentPage = 1;
    if (query.trim()) {
      goto(`/search?q=${encodeURIComponent(query.trim())}&type=${type}`);
    }
  }

  function goPage(pg: number) {
    if (pg < 1 || pg > totalPages) return;
    goto(`/search?q=${encodeURIComponent(query.trim())}&type=${activeType}&page=${pg}`);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      doSearch();
    }
  }
</script>

<div class="search-page">
  <div class="search-header">
    <h1>{t('search.title')}</h1>
    <div class="search-box">
      <div class="search-input-wrapper">
        <svg class="search-icon" viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
          <path d="M11.5 7a4.5 4.5 0 1 1-9 0 4.5 4.5 0 0 1 9 0Zm-.82 4.74a6 6 0 1 1 1.06-1.06l3.04 3.04a.75.75 0 1 1-1.06 1.06l-3.04-3.04Z"/>
        </svg>
        <input
          type="text"
          class="search-input"
          bind:value={query}
          onkeydown={handleKeydown}
          placeholder={t('search.placeholder')}
        />
        <button class="search-btn" onclick={doSearch}>{t('search.search_button')}</button>
      </div>
    </div>

    <div class="type-tabs">
      {#each [
        { key: 'all', label: t('search.all') },
        { key: 'repo', label: t('search.repos') },
        { key: 'issue', label: t('search.issues') },
        { key: 'wiki', label: t('search.wiki') }
      ] as tab}
        <button
          class="type-tab"
          class:active={activeType === tab.key}
          onclick={() => setType(tab.key)}
        >
          {tab.label}
        </button>
      {/each}
    </div>
  </div>

  <div class="search-body">
    {#if loading}
      <div class="loading">
        <div class="spinner"></div>
        <span>{t('common.loading')}</span>
      </div>
    {:else if !hasSearched}
      <div class="empty-state">
        <svg class="empty-icon" viewBox="0 0 16 16" width="48" height="48" fill="currentColor">
          <path d="M11.5 7a4.5 4.5 0 1 1-9 0 4.5 4.5 0 0 1 9 0Zm-.82 4.74a6 6 0 1 1 1.06-1.06l3.04 3.04a.75.75 0 1 1-1.06 1.06l-3.04-3.04Z"/>
        </svg>
        <p>{t('search.placeholder')}</p>
      </div>
    {:else if results.length === 0}
      <div class="empty-state">
        <p>{t('search.no_results')}</p>
      </div>
    {:else}
      <div class="results-info">
        {t('search.results_count', { total })}
      </div>
      <div class="results-list">
        {#each results as result (result.result_type + '-' + result.id)}
          {#if result.result_type === 'repo'}
            <a href="/{result.repo_owner}/{result.repo_name}" class="result-card repo-card">
              <div class="result-body">
                <div class="repo-name-row">
                  <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor" class="type-icon">
                    <path d="M2 2.5A2.5 2.5 0 0 1 4.5 0h8.75a.75.75 0 0 1 .75.75v12.5a.75.75 0 0 1-.75.75h-2.5a.75.75 0 0 1 0-1.5h1.75v-2h-8a1 1 0 0 0-.714 1.7.75.75 0 1 1-1.072 1.05A2.495 2.495 0 0 1 2 11.5Zm10.5-1h-8a1 1 0 0 0-1 1v6.708A2.486 2.486 0 0 1 4.5 9h8ZM5 12.25a.25.25 0 0 1 .25-.25h3.5a.25.25 0 0 1 .25.25v3.25a.25.25 0 0 1-.4.2l-1.45-1.087a.25.25 0 0 0-.3 0L5.4 15.7a.25.25 0 0 1-.4-.2Z"/>
                  </svg>
                  <span class="result-title">{result.repo_owner}/{result.repo_name}</span>
                  <span class="star-icon">&#9733;</span>
                </div>
                {#if result.title}
                  <div class="result-desc">{result.title}</div>
                {/if}
                {#if result.excerpt}
                  <div class="result-excerpt">{result.excerpt}</div>
                {/if}
              </div>
            </a>
          {:else if result.result_type === 'issue'}
            <a href="/{result.repo_owner}/{result.repo_name}/issues/{result.id}" class="result-card issue-card">
              <div class="result-body">
                <div class="issue-header">
                  <span class="issue-badge">#{result.id}</span>
                  <span class="result-title">{result.title}</span>
                </div>
                <div class="result-meta">
                  <span class="repo-path">{result.repo_owner}/{result.repo_name}</span>
                </div>
                {#if result.excerpt}
                  <div class="result-excerpt">{result.excerpt}</div>
                {/if}
              </div>
            </a>
          {:else if result.result_type === 'wiki'}
            <a href="/{result.repo_owner}/{result.repo_name}/wiki/{encodeURIComponent(result.title)}" class="result-card wiki-card">
              <div class="result-body">
                <div class="wiki-header">
                  <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor" class="type-icon">
                    <path d="M0 1.75A.75.75 0 0 1 .75 1h4.253c1.227 0 2.317.59 3 1.501A3.743 3.743 0 0 1 11.006 1h4.245a.75.75 0 0 1 .75.75v10.5a.75.75 0 0 1-.75.75h-4.507a2.25 2.25 0 0 0-1.591.659l-.622.621a.75.75 0 0 1-1.06 0l-.622-.621A2.25 2.25 0 0 0 5.258 13H.75a.75.75 0 0 1-.75-.75Zm7.251 10.324.004-7.073-.002.003A2.25 2.25 0 0 0 5.003 4.5H1.5v7.5h3.757a3.75 3.75 0 0 1 1.994.574Zm.004-8.073-.001.002-.003.002V12.7A3.75 3.75 0 0 1 12.493 12H14.5V4.5h-3.497a2.25 2.25 0 0 0-2.244 2.5Zm-1.504 8.073H1.5v1h3.757a3.75 3.75 0 0 1 1.994.574v-1.574Zm8.254-8.073H14.5v1h-3.497a2.25 2.25 0 0 0-2.244 2.5V4.5Z"/>
                  </svg>
                  <span class="result-title">{result.title}</span>
                </div>
                <div class="result-meta">
                  <span class="repo-path">{result.repo_owner}/{result.repo_name}</span>
                </div>
                {#if result.excerpt}
                  <div class="result-excerpt">{result.excerpt}</div>
                {/if}
              </div>
            </a>
          {/if}
        {/each}
      </div>

      {#if totalPages > 1}
        <div class="pagination">
          <button class="page-btn" disabled={!hasPrev} onclick={() => goPage(currentPage - 1)}>&larr; Prev</button>
          <span class="page-info">Page {currentPage} of {totalPages}</span>
          <button class="page-btn" disabled={!hasNext} onclick={() => goPage(currentPage + 1)}>Next &rarr;</button>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .search-page {
    max-width: 800px;
    margin: 0 auto;
    padding: 32px 24px;
  }

  .search-header {
    margin-bottom: 24px;
  }

  h1 {
    font-size: 24px;
    margin-bottom: 20px;
  }

  .search-box {
    margin-bottom: 16px;
  }

  .search-input-wrapper {
    display: flex;
    align-items: center;
    gap: 0;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .search-input-wrapper:focus-within {
    border-color: var(--orange);
  }

  .search-icon {
    flex-shrink: 0;
    margin-left: 12px;
    color: var(--text-muted);
  }

  .search-input {
    flex: 1;
    background: none;
    border: none;
    outline: none;
    padding: 12px;
    font-size: 15px;
    color: var(--text-primary);
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .search-btn {
    flex-shrink: 0;
    padding: 10px 20px;
    background: var(--orange);
    color: #fff;
    border: none;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    border-radius: 0 var(--radius) var(--radius) 0;
  }

  .search-btn:hover {
    opacity: 0.9;
  }

  .type-tabs {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--border);
  }

  .type-tab {
    padding: 8px 16px;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--text-secondary);
    font-size: 14px;
    cursor: pointer;
    transition: color 0.15s, border-color 0.15s;
  }

  .type-tab:hover {
    color: var(--text-primary);
  }

  .type-tab.active {
    color: var(--orange);
    border-bottom-color: var(--orange);
    font-weight: 600;
  }

  .search-body {
    min-height: 200px;
  }

  .loading {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 40px 0;
    color: var(--text-secondary);
    justify-content: center;
  }

  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid var(--border);
    border-top-color: var(--orange);
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .empty-state {
    text-align: center;
    padding: 60px 24px;
    color: var(--text-secondary);
  }

  .empty-icon {
    color: var(--text-muted);
    margin-bottom: 16px;
  }

  .empty-state p {
    font-size: 15px;
  }

  .results-info {
    font-size: 13px;
    color: var(--text-muted);
    margin-bottom: 12px;
  }

  .results-list {
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  .result-card {
    display: block;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px 20px;
    margin-bottom: 8px;
    text-decoration: none;
    color: var(--text-primary);
    transition: border-color 0.15s;
  }

  .result-card:hover {
    border-color: var(--accent);
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

  .star-icon {
    color: var(--orange);
    font-size: 14px;
    margin-left: auto;
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

  .result-desc {
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
</style>
