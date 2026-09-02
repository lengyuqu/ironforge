<script lang="ts">
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    filter,
    onFilterChange,
  }: {
    filter: string;
    onFilterChange: (filter: string) => void;
  } = $props();

  const tabs = [
    { value: 'open', label: t('issues.tabs.open') },
    { value: 'closed', label: t('issues.tabs.closed') },
    { value: 'all', label: t('issues.tabs.all') }
  ];
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

<style>
  .filter-tabs {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
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
</style>
