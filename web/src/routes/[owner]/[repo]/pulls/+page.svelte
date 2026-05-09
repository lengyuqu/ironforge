<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { pulls, repos } from '$lib/api/client';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner);
  let repo = $derived($page.params.repo);
  let prList = $state<any[]>([]);
  let loading = $state(true);
  let error = $state('');
  let filterState = $state('open');
  let showCreate = $state(false);
  let newTitle = $state('');
  let newBody = $state('');
  let newHead = $state('');
  let newBase = $state('main');
  let branches = $state<any[]>([]);

  $effect(() => {
    loadPRs();
    loadBranches();
  });

  async function loadPRs() {
    try {
      loading = true;
      prList = (await pulls.list(owner, repo, filterState)).data;
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function loadBranches() {
    try {
      branches = await repos.branches(owner, repo);
    } catch { /* ignore */ }
  }

  async function handleCreate(e: Event) {
    e.preventDefault();
    try {
      await pulls.create(owner, repo, {
        title: newTitle,
        body: newBody || undefined,
        head_branch: newHead,
        base_branch: newBase,
      });
      showCreate = false;
      newTitle = '';
      newBody = '';
      await loadPRs();
    } catch (e: any) {
      error = e.message;
    }
  }
</script>

<svelte:head>
  <title>Pull Requests · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader {owner} {repo} activeTab="pulls" starsCount={0} />

  <div class="toolbar">
    <div class="filter-tabs">
      <button class="filter-btn" class:active={filterState === 'open'} onclick={() => { filterState = 'open'; loadPRs(); }}>
        {$t('pulls.tabs.open')}
      </button>
      <button class="filter-btn" class:active={filterState === 'closed'} onclick={() => { filterState = 'closed'; loadPRs(); }}>
        {$t('pulls.tabs.closed')}
      </button>
      <button class="filter-btn" class:active={filterState === 'merged'} onclick={() => { filterState = 'merged'; loadPRs(); }}>
        {$t('pulls.tabs.merged')}
      </button>
    </div>
    <button class="btn-primary" onclick={() => showCreate = !showCreate}>
      {$t('pulls.new')}
    </button>
  </div>

  {#if showCreate}
    <div class="create-form">
      <h2>{$t('pulls.create_form.title')}</h2>
      <form onsubmit={handleCreate}>
        <div class="branch-row">
          <label>
            {$t('pulls.create_form.from')}
            <select bind:value={newHead} required>
              <option value="" disabled selected>{$t('pulls.create_form.select_branch')}</option>
              {#each branches as b}
                <option value={b.name}>{b.name}</option>
              {/each}
            </select>
          </label>
          <span class="arrow">→</span>
          <label>
            {$t('pulls.create_form.into')}
            <select bind:value={newBase} required>
              {#each branches as b}
                <option value={b.name}>{b.name} {b.is_default ? $t('repo.browser.default_branch') : ''}</option>
              {/each}
            </select>
          </label>
        </div>
        <label>
          {$t('pulls.create_form.description')}
          <input type="text" bind:value={newTitle} required placeholder={$t('pulls.create_form.description_placeholder')} />
        </label>
        <label>
          {$t('pulls.create_form.description')} <span class="optional">{$t('pulls.create_form.description_hint')}</span>
          <textarea bind:value={newBody} rows="4" placeholder={$t('pulls.create_form.description_placeholder')}></textarea>
        </label>
        <div class="form-actions">
          <button type="submit" class="btn-primary" disabled={!newHead}>{$t('pulls.create_form.submit')}</button>
          <button type="button" class="btn-secondary" onclick={() => showCreate = false}>{$t('pulls.create_form.cancel')}</button>
        </div>
      </form>
    </div>
  {/if}

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">{$t('common.loading')}</p>
  {:else if prList.length === 0}
    <div class="empty"><p>{$t('pulls.empty', { state: filterState === 'all' ? '' : filterState })}</p></div>
  {:else}
    <div class="pr-list">
      {#each prList as pr}
        <a href="/{owner}/{repo}/pulls/{pr.number}" class="pr-item">
          <span class="pr-icon">
            {pr.state === 'merged' ? '⊛' : pr.state === 'closed' ? '✓' : '⑂'}
          </span>
          <div class="pr-info">
            <div class="pr-title">{pr.title}</div>
            <div class="pr-meta">
              #{pr.number} opened {formatDate(pr.created_at)} by {pr.author || $t('common.unknown')}
              <span class="branch-label">{pr.head_branch}</span> → <span class="branch-label">{pr.base_branch}</span>
            </div>
          </div>
        </a>
      {/each}
    </div>
  {/if}
</div>

<style>
  .repo-page { max-width: 1100px; margin: 0 auto; padding: 24px; }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .filter-tabs { display: flex; gap: 4px; }
  .filter-btn {
    padding: 5px 12px;
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-secondary);
    font-size: 13px;
    cursor: pointer;
  }
  .filter-btn.active { background: var(--bg-tertiary); color: var(--text-primary); font-weight: 600; }
  .filter-btn:hover { background: var(--bg-hover); }

  .btn-primary {
    padding: 6px 16px;
    background: var(--green-dim);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-primary:hover { background: var(--green); }
  .btn-primary:disabled { opacity: 0.5; }

  .btn-secondary {
    padding: 6px 16px;
    background: none;
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 14px;
    cursor: pointer;
  }

  .create-form {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: 24px;
    margin-bottom: 24px;
  }
  h2 { font-size: 18px; margin-bottom: 16px; }
  form { display: flex; flex-direction: column; gap: 14px; }
  label { display: flex; flex-direction: column; gap: 6px; font-size: 13px; font-weight: 600; }
  .optional { font-weight: 400; color: var(--text-muted); }
  select { padding: 6px 10px; }
  textarea { font-family: var(--font-mono); font-size: 13px; resize: vertical; }

  .branch-row {
    display: flex;
    align-items: flex-end;
    gap: 12px;
  }
  .arrow { font-size: 20px; color: var(--text-muted); margin-bottom: 8px; }

  .form-actions { display: flex; gap: 8px; margin-top: 8px; }

  .error-banner {
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid var(--red-dim);
    color: var(--red);
    border-radius: var(--radius);
    padding: 10px 14px;
    font-size: 13px;
  }

  .empty { text-align: center; padding: 48px; color: var(--text-secondary); }

  .pr-list {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .pr-item {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-light);
    text-decoration: none;
    color: var(--text-primary);
  }
  .pr-item:last-child { border-bottom: none; }
  .pr-item:hover { background: var(--bg-secondary); text-decoration: none; }

  .pr-icon { font-size: 14px; margin-top: 3px; color: var(--green); }

  .pr-title { font-weight: 600; font-size: 15px; }
  .pr-meta { font-size: 12px; color: var(--text-muted); margin-top: 2px; }

  .branch-label {
    display: inline-block;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: 4px;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--accent);
  }
</style>
