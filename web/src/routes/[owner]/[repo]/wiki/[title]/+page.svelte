<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { wiki } from '$lib/api/client.svelte';
  import { createT, formatDate } from '$lib/i18n';
  import { renderMarkdown } from '$lib/utils/markdown';
  import type { WikiPage, WikiPageSummary } from '$lib/types/entities';
  import WikiSidebar from '$lib/components/wiki/WikiSidebar.svelte';
  import WikiEditPanel from '$lib/components/wiki/WikiEditPanel.svelte';
  import WikiHistoryPanel from '$lib/components/wiki/WikiHistoryPanel.svelte';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let title = $derived($page.params.title!);
  let wikiPage = $state<WikiPage | null>(null);
  let allPages = $state<WikiPageSummary[]>([]);
  let loading = $state(true);
  let error = $state('');
  let editing = $state(false);
  let renderedHtml = $state('');
  let toc = $state<{ id: string; text: string; level: number }[]>([]);
  let showHistory = $state(false);

  $effect(() => { loadPage(); });

  async function loadPage() {
    try {
      loading = true;
      const [pageData, pages] = await Promise.all([
        wiki.get(owner, repo, title),
        wiki.list(owner, repo),
      ]);
      wikiPage = pageData;
      allPages = pages;
      renderContent(pageData.content);
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function renderContent(content: string) {
    if (!content) { renderedHtml = ''; toc = []; return; }

    // Extract headings for TOC before rendering
    const headingRegex = /^(#{1,6})\s+(.+)$/gm;
    const headings: { id: string; text: string; level: number }[] = [];
    let match;
    while ((match = headingRegex.exec(content)) !== null) {
      const text = match[2].trim();
      const id = text.toLowerCase().replace(/[^\w\u4e00-\u9fff]+/g, '-').replace(/^-|-$/g, '');
      headings.push({ id, text, level: match[1].length });
    }
    toc = headings;

    // Add IDs to headings in rendered HTML
    const html = renderMarkdown(content);
    // Inject IDs into heading tags
    let idIndex = 0;
    renderedHtml = html.replace(/<(h[1-6])>(.*?)<\/h[1-6]>/g, (_match, tag, text) => {
      const plainText = text.replace(/<[^>]*>/g, '');
      const id = headings[idIndex]?.id || plainText.toLowerCase().replace(/[^\w]+/g, '-');
      idIndex++;
      return `<${tag} id="${id}">${text}</${tag}>`;
    });
  }

  async function handleDelete() {
    if (!confirm('Delete this wiki page? This cannot be undone.')) return;
    try {
      await wiki.remove(owner, repo, title);
      window.location.href = `/${owner}/${repo}/wiki`;
    } catch (e: any) {
      error = e.message;
    }
  }

  function toggleHistory() {
    showHistory = !showHistory;
  }

  function scrollToHeading(id: string) {
    const el = document.getElementById(id);
    if (el) el.scrollIntoView({ behavior: 'smooth' });
  }
</script>

<svelte:head>
  <title>{title} · {owner}/{repo} Wiki · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="wiki" />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">{t('common.loading')}</p>
  {:else if wikiPage}
    <div class="wiki-layout">
      <WikiSidebar
        {owner}
        {repo}
        pages={allPages}
        currentTitle={title}
        {toc}
        onScrollToHeading={scrollToHeading}
      />

      <main class="wiki-main">
        <div class="wiki-header">
          <h1>{title}</h1>
          <div class="header-actions">
            {#if wikiPage.updated_at}
              <span class="text-secondary text-sm">Last edited {formatDate(wikiPage.updated_at)}</span>
            {/if}
            <button class="btn-outline" onclick={toggleHistory} class:active={showHistory}>History</button>
            <button class="btn-outline" onclick={() => editing = true}>{t('wiki.edit')}</button>
            <button class="btn-outline btn-danger" onclick={handleDelete}>{t('wiki.delete') || 'Delete'}</button>
          </div>
        </div>

        {#if showHistory}
          <WikiHistoryPanel {owner} {repo} {title} onRestored={loadPage} />
        {:else if editing}
          <WikiEditPanel
            {owner}
            {repo}
            {title}
            initialContent={wikiPage.content}
            onSaved={() => { editing = false; loadPage(); }}
            onCancel={() => editing = false}
          />
        {:else}
          <div class="wiki-content markdown-body">
            {@html renderedHtml}
          </div>
        {/if}

        <div class="wiki-footer">
          <a href={`/${owner}/${repo}/wiki`} class="back-link">← {t('wiki.back') || 'Back to Wiki'}</a>
        </div>
      </main>
    </div>
  {:else}
    <div class="empty"><p>{t('wiki.not_found') || 'Page not found'}</p></div>
  {/if}
</div>

<style>
  .empty { text-align: center; padding: 48px; color: var(--text-secondary); }
  .text-secondary { color: var(--text-secondary); }
  .error-banner {
    color: #f85149; background: rgba(248, 81, 73, 0.1);
    padding: 10px 12px; border-radius: 6px; margin-bottom: 16px;
  }

  /* ── Layout ── */
  .wiki-layout {
    display: grid;
    grid-template-columns: 240px 1fr;
    gap: 24px;
    align-items: start;
  }
  @media (max-width: 900px) { .wiki-layout { grid-template-columns: 1fr; } }

  /* ── Main ── */
  .wiki-main {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 24px;
    min-height: 400px;
  }

  .wiki-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 12px;
    margin-bottom: 20px;
    border-bottom: 1px solid var(--border);
    padding-bottom: 12px;
  }
  h1 { font-size: 24px; margin: 0; }
  .header-actions { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .text-sm { font-size: 12px; }

  .btn-outline {
    padding: 5px 12px; background: none; border: 1px solid var(--border);
    border-radius: var(--radius); color: var(--text-primary); font-size: 13px; cursor: pointer;
  }
  .btn-outline:hover { background: var(--bg-hover); }
  .btn-danger { border-color: var(--red-dim); color: var(--red); }
  .btn-danger:hover { background: rgba(248, 81, 73, 0.1); }
  .btn-outline.active { background: var(--bg-tertiary); border-color: var(--accent); color: var(--accent); }

  /* ── Markdown Content ── */
  :global(.wiki-main .markdown-body) {
    line-height: 1.7;
    font-size: 15px;
    color: var(--text-primary);
    overflow-x: auto;
  }
  :global(.wiki-main .markdown-body h1), :global(.wiki-main .markdown-body h2), :global(.wiki-main .markdown-body h3),
  :global(.wiki-main .markdown-body h4), :global(.wiki-main .markdown-body h5), :global(.wiki-main .markdown-body h6) {
    margin: 1.2em 0 0.6em;
    color: var(--text-primary);
    scroll-margin-top: 24px;
  }
  :global(.wiki-main .markdown-body h1) { font-size: 1.8em; border-bottom: 1px solid var(--border-light); padding-bottom: 6px; }
  :global(.wiki-main .markdown-body h2) { font-size: 1.5em; border-bottom: 1px solid var(--border-light); padding-bottom: 4px; }
  :global(.wiki-main .markdown-body h3) { font-size: 1.25em; }
  :global(.wiki-main .markdown-body p) { margin: 0 0 12px; }
  :global(.wiki-main .markdown-body a) { color: var(--accent); }
  :global(.wiki-main .markdown-body code) {
    background: var(--bg-tertiary); padding: 2px 6px; border-radius: 3px;
    font-size: 13px; font-family: var(--font-mono);
  }
  :global(.wiki-main .markdown-body pre) {
    background: #1a1a2e; color: #e0e0e0; padding: 14px; border-radius: 6px;
    overflow-x: auto; font-size: 13px; line-height: 1.5;
  }
  :global(.wiki-main .markdown-body pre code) { background: none; padding: 0; }
  :global(.wiki-main .markdown-body blockquote) {
    border-left: 4px solid var(--accent); padding: 4px 16px; margin: 12px 0;
    color: var(--text-secondary); background: var(--bg-primary); border-radius: 0 4px 4px 0;
  }
  :global(.wiki-main .markdown-body table) { border-collapse: collapse; width: 100%; margin: 12px 0; }
  :global(.wiki-main .markdown-body th), :global(.wiki-main .markdown-body td) {
    border: 1px solid var(--border); padding: 8px 12px; text-align: left; font-size: 14px;
  }
  :global(.wiki-main .markdown-body th) { background: var(--bg-tertiary); font-weight: 600; }
  :global(.wiki-main .markdown-body ul), :global(.wiki-main .markdown-body ol) { padding-left: 24px; margin: 8px 0; }
  :global(.wiki-main .markdown-body li) { margin: 4px 0; }
  :global(.wiki-main .markdown-body img) { max-width: 100%; border-radius: 4px; }
  :global(.wiki-main .markdown-body hr) { border: none; border-top: 1px solid var(--border); margin: 24px 0; }

  .wiki-footer { margin-top: 32px; padding-top: 16px; border-top: 1px solid var(--border-light); }
  .back-link { color: var(--accent); text-decoration: none; font-size: 14px; }
  .back-link:hover { text-decoration: underline; }
</style>
