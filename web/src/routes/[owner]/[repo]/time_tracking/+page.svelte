<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import { page } from '$app/state';
  import { issues, timeTracking } from '$lib/api/client.svelte';
  import type { Issue, TimeEntry } from '$lib/types/entities';
  import IssueSelector from '$lib/components/issues/IssueSelector.svelte';
  import TimeEntryForm from '$lib/components/issues/TimeEntryForm.svelte';
  import TimeEntryList from '$lib/components/issues/TimeEntryList.svelte';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';

  const t = createT();
  const issuesLatest = createLatest();
  const entriesLatest = createLatest();
  const totalLatest = createLatest();

  let owner = $derived(page.params.owner!);
  let repo = $derived(page.params.repo!);

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

  $effect(() => { void loadIssues(); });

  async function loadIssues() {
    const reqOwner = owner;
    const reqRepo = repo;
    entriesLatest.invalidate();
    totalLatest.invalidate();
    const isCurrent = issuesLatest.begin();
    issueLoading = true;
    error = '';
    try {
      const res = await issues.list(reqOwner, reqRepo, 'open', 1, 100);
      if (!isCurrent()) return;
      issueList = res.data || [];
    } catch (e) {
      if (!isCurrent()) return;
      error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      if (isCurrent()) {
        issueLoading = false;
      }
    }
  }

  async function selectIssue(issue: Issue) {
    selectedIssue = issue;
    currentPage = 1;
    await Promise.all([loadEntries(), loadTotal()]);
  }

  async function loadEntries() {
    const reqOwner = owner;
    const reqRepo = repo;
    const reqIssue = selectedIssue;
    const reqPage = currentPage;
    if (!reqIssue) return;
    const isCurrent = entriesLatest.begin();
    entriesLoading = true;
    try {
      const res = await timeTracking.list(reqOwner, reqRepo, reqIssue.number, reqPage, 20);
      if (!isCurrent()) return;
      entries = res.data || [];
      totalPages = res.pagination?.total_pages ?? 1;
    } catch (e) {
      if (!isCurrent()) return;
      error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      if (isCurrent()) {
        entriesLoading = false;
      }
    }
  }

  async function loadTotal() {
    const reqOwner = owner;
    const reqRepo = repo;
    const reqIssue = selectedIssue;
    if (!reqIssue) return;
    const isCurrent = totalLatest.begin();
    try {
      const res = await timeTracking.total(reqOwner, reqRepo, reqIssue.number);
      if (!isCurrent()) return;
      totalFormatted = res.total_formatted;
    } catch {
      if (!isCurrent()) return;
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
  <title>{t('time_tracking.title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-header">
  <h1>{t('time_tracking.title')}</h1>
</div>

{#if error}
  <ErrorBanner message={error} />
{/if}

<div class="layout">
  <IssueSelector
    issues={issueList}
    loading={issueLoading}
    selectedId={selectedIssue?.id ?? null}
    onSelect={selectIssue}
  />

  <section class="main-panel">
    {#if !selectedIssue}
      <div class="select-hint surface">
        <p>← {t('time_tracking.select_hint')}</p>
      </div>
    {:else}
      <div class="issue-header">
        <h2>
          <a href={`/${owner}/${repo}/issues/${selectedIssue.number}`} class="issue-link">
            #{selectedIssue.number} {selectedIssue.title}
          </a>
        </h2>
        {#if totalFormatted}
          <div class="total-badge">{t('time_tracking.total', { time: totalFormatted })}</div>
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
  </section>
</div>

<style>
  h1 {
    margin: 0;
  }

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
  }

  .issue-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 10px;
    margin-bottom: 16px;
  }

  h2 {
    margin: 0;
  }
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

</style>
