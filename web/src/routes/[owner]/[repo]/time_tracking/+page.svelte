<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { timeTracking, issues } from '$lib/api/client';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  let issueNumber = $state('');
  let entries = $state<any[]>([]);
  let totalMinutes = $state(0);
  let totalFormatted = $state('');
  let loading = $state(false);
  let error = $state('');
  let currentPage = $state(1);
  let totalPages = $state(1);

  // Form state
  let duration = $state<number>(60);
  let description = $state('');
  let saving = $state(false);

  async function loadEntries() {
    if (!issueNumber) return;
    const num = parseInt(issueNumber);
    if (isNaN(num)) return;
    loading = true;
    error = '';
    try {
      const res = await timeTracking.list(owner!, repo!, num, currentPage, 20);
      entries = res.data || [];
      totalPages = Math.ceil((res.pagination?.total || 0) / 20);

      const totalRes = await timeTracking.total(owner!, repo!, num);
      totalMinutes = totalRes.total_minutes;
      totalFormatted = totalRes.total_formatted;
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function handleAdd() {
    if (duration <= 0 || !issueNumber) return;
    const num = parseInt(issueNumber);
    if (isNaN(num)) return;
    saving = true;
    error = '';
    try {
      await timeTracking.add(owner!, repo!, num, {
        duration_minutes: duration,
        description: description || undefined,
      });
      description = '';
      duration = 60;
      await loadEntries();
    } catch (e: any) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  async function handleDelete(id: number) {
    if (!issueNumber) return;
    const num = parseInt(issueNumber);
    if (isNaN(num)) return;
    if (!confirm('Delete this time entry?')) return;
    try {
      await timeTracking.delete(owner!, repo!, num, id);
      await loadEntries();
    } catch (e: any) {
      error = e.message;
    }
  }

  function searchIssue() {
    if (issueNumber) loadEntries();
  }
</script>

<svelte:head>
  <title>Time Tracking · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader {owner} {repo} activeTab="time_tracking" />

  <div class="page-header">
    <h1>{t('time_tracking.title')}</h1>
  </div>

  <!-- Issue Selector -->
  <div class="form-card">
    <div class="form-row">
      <div class="form-group">
        <label for="tt-issue">Issue #</label>
        <input id="tt-issue" type="number" bind:value={issueNumber} placeholder="Enter issue number..." min="1" />
      </div>
      <div class="form-group" style="align-self:flex-end">
        <button class="btn btn-primary" onclick={searchIssue} disabled={!issueNumber}>{t('common.search')}</button>
      </div>
    </div>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if issueNumber}
    <!-- Total -->
    {#if totalFormatted}
      <div class="total-bar">
        {t('time_tracking.total_hours').replace('{hours}', totalFormatted)}
      </div>
    {/if}

    <!-- Add Entry Form -->
    <div class="form-card">
      <h2>{t('time_tracking.add_entry')}</h2>
      <div class="form-row">
        <div class="form-group">
          <label for="tt-duration">{t('time_tracking.duration')}</label>
          <input id="tt-duration" type="number" bind:value={duration} min="1" />
        </div>
        <div class="form-group" style="flex:2">
          <label for="tt-note">{t('time_tracking.note')}</label>
          <input id="tt-note" type="text" bind:value={description} placeholder="What did you work on?" />
        </div>
        <div class="form-group" style="align-self:flex-end">
          <button class="btn btn-primary" onclick={handleAdd} disabled={saving || duration <= 0}>
            {saving ? t('common.saving') : t('common.add')}
          </button>
        </div>
      </div>
    </div>

    <!-- Entries Table -->
    {#if loading}
      <p class="loading-text">{t('common.loading')}...</p>
    {:else if entries.length === 0}
      <p class="empty-text">{t('time_tracking.no_entries')}</p>
    {:else}
      <table class="entry-table">
        <thead>
          <tr>
            <th>{t('time_tracking.date')}</th>
            <th>{t('time_tracking.duration')}</th>
            <th>{t('time_tracking.note')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each entries as entry (entry.id)}
            <tr>
              <td>{entry.date || formatDate(entry.created_at)}</td>
              <td>{Math.round((entry.duration_minutes || entry.duration || 0) / 60 * 10) / 10}h</td>
              <td>{entry.description || entry.note || ''}</td>
              <td>
                <button class="btn btn-sm btn-danger" onclick={() => handleDelete(entry.id)}>
                  {t('common.delete')}
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>

      {#if totalPages > 1}
        <div class="pagination">
          <button class="btn btn-sm" onclick={() => { currentPage = Math.max(1, currentPage - 1); loadEntries(); }} disabled={currentPage <= 1}>
            {t('common.prev')}
          </button>
          <span>{currentPage} / {totalPages}</span>
          <button class="btn btn-sm" onclick={() => { currentPage = Math.min(totalPages, currentPage + 1); loadEntries(); }} disabled={currentPage >= totalPages}>
            {t('common.next')}
          </button>
        </div>
      {/if}
    {/if}
  {/if}
</div>

<style>
  .repo-page { max-width: 900px; margin: 0 auto; padding: 24px; }
  .page-header { margin-bottom: 20px; }
  h1 { font-size: 24px; font-weight: 600; margin: 0; }
  h2 { font-size: 15px; margin: 0 0 10px; }

  .error-banner { background: rgba(248,81,73,0.1); border:1px solid #dc2626; color:#dc2626; border-radius:8px; padding:10px 14px; font-size:13px; margin-bottom:16px; }
  .loading-text, .empty-text { text-align:center; padding:48px; color:var(--text-secondary, #666); }

  .form-card { background:var(--bg-secondary, #f9fafb); border:1px solid var(--border-color, #e5e7eb); border-radius:8px; padding:16px; margin-bottom:16px; }
  .form-row { display:flex; gap:12px; align-items:flex-end; flex-wrap:wrap; }
  .form-group { display:flex; flex-direction:column; gap:4px; min-width:100px; }
  .form-group label { font-size:12px; font-weight:600; color:var(--text-secondary, #666); }
  .form-group input { padding:6px 10px; border:1px solid var(--border-color, #d1d5db); border-radius:6px; font-size:13px; }

  .total-bar { background:var(--accent, #2563eb); color:#fff; border-radius:8px; padding:12px 16px; font-size:15px; font-weight:600; margin-bottom:16px; }

  .entry-table { width:100%; border-collapse:collapse; font-size:13px; }
  .entry-table th { text-align:left; padding:8px 12px; border-bottom:2px solid var(--border-color, #e5e7eb); color:var(--text-secondary, #666); font-weight:600; }
  .entry-table td { padding:8px 12px; border-bottom:1px solid var(--border-color, #e5e7eb); }

  .btn { padding:6px 14px; border:1px solid var(--border-color, #d1d5db); border-radius:6px; background:var(--bg-primary, #fff); cursor:pointer; font-size:13px; color:var(--text-primary, #333); }
  .btn:hover { background:var(--bg-secondary, #f3f4f6); }
  .btn-primary { background:var(--accent, #2563eb); color:#fff; border-color:var(--accent, #2563eb); }
  .btn-sm { padding:4px 10px; font-size:12px; }
  .btn-danger { color:#dc2626; border-color:#dc2626; }

  .pagination { display:flex; gap:12px; align-items:center; justify-content:center; margin-top:16px; font-size:13px; color:var(--text-secondary, #666); }
</style>
