<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { wiki } from '$lib/api/client';
  import { createT } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner);
  let repo = $derived($page.params.repo);
  let title = $derived(decodeURIComponent($page.params.title));
  let wikiPage = $state<any>(null);
  let loading = $state(true);
  let error = $state('');
  let editing = $state(false);
  let editContent = $state('');

  $effect(() => { loadPage(); });

  async function loadPage() {
    try {
      loading = true;
      wikiPage = await wiki.get(owner, repo, title);
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function handleSave() {
    try {
      await wiki.update(owner, repo, title, editContent);
      editing = false;
      await loadPage();
    } catch (e: any) {
      error = e.message;
    }
  }

  function startEditing() {
    editContent = wikiPage?.content || '';
    editing = true;
  }
</script>

<svelte:head>
  <title>{title} · {owner}/{repo} Wiki · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader {owner} {repo} activeTab="wiki" />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">{$t('common.loading')}</p>
  {:else if wikiPage}
    <div class="wiki-page">
      <div class="wiki-header">
        <h1>{title}</h1>
        <button class="btn-outline" onclick={startEditing}>{$t('wiki.edit')}</button>
      </div>

      {#if editing}
        <div class="edit-area">
          <textarea bind:value={editContent} rows="16"></textarea>
          <div class="form-actions">
            <button class="btn-primary" onclick={handleSave}>{$t('wiki.save')}</button>
            <button class="btn-secondary" onclick={() => editing = false}>{$t('wiki.cancel')}</button>
          </div>
        </div>
      {:else}
        <div class="wiki-content">
          {wikiPage.content}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .repo-page { max-width: 1100px; margin: 0 auto; padding: 24px; }

  .error-banner { background: rgba(248, 81, 73, 0.1); border: 1px solid var(--red-dim); color: var(--red); border-radius: var(--radius); padding: 10px 14px; font-size: 13px; }

  .wiki-header {
    display: flex; align-items: center; justify-content: space-between;
    margin-bottom: 20px; border-bottom: 1px solid var(--border); padding-bottom: 12px;
  }
  h1 { font-size: 24px; }

  .btn-outline {
    padding: 5px 12px; background: none; border: 1px solid var(--border);
    border-radius: var(--radius); color: var(--text-primary); font-size: 13px; cursor: pointer;
  }
  .btn-outline:hover { background: var(--bg-hover); }

  .btn-primary { padding: 6px 16px; background: var(--green-dim); color: #fff; border: none; border-radius: var(--radius); font-size: 14px; font-weight: 600; cursor: pointer; }
  .btn-primary:hover { background: var(--green); }
  .btn-secondary { padding: 6px 16px; background: none; color: var(--text-primary); border: 1px solid var(--border); border-radius: var(--radius); font-size: 14px; cursor: pointer; }

  .edit-area { margin-top: 16px; }
  textarea { width: 100%; font-family: var(--font-mono); font-size: 13px; resize: vertical; margin-bottom: 12px; }
  .form-actions { display: flex; gap: 8px; }

  .wiki-content {
    background: var(--bg-secondary); border: 1px solid var(--border);
    border-radius: var(--radius); padding: 24px; line-height: 1.7;
    white-space: pre-wrap; font-size: 14px;
  }
</style>
