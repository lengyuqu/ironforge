<script lang="ts">
  // Search page — orchestrator. Syncs query/type/page with the URL, performs
  // the search request and delegates UI to:
  //   SearchBox (input + help + type tabs)
  //   SearchResultsList (result cards + pagination)
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { createT } from '$lib/i18n';
  import { search } from '$lib/api/client.svelte';
  import type { SearchResult } from '$lib/api/search';
  import { toErrorMessage } from '$lib/utils/error';
  import SearchBox from '$lib/components/search/SearchBox.svelte';
  import SearchResultsList from '$lib/components/search/SearchResultsList.svelte';
  import { onMount } from 'svelte';

  const t = createT();

  let query = $state('');
  let activeType = $state('all');
  let loading = $state(false);
  let results = $state<SearchResult[]>([]);
  let total = $state(0);
  let currentPage = $state(1);
  let searchError = $state('');
  let perPage = 20;
  let hasSearched = $state(false);

  let totalPages = $derived(Math.ceil(total / perPage) || 1);

  function normalizeSearchType(type: string | null): string {
    if (type === 'repo') return 'repos';
    if (type === 'issue') return 'issues';
    if (type === 'repos' || type === 'issues' || type === 'wiki' || type === 'all') return type;
    return 'all';
  }

  // Sync from URL on mount and on URL changes
  $effect(() => {
    const url = $page.url;
    const q = url.searchParams.get('q') || '';
    const type = normalizeSearchType(url.searchParams.get('type'));
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

  // Keyboard shortcut: Ctrl+K or Cmd+K to focus search
  onMount(() => {
    function handleKeyboard(e: KeyboardEvent) {
      if ((e.ctrlKey || e.metaKey) && e.key === 'k') {
        e.preventDefault();
        const input = document.querySelector('.search-input') as HTMLInputElement;
        if (input) input.focus();
      }
    }
    window.addEventListener('keydown', handleKeyboard);
    return () => window.removeEventListener('keydown', handleKeyboard);
  });

  async function performSearch(q: string, type: string, pg: number) {
    try {
      loading = true;
      hasSearched = true;
      searchError = '';
      const response = await search.search(q, type, pg, perPage);
      results = response.results;
      total = response.total;
      currentPage = response.page;
    } catch (err: unknown) {
      results = [];
      total = 0;
      searchError = toErrorMessage(err, t('search.load_failed'));
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

  function keyboardHintParts() {
    const hint = t('search.keyboard_hint', 'Tip: Press Ctrl+K to focus search');
    const shortcut = 'Ctrl+K';
    const index = hint.indexOf(shortcut);

    if (index === -1) {
      return { before: hint, shortcut: '', after: '' };
    }

    return {
      before: hint.slice(0, index),
      shortcut,
      after: hint.slice(index + shortcut.length)
    };
  }
</script>

<svelte:head>
  <title>{query ? `${query} · ` : ''}{t('search.title')} · IronForge</title>
</svelte:head>

<div class="page-container search-page">
  <div class="search-header">
    <h1>{t('search.title')}</h1>
    <SearchBox
      {query}
      onQueryChange={(value) => (query = value)}
      onSearch={doSearch}
      activeType={activeType}
      onTypeChange={setType}
    />
  </div>

  <div class="search-body">
    {#if loading}
      <LoadingState />
    {:else if searchError}
      <ErrorBanner message={searchError} />
    {:else if !hasSearched}
      <EmptyState icon="🔍" title={t('search.placeholder')} />
    {:else if results.length === 0}
      <EmptyState title={t('search.no_results')} />
    {:else}
      <div class="results-info">
        {t('search.results_count', { total })}
      </div>
      <div class="results-list gh-list">
        <SearchResultsList
          {results}
          {query}
          {total}
          currentPage={currentPage}
          totalPages={totalPages}
          onPageChange={goPage}
        />
      </div>
    {/if}
  </div>

  <div class="keyboard-hint">
    {keyboardHintParts().before}{#if keyboardHintParts().shortcut}<kbd>{keyboardHintParts().shortcut}</kbd>{/if}{keyboardHintParts().after}
  </div>
</div>

<style>
  .search-header {
    margin-bottom: 24px;
  }

  h1 {
    font-size: 24px;
    margin-bottom: 20px;
  }

  .search-body {
    min-height: 200px;
  }

  .results-info {
    font-size: 13px;
    color: var(--text-muted);
    margin-bottom: 12px;
  }

  .results-list {
    margin-bottom: 16px;
  }

  .keyboard-hint {
    margin-top: 32px;
    text-align: center;
    font-size: 12px;
    color: var(--text-muted);
  }

  .keyboard-hint kbd {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 1px 5px;
    font-family: var(--font-mono);
    font-size: 11px;
  }
</style>
