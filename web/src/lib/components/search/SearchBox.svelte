<script lang="ts">
  // Search box — input + help panel toggle + result-type tabs.
  // Pure presentational: state changes bubble up via callbacks.
  import { createT } from '$lib/i18n';

  interface Props {
    query: string;
    /** Called with the raw input value on every keystroke (bind-able). */
    onQueryChange: (value: string) => void;
    onSearch: () => void;
    activeType: string;
    onTypeChange: (type: string) => void;
  }

  let { query, onQueryChange, onSearch, activeType, onTypeChange }: Props = $props();

  const t = createT();

  let showHelp = $state(false);

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      onSearch();
    }
  }

  function toggleHelp() {
    showHelp = !showHelp;
  }
</script>

<div class="search-box">
  <div class="search-input-wrapper">
    <svg class="search-icon" viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
      <path d="M11.5 7a4.5 4.5 0 1 1-9 0 4.5 4.5 0 0 1 9 0Zm-.82 4.74a6 6 0 1 1 1.06-1.06l3.04 3.04a.75.75 0 1 1-1.06 1.06l-3.04-3.04Z"/>
    </svg>
    <input
      type="text"
      class="search-input"
      value={query}
      oninput={(e) => onQueryChange((e.target as HTMLInputElement).value)}
      onkeydown={handleKeydown}
      placeholder={t('search.placeholder')}
    />
    <button class="search-btn btn btn-primary" onclick={onSearch}>{t('search.search_button')}</button>
  </div>
  <button class="help-btn" onclick={toggleHelp} title={t('search.help_title', 'Search help')}>
    ?
  </button>
</div>

{#if showHelp}
  <div class="search-help">
    <h3>{t('search.help_title', 'Search Tips')}</h3>
    <p>{t('search.help_desc', 'Use qualifiers to refine your search:')}</p>
    <div class="help-qualifiers">
      <div class="help-item">
        <code>repo:owner/name</code>
        <span>{t('search.help_repo', 'Search in a specific repository')}</span>
      </div>
      <div class="help-item">
        <code>author:username</code>
        <span>{t('search.help_author', 'Search by author')}</span>
      </div>
      <div class="help-item">
        <code>state:open|closed|all</code>
        <span>{t('search.help_state', 'Filter by issue state')}</span>
      </div>
      <div class="help-item">
        <code>label:name</code>
        <span>{t('search.help_label', 'Filter by label')}</span>
      </div>
    </div>
    <p class="help-tip">{t('search.help_tip', 'Example: bug fix repo:owner/name state:open')}</p>
  </div>
{/if}

<div class="type-tabs">
  {#each [
    { key: 'all', label: t('search.all') },
    { key: 'repos', label: t('search.repos') },
    { key: 'issues', label: t('search.issues') },
    { key: 'wiki', label: t('search.wiki') }
  ] as tab (tab.key)}
    <button
      class="type-tab"
      class:active={activeType === tab.key}
      onclick={() => onTypeChange(tab.key)}
    >
      {tab.label}
    </button>
  {/each}
</div>

<style>
  .search-box {
    display: flex;
    align-items: center;
    margin-bottom: 16px;
  }

  .search-input-wrapper {
    display: flex;
    align-items: center;
    gap: 0;
    flex: 1;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .search-input-wrapper:focus-within {
    border-color: var(--accent);
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
    border-radius: 0 var(--radius) var(--radius) 0;
  }

  .help-btn {
    width: 36px;
    height: 36px;
    margin-left: 8px;
    flex-shrink: 0;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 50%;
    color: var(--text-secondary);
    font-size: 16px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .help-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
    border-color: var(--accent);
  }

  .search-help {
    margin-top: 0;
    margin-bottom: 16px;
    padding: 16px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 13px;
    color: var(--text-secondary);
  }

  .search-help h3 {
    font-size: 14px;
    margin-bottom: 8px;
    color: var(--text-primary);
  }

  .search-help p {
    margin-bottom: 10px;
  }

  .help-qualifiers {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 8px;
    margin-bottom: 10px;
  }

  .help-item {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .help-item code {
    background: var(--bg-primary);
    padding: 2px 6px;
    border-radius: 4px;
    font-size: 12px;
    color: var(--accent);
    flex-shrink: 0;
  }

  .help-item span {
    font-size: 12px;
  }

  .help-tip {
    font-style: italic;
  }

  .type-tabs {
    display: flex;
    gap: 4px;
    margin-top: 16px;
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
  }

  .type-tab:hover {
    color: var(--text-primary);
  }

  .type-tab.active {
    color: var(--text-primary);
    font-weight: 600;
    border-bottom-color: var(--accent);
  }
</style>
