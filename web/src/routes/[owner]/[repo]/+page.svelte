<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { repos } from '$lib/api/client';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let ref = $state('');
  let path = $state('');
  let entries = $state<any[]>([]);
  let branches = $state<any[]>([]);
  let commits = $state<any[]>([]);
  let loading = $state(true);
  let error = $state('');
  let showBranches = $state(false);

  $effect(() => {
    loadData();
  });

  async function loadData() {
    loading = true;
    error = '';
    try {
      const [treeData, branchData, logData] = await Promise.all([
        repos.tree(owner, repo, ref || undefined, path || undefined),
        repos.branches(owner, repo),
        repos.log(owner, repo, ref || undefined, path || undefined),
      ]);
      entries = treeData.entries || [];
      branches = branchData || [];
      commits = (logData.commits || []).slice(0, 5);
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function navigateToPath(entryName: string) {
    if (path) {
      path = path + '/' + entryName;
    } else {
      path = entryName;
    }
    loadData();
  }

  function navigateUp() {
    const parts = path.split('/');
    parts.pop();
    path = parts.join('/');
    loadData();
  }

  function selectBranch(branchName: string) {
    ref = branchName;
    showBranches = false;
    loadData();
  }

  function formatFileSize(size: number) {
    if (size < 1024) return size + t('repo.file_size.b');
    if (size < 1024 * 1024) return (size / 1024).toFixed(1) + t('repo.file_size.kb');
    return (size / (1024 * 1024)).toFixed(1) + t('repo.file_size.mb');
  }
</script>

<svelte:head>
  <title>{owner}/{repo} · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader {owner} {repo} activeTab="code" starsCount={0} />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">Loading...</p>
  {:else}
    <!-- Branch selector + path breadcrumb -->
    <div class="toolbar">
      <div class="branch-selector" style="position:relative">
        <button class="btn-outline" onclick={() => showBranches = !showBranches}>
          🌿 {ref || 'main'} ▾
        </button>
        {#if showBranches}
          <div class="dropdown">
            {#each branches as b}
              <button class="dropdown-item" class:active={b.name === ref || (!ref && b.is_default)} onclick={() => selectBranch(b.name)}>
                {b.name} {b.is_default ? t('repo.browser.default_branch') : ''}
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <div class="breadcrumb">
        <a href="/{owner}/{repo}">{repo}</a>
        {#if path}
          {#each path.split('/') as part, i}
            <span class="sep">/</span>
            <span>{part}</span>
          {/each}
        {/if}
      </div>
    </div>

    <div class="content-grid">
      <!-- File tree -->
      <div class="tree-panel">
        {#if path}
          <div class="entry" onclick={navigateUp}>
            <span class="entry-icon">📁</span>
            <span class="entry-name up">..</span>
          </div>
        {/if}
        {#each entries as entry}
          {#if entry.kind === 'tree' || entry.kind === 'dir'}
            <div class="entry" onclick={() => navigateToPath(entry.name)} role="button" tabindex="0">
              <span class="entry-icon">📁</span>
              <span class="entry-name dir">{entry.name}</span>
            </div>
          {:else}
            <a href="/{owner}/{repo}/blob/{path ? path + '/' : ''}{entry.name}" class="entry file-entry">
              <span class="entry-icon">📄</span>
              <span class="entry-name">{entry.name}</span>
              {#if entry.size}
                <span class="entry-size">{formatFileSize(entry.size)}</span>
              {/if}
            </a>
          {/if}
        {/each}
      </div>

      <!-- Recent commits -->
      <div class="commits-panel">
        <h3>{t('repo.browser.recent_commits')}</h3>
        {#each commits as commit}
          <div class="commit-item">
            <div class="commit-msg truncate">{commit.message?.split('\n')[0]}</div>
            <div class="commit-meta">
              <span class="commit-author">{commit.author}</span>
              <span class="commit-date">{formatDate(commit.date)}</span>
              <code class="commit-sha">{commit.sha?.slice(0, 7)}</code>
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .repo-page {
    max-width: 1100px;
    margin: 0 auto;
    padding: 24px;
  }

  .error-banner {
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid var(--red-dim);
    color: var(--red);
    border-radius: var(--radius);
    padding: 10px 14px;
    font-size: 13px;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 16px;
  }

  .btn-outline {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
  }
  .btn-outline:hover { background: var(--bg-hover); }

  .dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 4px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    min-width: 200px;
    max-height: 300px;
    overflow-y: auto;
    box-shadow: 0 8px 24px rgba(0,0,0,0.4);
    z-index: 200;
  }

  .dropdown-item {
    display: block;
    width: 100%;
    padding: 8px 12px;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .dropdown-item:hover { background: var(--bg-hover); }
  .dropdown-item.active { font-weight: 600; color: var(--accent); }

  .breadcrumb {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 14px;
  }
  .breadcrumb a { color: var(--accent); font-weight: 600; }
  .sep { color: var(--text-muted); }

  .content-grid {
    display: grid;
    grid-template-columns: 1fr 320px;
    gap: 16px;
  }

  @media (max-width: 768px) {
    .content-grid { grid-template-columns: 1fr; }
  }

  .tree-panel {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .entry {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    border-bottom: 1px solid var(--border-light);
    font-size: 14px;
    cursor: pointer;
    text-decoration: none;
    color: var(--text-primary);
  }
  .entry:hover { background: var(--bg-hover); }
  .file-entry { cursor: pointer; }

  .entry-icon { font-size: 14px; }
  .entry-name { flex: 1; }
  .entry-name.dir { color: var(--text-primary); font-weight: 500; }
  .entry-name.up { color: var(--text-muted); }
  .entry-size { font-size: 12px; color: var(--text-muted); font-family: var(--font-mono); }

  .commits-panel {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
  }

  h3 { font-size: 14px; margin-bottom: 12px; }

  .commit-item {
    padding: 8px 0;
    border-bottom: 1px solid var(--border-light);
  }
  .commit-item:last-child { border-bottom: none; }

  .commit-msg {
    font-size: 13px;
    font-weight: 500;
    margin-bottom: 4px;
  }

  .commit-meta {
    display: flex;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
    align-items: center;
  }

  .commit-sha {
    font-size: 11px;
    background: var(--bg-tertiary);
    padding: 1px 6px;
    border-radius: 4px;
    color: var(--accent);
  }
</style>
