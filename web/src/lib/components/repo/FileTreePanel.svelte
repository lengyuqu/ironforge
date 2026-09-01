<script lang="ts">
  // File tree panel — pure presentation: renders tree entries (dirs are
  // clickable via onNavigateTo/onNavigateUp, files link to the blob viewer).
  import { buildBlobHref } from '$lib/utils/repoUrls';
  import { createT } from '$lib/i18n';
  import type { RepoTreeEntry } from '$lib/types/entities';

  interface Props {
    owner: string;
    repo: string;
    ref: string;
    path: string;
    entries: RepoTreeEntry[];
    onNavigateTo: (entryName: string) => void;
    onNavigateUp: () => void;
  }

  let { owner, repo, ref, path, entries, onNavigateTo, onNavigateUp }: Props = $props();

  const t = createT();

  function formatFileSize(size: number) {
    if (size < 1024) return size + t('repo.file_size.b');
    if (size < 1024 * 1024) return (size / 1024).toFixed(1) + t('repo.file_size.kb');
    return (size / (1024 * 1024)).toFixed(1) + t('repo.file_size.mb');
  }
</script>

<div class="gh-card tree-panel">
  {#if path}
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="entry" onclick={onNavigateUp} role="button" tabindex="0">
      <span class="entry-icon">📁</span>
      <span class="entry-name up">..</span>
    </div>
  {/if}
  {#each entries as entry (entry.name)}
    {#if entry.kind === 'tree' || entry.kind === 'dir'}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div class="entry" onclick={() => onNavigateTo(entry.name)} role="button" tabindex="0">
        <span class="entry-icon">📁</span>
        <span class="entry-name dir">{entry.name}</span>
      </div>
    {:else}
      <a
        href={buildBlobHref(owner, repo, ref, path ? path + '/' + entry.name : entry.name)}
        class="entry file-entry"
      >
        <span class="entry-icon">📄</span>
        <span class="entry-name">{entry.name}</span>
        {#if entry.size}
          <span class="entry-size">{formatFileSize(entry.size)}</span>
        {/if}
      </a>
    {/if}
  {/each}
</div>

<style>
  .tree-panel {
    overflow: hidden;
    padding: 0;
  }

  .entry {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    border-bottom: 1px solid var(--border-light);
    font-size: 14px;
    cursor: pointer;
    text-decoration: none;
    color: var(--text-primary);
  }
  .entry:hover { background: var(--bg-hover); }
  .file-entry { cursor: pointer; }

  .entry-icon { font-size: 14px; }
  .entry-name { flex: 1; }
  .entry-name.dir { color: var(--text-primary); font-weight: 500; }
  .entry-name.up { color: var(--text-muted); }
  .entry-size { font-size: 12px; color: var(--text-muted); font-family: var(--font-mono); }
</style>
