<script lang="ts">
  // Milestones grid — pure presentation: renders milestone cards and
  // delegates edit/delete intents to the parent via callbacks.
  import { createT, formatDate } from '$lib/i18n';
  import type { Milestone } from '$lib/types/entities';

  interface Props {
    items: Milestone[];
    onEdit: (milestone: Milestone) => void;
    onDelete: (milestone: Milestone) => void;
  }

  let { items, onEdit, onDelete }: Props = $props();

  const t = createT();
</script>

<div class="milestones-grid">
  {#each items as milestone (milestone.id)}
    <div class="milestone-card">
      <div class="milestone-info">
        <div class="milestone-text">
          <div class="milestone-title-row">
            <span class="milestone-name">{milestone.title}</span>
            <span class="state-badge" class:closed={milestone.state === 'closed'}>
              {milestone.state}
            </span>
          </div>
          {#if milestone.description}
            <span class="milestone-desc">{milestone.description}</span>
          {/if}
          {#if milestone.due_date}
            <span class="milestone-due">
              {t('settings.milestone_due')}: {formatDate(milestone.due_date)}
            </span>
          {/if}
        </div>
      </div>
      <div class="milestone-actions">
        <button class="btn-icon" onclick={() => onEdit(milestone)} title={t('settings.edit_milestone')}>
          ✏️
        </button>
        <button class="btn-icon" onclick={() => onDelete(milestone)} title={t('settings.delete_milestone')}>
          🗑️
        </button>
      </div>
    </div>
  {/each}
</div>

<style>
  .milestones-grid {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .milestone-card {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 1rem;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
    transition: all 0.2s;
  }

  .milestone-card:hover {
    border-color: var(--accent);
  }

  .milestone-info {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    min-width: 0;
  }

  .milestone-text {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
  }

  .milestone-title-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .milestone-name {
    font-weight: 600;
    color: var(--text-primary);
    font-size: 0.95rem;
  }

  .state-badge {
    font-size: 0.75rem;
    padding: 0.1rem 0.5rem;
    border-radius: 10px;
    color: #1a7f37;
    background: rgba(26, 127, 55, 0.12);
    text-transform: capitalize;
  }

  .state-badge.closed {
    color: #cf222e;
    background: rgba(207, 34, 46, 0.12);
  }

  .milestone-desc {
    color: var(--text-secondary);
    font-size: 0.85rem;
    overflow-wrap: anywhere;
  }

  .milestone-due {
    color: var(--text-secondary);
    font-size: 0.8rem;
  }

  .milestone-actions {
    display: flex;
    gap: 0.5rem;
    opacity: 0;
    transition: opacity 0.2s;
    flex-shrink: 0;
  }

  .milestone-card:hover .milestone-actions {
    opacity: 1;
  }

  .btn-icon {
    background: none;
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.25rem 0.5rem;
    cursor: pointer;
    font-size: 0.9rem;
    transition: all 0.2s;
  }

  .btn-icon:hover {
    background: var(--bg-primary);
    border-color: var(--accent);
  }
</style>
