<script lang="ts">
  import { timeTracking } from '$lib/api/client.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
    issueNumber,
    onAdded,
  }: {
    owner: string;
    repo: string;
    issueNumber: number;
    onAdded: () => void | Promise<void>;
  } = $props();

  let durationHours = $state<number>(1);
  let description = $state('');
  let saving = $state(false);
  let error = $state('');

  async function handleAdd() {
    if (durationHours <= 0 || saving) return;
    try {
      saving = true;
      error = '';
      await timeTracking.add(owner, repo, issueNumber, {
        duration_minutes: Math.round(durationHours * 60),
        description: description || undefined,
      });
      durationHours = 1;
      description = '';
      await onAdded();
    } catch (e: any) {
      error = toErrorMessage(e, 'Failed to add time entry');
    } finally {
      saving = false;
    }
  }
</script>

<div class="form-card">
  <h3>{t('repo.time_tracking.add_entry')}</h3>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  <div class="form-row">
    <div class="form-group">
      <label for="tt-dur">Duration (hours)</label>
      <input
        id="tt-dur"
        type="number"
        min="0.25"
        step="0.25"
        bind:value={durationHours}
        disabled={saving}
      />
    </div>
    <div class="form-group flex-grow">
      <label for="tt-desc">Note</label>
      <input
        id="tt-desc"
        type="text"
        placeholder="(optional)"
        bind:value={description}
        disabled={saving}
      />
    </div>
    <div class="form-action">
      <button class="btn-primary" onclick={handleAdd} disabled={saving || durationHours <= 0}>
        {saving ? '…' : 'Add'}
      </button>
    </div>
  </div>
</div>

<style>
  .form-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
    margin-bottom: 20px;
  }

  h3 {
    font-size: 14px;
    font-weight: 600;
    margin: 0 0 12px;
  }

  .form-row {
    display: flex;
    gap: 12px;
    align-items: flex-end;
    flex-wrap: wrap;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .form-group label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
  }

  .form-group input {
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-primary);
    color: var(--text-primary);
    font-size: 14px;
  }

  .form-group input[type='number'] {
    width: 90px;
  }

  .flex-grow {
    flex: 1;
  }

  .form-action {
    display: flex;
    align-items: flex-end;
  }

  .btn-primary {
    padding: 6px 14px;
    background: var(--accent);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .btn-primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .error-banner {
    padding: 8px 12px;
    border-radius: var(--radius);
    margin-bottom: 12px;
    font-size: 13px;
    color: var(--red, #f85149);
    background: rgba(248, 81, 73, 0.1);
  }
</style>
