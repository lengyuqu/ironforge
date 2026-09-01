<script lang="ts">
  import { createT } from '$lib/i18n';
  import type { WikiPageSummary } from '$lib/types/entities';

  const t = createT();

  let {
    owner,
    repo,
    pages,
    currentTitle,
    toc = [],
    onScrollToHeading,
  }: {
    owner: string;
    repo: string;
    pages: WikiPageSummary[];
    currentTitle: string;
    toc?: { id: string; text: string; level: number }[];
    onScrollToHeading: (id: string) => void;
  } = $props();
</script>

<aside class="wiki-sidebar">
  <div class="sidebar-section">
    <h3>{t('wiki.pages')}</h3>
    <nav class="page-nav">
      {#each pages as p}
        <a href={`/${owner}/${repo}/wiki/${encodeURIComponent(p.title)}`} class="page-link" class:active={p.title === currentTitle}>
          <span class="page-icon">📄</span>
          <span>{p.title}</span>
        </a>
      {/each}
    </nav>
  </div>

  {#if toc.length > 0}
    <div class="sidebar-section">
      <h3>{t('wiki.toc') || 'Table of Contents'}</h3>
      <nav class="toc-nav">
        {#each toc as heading}
          <button
            class="toc-link"
            style="padding-left: {heading.level * 12}px"
            onclick={() => onScrollToHeading(heading.id)}
          >
            {heading.text}
          </button>
        {/each}
      </nav>
    </div>
  {/if}
</aside>

<style>
  .wiki-sidebar {
    position: sticky;
    top: 24px;
  }
  .sidebar-section {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px;
    margin-bottom: 12px;
  }
  .sidebar-section h3 {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
    margin: 0 0 8px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--border-light);
  }

  .page-nav { display: flex; flex-direction: column; gap: 2px; max-height: 300px; overflow-y: auto; }
  .page-link {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border-radius: 4px;
    font-size: 13px;
    text-decoration: none;
    color: var(--text-primary);
  }
  .page-link:hover { background: var(--bg-hover); }
  .page-link.active { background: var(--bg-tertiary); font-weight: 600; }
  .page-icon { font-size: 12px; }

  .toc-nav { display: flex; flex-direction: column; gap: 2px; max-height: 400px; overflow-y: auto; }
  .toc-link {
    display: block;
    padding: 3px 8px;
    border-radius: 3px;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
    text-align: left;
    background: none;
    border: none;
    width: 100%;
  }
  .toc-link:hover { color: var(--accent); background: var(--bg-hover); }
</style>
