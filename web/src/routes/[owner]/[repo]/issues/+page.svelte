<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { issues } from '$lib/api/client';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let issueList = $state<any[]>([]);
  let loading = $state(true);
  let error = $state('');
  let filterState = $state('open');
  let showCreate = $state(false);
  let newTitle = $state('');
  let newBody = $state('');
  let newLabels = $state('');

  $effect(() => {
    loadIssues();
  });

  async function loadIssues() {
    try {
      loading = true;
      issueList = (await issues.list(owner, repo, filterState)).data;
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function handleCreate(e: Event) {
    e.preventDefault();
    try {
      const labels = newLabels ? newLabels.split(',').map(l => l.trim()) : undefined;
      await issues.create(owner, repo, newTitle, newBody || undefined, labels);
      showCreate = false;
      newTitle = '';
      newBody = '';
      newLabels = '';
      await loadIssues();
    } catch (e: any) {
      error = e.message;
    }
  }
</script>

<svelte:head>
  <title>Issues · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader {owner} {repo} activeTab="issues" starsCount={0} />

  <div class="issues-toolbar">
    <div class="filter-tabs">
      <button class="filter-btn" class:active={filterState === 'open'} onclick={() => { filterState = 'open'; loadIssues(); }}>
        {t('issues.tabs.open')}
      </button>
      <button class="filter-btn" class:active={filterState === 'closed'} onclick={() => { filterState = 'closed'; loadIssues(); }}>
        {t('issues.tabs.closed')}
      </button>
      <button class="filter-btn" class:active={filterState === 'all'} onclick={() => { filterState = 'all'; loadIssues(); }}>
        {t('issues.tabs.all')}
      </button>
    </div>
    <button class="btn-primary" onclick={() => showCreate = !showCreate}>
      {t('issues.new')}
    </button>
  </div>

  {#if showCreate}
    <div class="create-form">
      <form onsubmit={handleCreate}>
        <label>
          {t('issues.create_form.title')}
          <input type="text" bind:value={newTitle} required placeholder={t('issues.create_form.title_placeholder')} />
        </label>
        <label>
          {t('issues.create_form.body')} <span class="optional">{t('issues.create_form.body_hint')}</span>
          <textarea bind:value={newBody} rows="6" placeholder={t('issues.create_form.body_placeholder')}></textarea>
        </label>
        <label>
          {t('issues.create_form.labels')} <span class="optional">{t('issues.create_form.labels_hint')}</span>
          <input type="text" bind:value={newLabels} placeholder={t('issues.create_form.labels_placeholder')} />
        </label>
        <div class="form-actions">
          <button type="submit" class="btn-primary">{t('issues.create_form.submit')}</button>
          <button type="button" class="btn-secondary" onclick={() => showCreate = false}>{t('issues.create_form.cancel')}</button>
        </div>
      </form>
    </div>
  {/if}

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">{t('common.loading')}</p>
  {:else if issueList.length === 0}
    <div class="empty">
      <p>{t('issues.empty', { state: filterState === 'all' ? '' : filterState })}</p>
    </div>
  {:else}
    <div class="issue-list">
      {#each issueList as issue}
        <a href="/{owner}/{repo}/issues/{issue.number}" class="issue-item">
          <span class="issue-icon">
            {issue.state === 'closed' ? '✓' : '●'}
          </span>
          <div class="issue-info">
            <div class="issue-title">{issue.title}</div>
            <div class="issue-meta">
              #${issue.number} {t('issues.meta', { date: formatDate(issue.created_at), author: issue.author || t('common.unknown') })}
              {#if issue.labels?.length}
                {#each issue.labels as label}
                  <span class="label-badge">{label}</span>
                {/each}
              {/if}
            </div>
          </div>
        </a>
      {/each}
    </div>
  {/if}
</div>

<style>
  .repo-page { max-width: 1100px; margin: 0 auto; padding: 24px; }

  .issues-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .filter-tabs {
    display: flex;
    gap: 4px;
  }

  .filter-btn {
    padding: 5px 12px;
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-secondary);
    font-size: 13px;
    cursor: pointer;
  }
  .filter-btn.active {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    font-weight: 600;
  }
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

  form { display: flex; flex-direction: column; gap: 14px; }
  label { display: flex; flex-direction: column; gap: 6px; font-size: 13px; font-weight: 600; }
  .optional { font-weight: 400; color: var(--text-muted); }
  textarea { font-family: var(--font-mono); font-size: 13px; resize: vertical; }
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

  .issue-list {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .issue-item {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-light);
    text-decoration: none;
    color: var(--text-primary);
  }
  .issue-item:last-child { border-bottom: none; }
  .issue-item:hover { background: var(--bg-secondary); text-decoration: none; }

  .issue-icon {
    font-size: 14px;
    margin-top: 3px;
    color: var(--green);
  }

  .issue-title { font-weight: 600; font-size: 15px; }

  .issue-meta {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 2px;
  }

  .label-badge {
    display: inline-block;
    padding: 0 6px;
    border: 1px solid var(--purple);
    color: var(--purple);
    border-radius: 10px;
    font-size: 11px;
    margin-left: 4px;
  }
</style>
