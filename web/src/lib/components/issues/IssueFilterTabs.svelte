<script lang="ts">
  import { createT } from '$lib/i18n';
  import type { Milestone } from '$lib/types/entities';

  const t = createT();

  let {
    filter,
    onFilterChange,
    milestones,
    milestoneFilter,
    onMilestoneChange,
  }: {
    filter: string;
    onFilterChange: (filter: string) => void;
    /** When provided, an extra milestone filter chip row renders (M2-2 A1). */
    milestones?: Milestone[];
    /** Active milestone filter: '' = no constraint, 'none' = no milestone, otherwise String(milestone.id). */
    milestoneFilter?: string;
    onMilestoneChange?: (milestone: string) => void;
  } = $props();

  const tabs = [
    { value: 'open', label: t('issues.tabs.open') },
    { value: 'closed', label: t('issues.tabs.closed') },
    { value: 'all', label: t('issues.tabs.all') }
  ];

  const noMilestone = 'none';
</script>

<div class="filter-tabs">
  {#each tabs as tab (tab.value)}
    <button
      class="filter-btn btn btn-outline btn-sm"
      class:active={filter === tab.value}
      onclick={() => onFilterChange(tab.value)}
    >
      {tab.label}
    </button>
  {/each}
</div>

{#if milestones !== undefined}
  <div class="milestone-filter-row">
    <span class="milestone-filter-label">{t('issues.milestone')}</span>
    <button
      class="filter-btn btn btn-outline btn-sm"
      class:active={!milestoneFilter}
      onclick={() => onMilestoneChange?.('')}
    >
      {t('common.all')}
    </button>
    <button
      class="filter-btn btn btn-outline btn-sm"
      class:active={milestoneFilter === noMilestone}
      onclick={() => onMilestoneChange?.(noMilestone)}
    >
      {t('issues.no_milestone')}
    </button>
    {#each milestones as milestone (milestone.id)}
      <button
        class="filter-btn btn btn-outline btn-sm milestone-chip"
        class:active={milestoneFilter === String(milestone.id)}
        onclick={() => onMilestoneChange?.(String(milestone.id))}
        title={milestone.description ?? milestone.title}
      >
        {milestone.title}
        {#if (milestone.open_issues ?? 0) > 0}
          <span class="milestone-count">{milestone.open_issues}</span>
        {/if}
      </button>
    {/each}
  </div>
{/if}

<style>
  .filter-tabs {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }

  .milestone-filter-row {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: 8px;
  }

  .milestone-filter-label {
    font-size: 0.8rem;
    color: var(--text-muted);
    margin-right: 2px;
  }

  .filter-btn {
    color: var(--text-secondary);
  }

  .filter-btn.active {
    color: var(--text-primary);
    background: var(--bg-secondary);
    font-weight: 600;
  }

  .filter-btn:hover {
    background: var(--bg-hover);
    border-color: var(--text-muted);
  }

  .milestone-chip {
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .milestone-count {
    margin-left: 4px;
    padding: 0 6px;
    border-radius: 10px;
    font-size: 0.7rem;
    background: var(--bg-hover);
    color: var(--text-muted);
  }
</style>
