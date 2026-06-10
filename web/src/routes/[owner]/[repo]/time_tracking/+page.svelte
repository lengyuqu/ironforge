<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { timeTracking } from '$lib/api/client';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  let entries = $state<any[]>([]);
  let loading = $state(true);
  let error = $state('');
  let currentPage = $state(1);
  let totalPages = $state(1);

  // Form state
  let duration = $state<number>(1);
  let note = $state('');
  let entryDate = $state(new Date().toISOString().slice(0, 10));
  let saving = $state(false);

  $effect(() => {
    loadEntries();
  });

  async function loadEntries() {
    loading = true;
    error = '';
    try {
      const res = await timeTracking.list(owner!, repo!, currentPage, 20);
      entries = res.data;
      totalPages = res.pagination.total_pages;
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function handleAdd() {
    if (duration <= 0) return;
    saving = true;
    error = '';
    try {
      await timeTracking.add(owner!, repo!, {
        duration,
        note: note || undefined,
        date: entryDate || undefined,
      });
      note = '';
      duration = 1;
      await loadEntries();
    } catch (e: any) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  async function handleDelete(id: number) {
    if (!confirm(t('time_tracking.delete_confirm') || `Delete entry ${id}?`)) return;
    try {
      await timeTracking.delete(owner!, repo!, id);
      await loadEntries();
    } catch (e: any) {
      error = e.message;
    }
  }

  function calcTotal(): number {
    return entries.reduce((sum: number, e: any) => sum + (e.duration || 0), 0);
  }
</script>

<svelte:head>
  <title>Time Tracking · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader owner={owner!} repo={repo!} activeTab="time_tracking" />

  <div class="page-header">
    <h1>{t('repo.tabs.time_tracking')}</h1>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <!-- Add Entry Form -->
  <div class="form-card">
    <h2>{t('time_tracking.add_entry')}</h2>
    <div class="form-row">
      <div class="form-group">
        <label for="tt-date">{t('time_tracking.date')}</label>
        <input id="tt-date" type="date" bind:value={entryDate} />
      </div>
      <div class="form-group">
        <label for="tt-duration">{t('time_tracking.duration')}</label>
        <input id="tt-duration" type="number" min="0.5" step="0.5" bind:value={duration} />
        <span class="unit">hrs</span>
      </div>
      <div class="form-group flex-grow">
        <label for="tt-note">{t('time_tracking.note')}</label>
        <input id="tt-note" type="text" placeholder={t('common.optional')} bind:value={note} />
      </div>
      <div class="form-action">
        <button class="btn-primary" onclick={handleAdd} disabled={saving}>
          {saving ? t('common.saving') : t('common.add')}
        </button>
      </div>
    </div>
  </div>

  {#if loading}
    <p class="loading-text">{t('common.loading')}</p>
  {:else if entries.length === 0}
    <div class="empty">
      <p>{t('time_tracking.no_entries')}</p>
    </div>
  {:else}
    <div class="entries-list">
      <div class="entries-header">
        <h2>{t('time_tracking.entries')}</h2>
        <p class="total-hours">{t('time_tracking.total_hours', { hours: calcTotal() })}</p>
      </div>
      <table class="entries-table">
        <thead>
          <tr>
            <th>Date</th>
            <th>Duration</th>
            <th>Note</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each entries as entry (entry.id)}
            <tr>
              <td class="date-cell">{entry.entry_date || entry.created_at?.slice(0, 10)}</td>
              <td class="duration-cell">{entry.duration}h</td>
              <td class="note-cell">{entry.note || '—'}</td>
              <td class="actions-cell">
                <button class="btn-danger btn-sm" onclick={() => handleDelete(entry.id)}>
                  {t('common.delete')}
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    {#if totalPages > 1}
      <div class="pagination">
        <button class="btn-outline" disabled={currentPage <= 1} onclick={() => { currentPage--; loadEntries(); }}>
          {t('common.previous')}
        </button>
        <span class="page-info">Page {currentPage} of {totalPages}</span>
        <button class="btn-outline" disabled={currentPage >= totalPages} onclick={() => { currentPage++; loadEntries(); }}>
          {t('common.next')}
        </button>
      </div>
    {/if}
  {/if}
</div>

<style>
  .repo-page {
    max-width: 900px;
    margin: 0 auto;
    padding: 24px;
  }

  .page-header {
    margin-bottom: 24px;
  }

  h1 {
    font-size: 24px;
    font-weight: 600;
  }

  .error-banner {
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid var(--red-dim);
    color: var(--red);
    border-radius: var(--radius);
    padding: 10px 14px;
    font-size: 13px;
    margin-bottom: 16px;
  }

  .form-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 20px;
    margin-bottom: 24px;
  }

  h2 {
    font-size: 18px;
    font-weight: 600;
    margin: 0 0 16px 0;
  }

  .form-row {
    display: flex;
    gap: 16px;
    align-items: flex-end;
    flex-wrap: wrap;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .form-group label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
  }

  .form-group input {
    padding: 6px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-primary);
    color: var(--text-primary);
    font-size: 14px;
  }

  .form-group input[type="number"] {
    width: 80px;
  }

  .form-group input[type="date"] {
    width: 140px;
  }

  .unit {
    font-size: 12px;
    color: var(--text-muted);
    margin-left: 4px;
  }

  .flex-grow {
    flex: 1;
  }

  .form-action {
    display: flex;
    align-items: flex-end;
  }

  .btn-primary {
    padding: 6px 16px;
    background: var(--orange);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-primary:hover { background: #e09a1e; }
  .btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }

  .loading-text {
    color: var(--text-secondary);
    text-align: center;
    padding: 48px;
  }

  .empty {
    text-align: center;
    padding: 48px;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .entries-list { margin-bottom: 24px; }

  .entries-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 12px;
  }

  .total-hours {
    font-size: 14px;
    color: var(--text-secondary);
  }

  .entries-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 14px;
  }

  .entries-table th {
    text-align: left;
    padding: 6px 12px;
    border-bottom: 2px solid var(--border);
    color: var(--text-secondary);
    font-weight: 600;
    font-size: 12px;
    text-transform: uppercase;
  }

  .entries-table td {
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    color: var(--text-primary);
  }

  .date-cell { white-space: nowrap; }
  .duration-cell { white-space: nowrap; }
  .note-cell { max-width: 400px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .actions-cell {
    text-align: right;
    white-space: nowrap;
  }

  .btn-danger {
    padding: 4px 10px;
    background: var(--red-dim);
    border: 1px solid var(--red);
    border-radius: var(--radius);
    color: #fff;
    font-size: 12px;
    cursor: pointer;
  }
  .btn-danger:hover { background: var(--red); }

  .btn-sm { padding: 4px 10px; font-size: 12px; }

  .pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
    margin-top: 24px;
  }

  .btn-outline {
    padding: 5px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
  }
  .btn-outline:hover { background: var(--bg-hover); }
  .btn-outline:disabled { opacity: 0.5; cursor: not-allowed; }

  .page-info {
    font-size: 14px;
    color: var(--text-secondary);
  }

  @media (max-width: 640px) {
    .form-row {
      flex-direction: column;
      align-items: stretch;
    }
  }
</style>
