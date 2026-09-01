<script lang="ts">
  // Repo browser toolbar — branch selector dropdown, "new file" action and
  // path breadcrumb. Pure presentation: navigation intent is delegated via
  // onSelectBranch; breadcrumb/tree links are built from props.
  import Dropdown from '$lib/components/Dropdown.svelte';
  import { buildTreeHref } from '$lib/utils/repoUrls';
  import { createT } from '$lib/i18n';
  import type { Branch } from '$lib/types/entities';

  interface Props {
    owner: string;
    repo: string;
    ref: string;
    path: string;
    branches: Branch[];
    currentRefLabel: string;
    onSelectBranch: (branchName: string) => void;
  }

  let { owner, repo, ref, path, branches, currentRefLabel, onSelectBranch }: Props = $props();

  const t = createT();

  function selectBranch(branchName: string, close: () => void) {
    onSelectBranch(branchName);
    close();
  }
</script>

<div class="repo-toolbar">
  <div class="branch-selector">
    <Dropdown ariaLabel={t('repo.select_branch')} triggerClass="btn-outline" placement="left">
      {#snippet trigger()}
        🌿 {currentRefLabel} <span aria-hidden="true">▾</span>
      {/snippet}
      {#snippet menu(close)}
        {#each branches as b (b.name)}
          <button
            class="dropdown-item"
            class:active={b.name === ref || (!ref && b.is_default)}
            onclick={() => selectBranch(b.name, close)}
            role="menuitem"
          >
            {b.name} {b.is_default ? t('repo.browser.default_branch') : ''}
          </button>
        {/each}
      {/snippet}
    </Dropdown>
  </div>

  <div class="toolbar-actions">
    <a href={`/${owner}/${repo}/new`} class="btn-outline btn-sm">
      ➕ {t('repo.new_file') || 'New file'}
    </a>
  </div>

  <div class="breadcrumb">
    <a href={buildTreeHref(owner, repo, ref, '')}>{repo}</a>
    {#if path}
      {#each path.split('/') as part}
        <span class="sep">/</span>
        <span>{part}</span>
      {/each}
    {/if}
  </div>
</div>

<style>
  .repo-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 16px;
  }

  .breadcrumb {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 14px;
  }
  .breadcrumb a { color: var(--accent); font-weight: 600; }
  .sep { color: var(--text-muted); }
</style>
