<script lang="ts">
  // Issue milestone panel — self-contained: loads the repo milestone list,
  // shows the issue's current milestone (title resolved from the list —
  // backend returns only milestone_id, see gap-analysis §A3), and persists
  // set/clear via issues.update PATCH (null clears the association).
  // Errors are surfaced as toasts.
  import { issues, milestones } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Milestone } from '$lib/types/entities';

  interface Props {
    owner: string;
    repo: string;
    issueNumber: number;
    /** Current milestone_id from the issue payload. */
    milestoneId: number | null | undefined;
    /** Called after a successful set/clear so the page can refresh. */
    onChanged: () => void | Promise<void>;
  }

  let { owner, repo, issueNumber, milestoneId, onChanged }: Props = $props();

  const t = createT();

  let milestoneList = $state<Milestone[]>([]);
  let currentTitle = $state('');
  let editing = $state(false);
  let selected = $state<number | ''>('');
  let saving = $state(false);

  // Open milestones plus the currently-linked one (it may be closed).
  let selectable = $state<Milestone[]>([]);

  $effect(() => {
    loadMilestones();
  });

  async function loadMilestones() {
    try {
      milestoneList = await milestones.list(owner, repo);
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
      milestoneList = [];
    }
  }

  $effect(() => {
    const linked = milestoneId != null ? milestoneList.find((m) => m.id === milestoneId) : null;
    currentTitle = linked ? linked.title : milestoneId != null ? `#${milestoneId}` : '';
    selectable =
      milestoneId != null
        ? milestoneList.filter((m) => m.state === 'open' || m.id === milestoneId)
        : milestoneList.filter((m) => m.state === 'open');
  });

  function startEdit() {
    selected = milestoneId ?? '';
    editing = true;
  }

  async function saveMilestone() {
    try {
      saving = true;
      // Explicit null clears the association (Some(None) semantics).
      await issues.update(owner, repo, issueNumber, {
        milestone_id: selected === '' ? null : selected,
      });
      toast.success(t('issues.milestone_saved', 'Milestone updated'));
      editing = false;
      await onChanged();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e, t('errors.update_failed', 'Update failed')));
    } finally {
      saving = false;
    }
  }
</script>

<div class="milestone-panel">
  <div class="panel-header">
    <span class="panel-title">{t('issues.milestone')}</span>
    {#if !editing}
      <button type="button" class="btn-edit" onclick={startEdit}>
        {t('issues.milestone_edit')}
      </button>
    {/if}
  </div>

  {#if editing}
    <div class="editor">
      <select bind:value={selected} disabled={saving}>
        <option value="">{t('issues.milestone_none')}</option>
        {#each selectable as m (m.id)}
          <option value={m.id}>{m.title}{m.state === 'closed' ? ` (${m.state})` : ''}</option>
        {/each}
      </select>
      <div class="editor-actions">
        <button type="button" class="btn-primary" disabled={saving} onclick={saveMilestone}>
          {t('issues.milestone_save')}
        </button>
        <button type="button" class="btn-close" disabled={saving} onclick={() => (editing = false)}>
          {t('common.cancel', 'Cancel')}
        </button>
      </div>
    </div>
  {:else}
    <div class="current">
      {#if currentTitle}
        <span class="current-title">{currentTitle}</span>
      {:else}
        <span class="current-none">{t('issues.milestone_none')}</span>
      {/if}
    </div>
  {/if}
</div>

<style>
  .milestone-panel {
    padding: 12px 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-primary);
    margin-bottom: 16px;
  }

  .panel-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 8px;
  }

  .panel-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .btn-edit {
    background: none;
    border: none;
    color: var(--accent);
    font-size: 12px;
    cursor: pointer;
    padding: 0;
  }

  .btn-edit:hover {
    text-decoration: underline;
  }

  .editor {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .editor select {
    padding: 6px 8px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-primary);
    font-size: 13px;
  }

  .editor-actions {
    display: flex;
    gap: 8px;
  }

  .btn-primary {
    padding: 4px 12px;
    background: var(--accent);
    color: #fff;
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    font-size: 12px;
    cursor: pointer;
  }

  .btn-primary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .btn-close {
    padding: 4px 12px;
    background: none;
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 12px;
    cursor: pointer;
  }

  .current-title {
    font-size: 13px;
    color: var(--text-primary);
    font-weight: 500;
  }

  .current-none {
    font-size: 13px;
    color: var(--text-muted);
  }
</style>
