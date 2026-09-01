<script lang="ts">
  // Assignees panel — self-contained: loads the assignee list via the issues
  // API, manages the inline editor, and persists changes with setAssignees.
  // Errors are surfaced as toasts (page-level load failures keep their banner).
  import { issues } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';

  interface Props {
    owner: string;
    repo: string;
    issueNumber: number;
  }

  let { owner, repo, issueNumber }: Props = $props();

  const t = createT();

  let assignees = $state<string[]>([]);
  let editing = $state(false);
  let assigneeInput = $state('');
  let saving = $state(false);

  $effect(() => {
    loadAssignees();
  });

  async function loadAssignees() {
    try {
      const result = await issues.listAssignees(owner, repo, issueNumber);
      assignees = result?.assignees || [];
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    }
  }

  function startEdit() {
    assigneeInput = assignees.join(', ');
    editing = true;
  }

  async function saveAssignees() {
    try {
      saving = true;
      const names = assigneeInput
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean);
      const result = await issues.setAssignees(owner, repo, issueNumber, names);
      assignees = result?.assignees || [];
      editing = false;
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      saving = false;
    }
  }
</script>

<div class="assignees-panel">
  <div class="assignees-header">
    <span class="assignees-title">{t('issues.assignees.title')}</span>
    {#if !editing}
      <button type="button" class="btn-assignee-edit" onclick={startEdit}>
        {t('issues.assignees.edit')}
      </button>
    {/if}
  </div>
  {#if editing}
    <div class="assignees-editor">
      <input
        type="text"
        bind:value={assigneeInput}
        placeholder={t('issues.assignees.placeholder')}
      />
      <div class="assignees-editor-actions">
        <button
          type="button"
          class="btn-primary"
          disabled={saving}
          onclick={saveAssignees}
        >
          {t('issues.assignees.save')}
        </button>
        <button
          type="button"
          class="btn-close"
          disabled={saving}
          onclick={() => (editing = false)}
        >
          {t('issues.assignees.cancel')}
        </button>
      </div>
      <p class="assignees-hint">{t('issues.assignees.hint')}</p>
    </div>
  {:else}
    {#if assignees.length}
      <div class="assignees-list">
        {#each assignees as name, i (name)}
          <span
            class="assignee-badge"
            class:primary={i === 0}
            title={i === 0 ? t('issues.assignees.hint') : name}
          >
            {name}
          </span>
        {/each}
      </div>
    {:else}
      <p class="assignees-empty">{t('issues.assignees.empty')}</p>
    {/if}
  {/if}
</div>

<style>
  .assignees-panel {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px 16px;
    margin-bottom: 16px;
  }

  .assignees-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }

  .assignees-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .btn-assignee-edit {
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 12px;
    cursor: pointer;
    padding: 2px 6px;
    border-radius: var(--radius);
  }
  .btn-assignee-edit:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .assignees-list {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .assignee-badge {
    display: inline-block;
    padding: 2px 10px;
    border: 1px solid var(--border);
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border-radius: 12px;
    font-size: 12px;
  }

  .assignee-badge.primary {
    border-color: var(--green);
    color: var(--green);
    background: rgba(63, 185, 80, 0.12);
  }

  .assignees-empty {
    font-size: 13px;
    color: var(--text-muted);
    margin: 0;
  }

  .assignees-editor input {
    width: 100%;
    font-size: 13px;
    padding: 6px 10px;
    margin-bottom: 8px;
  }

  .assignees-editor-actions {
    display: flex;
    gap: 8px;
  }

  .assignees-hint {
    font-size: 12px;
    color: var(--text-muted);
    margin: 8px 0 0;
  }

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
  .btn-primary:disabled { opacity: 0.5; }

  .btn-close {
    padding: 6px 16px;
    background: none;
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 14px;
    cursor: pointer;
  }
  .btn-close:hover { background: var(--bg-hover); }
</style>
