<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { wiki } from '$lib/api/client';

  let owner = $derived($page.params.owner);
  let repo = $derived($page.params.repo);
  let pageList = $state<any[]>([]);
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
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function handleCreate(e: Event) {
    e.preventDefault();
    try {
      await wiki.create(owner, repo, newTitle, newContent);
      showCreate = false;
      newTitle = '';
      newContent = '';
      await loadPages();
    } catch (e: any) {
      error = e.message;
    }
  }
</script>

<svelte:head>
  <title>Wiki · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader {owner} {repo} activeTab="wiki" />

  <div class="toolbar">
    <h2>Wiki Pages</h2>
    <button class="btn-primary" onclick={() => showCreate = !showCreate}>New Page</button>
  </div>

  {#if showCreate}
    <div class="create-form">
      <form onsubmit={handleCreate}>
        <label>
          Title
          <input type="text" bind:value={newTitle} required placeholder="Page title" />
        </label>
        <label>
          Content (Markdown)
          <textarea bind:value={newContent} rows="8" required placeholder="Write your wiki content..."></textarea>
        </label>
        <div class="form-actions">
          <button type="submit" class="btn-primary">Create Page</button>
          <button type="button" class="btn-secondary" onclick={() => showCreate = false}>Cancel</button>
        </div>
      </form>
    </div>
  {/if}

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">Loading wiki...</p>
  {:else if pageList.length === 0}
    <div class="empty"><p>No wiki pages yet.</p></div>
  {:else}
    <div class="page-list">
      {#each pageList as p}
        <a href="/{owner}/{repo}/wiki/{encodeURIComponent(p.title)}" class="wiki-item">
          📄 {p.title}
          <span class="text-secondary text-sm">{new Date(p.updated_at).toLocaleDateString()}</span>
        </a>
      {/each}
    </div>
  {/if}
</div>

<style>
  .repo-page { max-width: 1100px; margin: 0 auto; padding: 24px; }
  .toolbar { display: flex; align-items: center; justify-content: space-between; margin-bottom: 16px; }
  h2 { font-size: 18px; }

  .btn-primary { padding: 6px 16px; background: var(--green-dim); color: #fff; border: none; border-radius: var(--radius); font-size: 14px; font-weight: 600; cursor: pointer; }
  .btn-primary:hover { background: var(--green); }
  .btn-secondary { padding: 6px 16px; background: none; color: var(--text-primary); border: 1px solid var(--border); border-radius: var(--radius); font-size: 14px; cursor: pointer; }

  .create-form { background: var(--bg-secondary); border: 1px solid var(--border); border-radius: var(--radius-lg); padding: 24px; margin-bottom: 24px; }
  form { display: flex; flex-direction: column; gap: 14px; }
  label { display: flex; flex-direction: column; gap: 6px; font-size: 13px; font-weight: 600; }
  textarea { font-family: var(--font-mono); font-size: 13px; resize: vertical; }
  .form-actions { display: flex; gap: 8px; margin-top: 8px; }

  .error-banner { background: rgba(248, 81, 73, 0.1); border: 1px solid var(--red-dim); color: var(--red); border-radius: var(--radius); padding: 10px 14px; font-size: 13px; }
  .empty { text-align: center; padding: 48px; color: var(--text-secondary); }

  .page-list { border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; }
  .wiki-item {
    display: flex; align-items: center; justify-content: space-between;
    padding: 12px 16px; border-bottom: 1px solid var(--border-light);
    text-decoration: none; color: var(--text-primary); font-size: 14px;
  }
  .wiki-item:last-child { border-bottom: none; }
  .wiki-item:hover { background: var(--bg-secondary); text-decoration: none; }
</style>
