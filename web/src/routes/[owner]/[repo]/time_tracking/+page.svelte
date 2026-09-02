<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { issues, timeTracking } from '$lib/api/client.svelte';
  import type { Issue, TimeEntry } from '$lib/types/entities';
  import IssueSelector from '$lib/components/issues/IssueSelector.svelte';
  import TimeEntryForm from '$lib/components/issues/TimeEntryForm.svelte';
  import TimeEntryList from '$lib/components/issues/TimeEntryList.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  // Issue selector
  let issueList = $state<Issue[]>([]);
  let selectedIssue = $state<Issue | null>(null);
  let issueLoading = $state(true);

  // Time entries for selected issue
  let entries = $state<TimeEntry[]>([]);
  let totalFormatted = $state('');
  let entriesLoading = $state(false);
  let currentPage = $state(1);
  let totalPages = $state(1);

  let error = $state('');

  $effect(() => { loadIssues(); });

  async function loadIssues() {
    issueLoading = true;
    error = '';
    try {
      const res = await issues.list(owner, repo, 'open', 1, 100);
      issueList = res.data || [];
    } catch (e: any) {
      error = e.message;
    } finally {
      issueLoading = false;
    }
  }

  async function selectIssue(issue: Issue) {
    selectedIssue = issue;
    currentPage = 1;
    await Promise.all([loadEntries(), loadTotal()]);
  }

  async function loadEntries() {
    if (!selectedIssue) return;
    entriesLoading = true;
    try {
      const res = await timeTracking.list(owner, repo, selectedIssue.number, currentPage, 20);
      entries = res.data || [];
      totalPages = res.pagination?.total_pages ?? 1;
    } catch (e: any) {
      error = e.message;
    } finally {
      entriesLoading = false;
    }
  }

  async function loadTotal() {
    if (!selectedIssue) return;
    try {
      const res = await timeTracking.total(owner, repo, selectedIssue.number);
      totalFormatted = res.total_formatted;
    } catch {
      totalFormatted = '';
    }
  }

  /** 添加/删除条目后统一刷新（条目列表 + 总时长） */
  async function refreshEntries() {
    await Promise.all([loadEntries(), loadTotal()]);
  }

  function handlePageChange(next: number) {
    currentPage = next;
    loadEntries();
  }
</script>

<svelte:head>
  <title>Time Tracking · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="time_tracking" />

  <div class="page-header">
    <h1>{t('time_tracking.title')}</h1>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <div class="layout">
    <IssueSelector
      issues={issueList}
      loading={issueLoading}
      selectedId={selectedIssue?.id ?? null}
      onSelect={selectIssue}
    />

    <main class="main-panel">
      {#if !selectedIssue}
        <div class="select-hint">
          <p>← Select an issue to view and log time</p>
        </div>
      {:else}
        <div class="issue-header">
          <h2>
            <a href={`/${owner}/${repo}/issues/${selectedIssue.number}`} class="issue-link">
              #{selectedIssue.number} {selectedIssue.title}
            </a>
          </h2>
          {#if totalFormatted}
            <div class="total-badge">Total: {totalFormatted}</div>
          {/if}
        </div>

        <TimeEntryForm
          {owner}
          {repo}
          issueNumber={selectedIssue.number}
          onAdded={refreshEntries}
        />

        <TimeEntryList
          {owner}
          {repo}
          issueNumber={selectedIssue.number}
          entries={entries}
          loading={entriesLoading}
          currentPage={currentPage}
          totalPages={totalPages}
          onPageChange={handlePageChange}
          onRefresh={refreshEntries}
        />
      {/if}
    </main>
  </div>
</div>

<style>
  .page-header { margin-bottom: 20px; }
  h1 { font-size: 22px; font-weight: 600; margin: 0; }

  .layout {
    display: grid;
    grid-template-columns: 240px 1fr;
    gap: 20px;
    align-items: start;
  }

  @media (max-width: 600px) { .layout { grid-template-columns: 1fr; } }

  .main-panel { min-width: 0; }

  .select-hint {
    padding: 80px 24px;
    text-align: center;
    color: var(--text-secondary);
    font-size: 14px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .issue-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 10px;
    margin-bottom: 16px;
  }

  h2 { font-size: 17px; font-weight: 600; margin: 0; }
  .issue-link { color: var(--text-primary); text-decoration: none; }
  .issue-link:hover { color: var(--accent); }

  .total-badge {
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 4px 12px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .error-banner {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 10px 12px;
    border-radius: var(--radius);
    margin-bottom: 16px;
  }
</style>
