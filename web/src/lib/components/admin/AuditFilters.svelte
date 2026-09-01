<script lang="ts">
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    actionFilter = $bindable(''),
    resourceFilter = $bindable(''),
    onApply,
    onClear,
  }: {
    actionFilter?: string;
    resourceFilter?: string;
    onApply: () => void;
    onClear: () => void;
  } = $props();

  // Predefined action groups for the filter dropdown
  const actionGroups = [
    { value: '', label: () => t('admin.audit.filter_all') },
    { value: 'user.login', label: 'user.login' },
    { value: 'user.register', label: 'user.register' },
    { value: 'repo.create', label: 'repo.create' },
    { value: 'repo.delete', label: 'repo.delete' },
    { value: 'repo.fork', label: 'repo.fork' },
    { value: 'repo.transfer', label: 'repo.transfer' },
    { value: 'org.create', label: 'org.create' },
    { value: 'org.update', label: 'org.update' },
    { value: 'org.delete', label: 'org.delete' },
    { value: 'org.add_member', label: 'org.add_member' },
    { value: 'org.remove_member', label: 'org.remove_member' },
    { value: 'admin.update_user', label: 'admin.update_user' },
    { value: 'admin.delete_user', label: 'admin.delete_user' },
    { value: 'admin.delete_org', label: 'admin.delete_org' },
  ];
</script>

<div class="filters">
  <select bind:value={actionFilter} onchange={onApply}>
    {#each actionGroups as g}
      <option value={g.value}>{typeof g.label === 'function' ? g.label() : t(g.label)}</option>
    {/each}
  </select>
  <select bind:value={resourceFilter} onchange={onApply}>
    <option value="">{t('admin.audit.fields.resource_type')}: All</option>
    <option value="user">User</option>
    <option value="repo">Repository</option>
    <option value="org">Organization</option>
  </select>
  {#if actionFilter || resourceFilter}
    <button class="btn-sm" onclick={onClear}>Clear filters</button>
  {/if}
</div>

<style>
  .filters {
    display: flex;
    gap: 0.75rem;
    align-items: center;
    margin-bottom: 1rem;
    flex-wrap: wrap;
  }
  .filters select {
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.4rem 0.75rem;
    font-size: 0.85rem;
  }
  .filters .btn-sm {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    color: var(--text-secondary);
    border-radius: 4px;
    padding: 0.3rem 0.6rem;
    font-size: 0.8rem;
    cursor: pointer;
  }
  .filters .btn-sm:hover { color: var(--text-primary); background: var(--bg-hover); }
</style>
