<script lang="ts">
  // "Target branch / commit" field: a tags|branches toggle over one select.
  import { createT } from '$lib/i18n';

  interface Props {
    tags: string[];
    branches: string[];
    /** Selected ref, or '' for "none". */
    value: string;
  }

  let { tags, branches, value = $bindable() }: Props = $props();

  const t = createT();

  let targetType = $state<'branch' | 'tag'>('tag');
</script>

<div class="form-group">
  <label for="target-commitish">{t('releases.target_commitish')}</label>
  <div class="target-toggle">
    <button
      type="button"
      class="toggle-btn"
      class:active={targetType === 'tag'}
      onclick={() => (targetType = 'tag')}
    >
      {t('releases.target_tags')}
    </button>
    <button
      type="button"
      class="toggle-btn"
      class:active={targetType === 'branch'}
      onclick={() => (targetType = 'branch')}
    >
      {t('releases.target_branches')}
    </button>
  </div>

  <select id="target-commitish" bind:value class="select">
    {#if targetType === 'tag'}
      <option value="">{t('releases.select_tag')}</option>
      {#each tags as tag}
        <option value={tag}>{tag}</option>
      {/each}
    {:else}
      <option value="">{t('releases.select_branch')}</option>
      {#each branches as branch}
        <option value={branch}>{branch}</option>
      {/each}
    {/if}
  </select>
</div>

<style>
  .select {
    width: 100%;
    box-sizing: border-box;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%23888' d='M6 8L1 3h10z'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 10px center;
    padding-right: 30px;
  }

  .target-toggle {
    display: flex;
    gap: 0;
    margin-bottom: 8px;
  }

  .toggle-btn {
    padding: 6px 16px;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
    background: var(--bg-primary);
    border: 1px solid var(--border);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .toggle-btn:first-child {
    border-radius: var(--radius) 0 0 var(--radius);
  }

  .toggle-btn:last-child {
    border-radius: 0 var(--radius) var(--radius);
    border-left: none;
  }

  .toggle-btn.active {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--text-on-accent);
  }
</style>
