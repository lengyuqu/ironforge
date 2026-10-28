<script lang="ts">
  // Labels grid — pure presentation: renders label cards and delegates
  // edit/delete intents to the parent via callbacks.
  import type { Label } from '$lib/types/entities';
  import { createT } from '$lib/i18n';

  const t = createT();

  interface Props {
    items: Label[];
    onEdit: (label: Label) => void;
    onDelete: (label: Label) => void;
  }

  let { items, onEdit, onDelete }: Props = $props();
</script>

<div class="labels-grid">
  {#each items as label (label.id)}
    <div class="label-card surface">
      <div class="label-info">
        <div class="label-color" style="background-color: {label.color}"></div>
        <div class="label-text">
          <span class="label-name">{label.name}</span>
          {#if label.description}
            <span class="label-desc">{label.description}</span>
          {/if}
        </div>
      </div>
      <div class="label-actions">
        <button class="btn-icon" onclick={() => onEdit(label)} title={t('common.edit')}>
          ✏️
        </button>
        <button class="btn-icon" onclick={() => onDelete(label)} title={t('common.delete')}>
          🗑️
        </button>
      </div>
    </div>
  {/each}
</div>

<style>
  .labels-grid {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .label-card {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 1rem;
    transition: all 0.2s;
  }

  .label-card:hover {
    border-color: var(--accent);
  }

  .label-info {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .label-color {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .label-text {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .label-name {
    font-weight: 600;
    color: var(--text-primary);
    font-size: 0.95rem;
  }

  .label-desc {
    color: var(--text-secondary);
    font-size: 0.85rem;
  }

  .label-actions {
    display: flex;
    gap: 0.5rem;
    opacity: 0;
    transition: opacity 0.2s;
  }

  .label-card:hover .label-actions {
    opacity: 1;
  }
</style>
