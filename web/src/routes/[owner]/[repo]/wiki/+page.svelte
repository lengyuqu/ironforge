<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { wiki } from '$lib/api/client.svelte';
  import type { WikiPageSummary } from '$lib/types/entities';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let pageList = $state<WikiPageSummary[]>([]);
  let loading = $state(true);
  let error = $state('');
  let showCreate = $state(false);
  let newTitle = $state('');
  let newContent = $state('');

  $effect(() => { loadPages(); });

  async function loadPages() {
    try {
      loading = true;
      pageList = await wiki.list(owner, repo);
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      loading = false;
    }
  }

  async function handleCreate(e: Event) {
    e.preventDefault();
    try {
      const createdTitle = newTitle;
      await wiki.create(owner, repo, createdTitle, newContent);
      showCreate = false;
      newTitle = '';
      newContent = '';
      await goto(`/${owner}/${repo}/wiki/${encodeURIComponent(createdTitle)}`);
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    }
  }
</script>

<svelte:head>
  <title>Wiki · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="wiki" starsCount={0} />

  <div class="toolbar">
    <h2>{t('wiki.pages')}</h2>
    <button class="btn-primary" onclick={() => showCreate = !showCreate}>{t('wiki.new')}</button>
  </div>

  {#if showCreate}
    <div class="create-form surface">
      <form onsubmit={handleCreate}>
        <label>
          {t('wiki.create_form.title')}
          <input type="text" bind:value={newTitle} required placeholder={t('wiki.create_form.title_placeholder')} />
        </label>
        <label>
          {t('wiki.create_form.content')}
          <textarea bind:value={newContent} rows="8" required placeholder={t('wiki.create_form.content_placeholder')}></textarea>
        </label>
        <div class="form-actions">
          <button type="submit" class="btn-primary">{t('wiki.create_form.submit')}</button>
          <button type="button" class="btn-secondary" onclick={() => showCreate = false}>{t('wiki.create_form.cancel')}</button>
        </div>
      </form>
    </div>
  {/if}

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  {#if loading}
    <LoadingState />
  {:else if pageList.length === 0}
    <EmptyState title={t('wiki.empty')} />
  {:else}
    <div class="page-list">
      {#each pageList as p}
        <a href={`/${owner}/${repo}/wiki/${encodeURIComponent(p.title)}`} class="wiki-item">
          📄 {p.title}
          <span class="text-secondary text-sm">{formatDate(p.updated_at)}</span>
        </a>
      {/each}
    </div>
  {/if}
</div>

<style>
  .toolbar { display: flex; align-items: center; justify-content: space-between; margin-bottom: 16px; }

  .create-form {
    padding: 24px;
    margin-bottom: 24px;
  }
  form { display: flex; flex-direction: column; gap: 14px; }
  label { display: flex; flex-direction: column; gap: 6px; font-size: 13px; font-weight: 600; }
  textarea {
    resize: vertical;
  }

  .page-list { border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; }
  .wiki-item {
    display: flex; align-items: center; justify-content: space-between;
    padding: 12px 16px; border-bottom: 1px solid var(--border-light);
    text-decoration: none; color: var(--text-primary); font-size: 14px;
  }
  .wiki-item:last-child { border-bottom: none; }
  .wiki-item:hover { background: var(--bg-secondary); text-decoration: none; }
</style>
