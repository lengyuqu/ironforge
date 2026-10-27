<script lang="ts">
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { page } from '$app/stores';
  import { onMount } from 'svelte';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { wiki } from '$lib/api/client.svelte';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let title = $derived($page.params.title!);
  let revisions = $state<any[]>([]);
  let currentRev = $state<any>(null);
  let loading = $state(true);
  let error = $state('');
  let viewingRev = $state(false);

  onMount(() => loadHistory());

  async function loadHistory() {
    try {
      loading = true;
      revisions = await wiki.listRevisions(owner, repo, title);
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function viewRevision(rev: any) {
    try {
      currentRev = await wiki.getRevision(owner, repo, title, rev.id);
      viewingRev = true;
    } catch (e: any) {
      error = e.message;
    }
  }

  function closeRevision() {
    viewingRev = false;
    currentRev = null;
  }
</script>

<RepoHeader {owner} {repo} activeTab="wiki" />

<div class="content">
  <div class="header">
    <div class="breadcrumb">
      <a href={`/${owner}/${repo}/wiki`} class="link">{t('wiki.title')}</a>
      <span class="sep">/</span>
      <a href={`/${owner}/${repo}/wiki/${encodeURIComponent(title)}`} class="link">{title}</a>
      <span class="sep">/</span>
      <span class="current">{t('wiki.history')}</span>
    </div>
  </div>

  {#if loading}
    <LoadingState />
  {:else if error}
    <ErrorBanner message={error} />
  {:else}
    {#if viewingRev && currentRev}
      <div class="revision-detail">
        <div class="rev-meta">
          <h3>Revision #{currentRev.id}</h3>
          <p class="rev-info">
            {formatDate(currentRev.created_at)}
            {#if currentRev.message}
              &mdash; {currentRev.message}
            {/if}
          </p>
          <button class="btn btn-secondary" onclick={closeRevision}>
            {t('common.close')}
          </button>
        </div>
        <div class="rev-content">
          <pre class="wiki-content">{currentRev.content}</pre>
        </div>
      </div>
    {:else}
      <div class="revision-list">
        <table class="rev-table">
          <thead>
            <tr>
              <th>#</th>
              <th>{t('wiki.revisionMessage')}</th>
              <th>{t('common.date')}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each revisions as rev, i}
              <tr>
                <td class="rev-id">r{revisions.length - i}</td>
                <td class="rev-msg">
                  {rev.message || t('wiki.noMessage')}
                </td>
                <td class="rev-date">{formatDate(rev.created_at)}</td>
                <td class="rev-action">
                  <button class="btn btn-sm" onclick={() => viewRevision(rev)}>
                    {t('common.view')}
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
        {#if revisions.length === 0}
          <EmptyState title={t('wiki.noRevisions')} />
        {/if}
      </div>
    {/if}
  {/if}
</div>

<style>
  .content {
    max-width: 900px;
    margin: 0 auto;
    padding: 1.5rem;
  }
  .header {
    margin-bottom: 1.5rem;
  }
  .breadcrumb {
    font-size: 0.9rem;
    color: var(--text-secondary);
  }
  .breadcrumb a {
    color: var(--accent);
    text-decoration: none;
  }
  .breadcrumb a:hover { text-decoration: underline; }
  .breadcrumb .sep { margin: 0 0.4rem; }
  .breadcrumb .current { color: var(--text-primary); font-weight: 600; }

  .rev-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.9rem;
  }
  .rev-table th {
    text-align: left;
    padding: 0.5rem 0.75rem;
    border-bottom: 2px solid var(--border);
    color: var(--text-secondary);
    font-weight: 600;
  }
  .rev-table td {
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--border);
  }
  .rev-id {
    color: var(--text-secondary);
    font-family: monospace;
    width: 3rem;
  }
  .rev-msg { max-width: 400px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rev-date { color: var(--text-secondary); width: 10rem; }

  .revision-detail { margin-top: 1rem; }
  .rev-meta {
    background: var(--bg-secondary);
    padding: 1rem;
    border-radius: 8px;
    margin-bottom: 1rem;
    display: flex;
    align-items: center;
    gap: 1rem;
    flex-wrap: wrap;
  }
  .rev-meta h3 { margin: 0; font-size: 1rem; }
  .rev-info { margin: 0; color: var(--text-secondary); font-size: 0.85rem; }
  .rev-content {
    background: var(--bg-secondary);
    padding: 1rem;
    border-radius: 8px;
    overflow-x: auto;
  }
  .wiki-content {
    white-space: pre-wrap;
    word-wrap: break-word;
    font-family: inherit;
    font-size: 0.9rem;
    margin: 0;
    line-height: 1.6;
  }
</style>
