<script lang="ts">
  // File tree panel — pure presentation: renders tree entries (dirs link via
  // buildTreeHref for keyboard/middle-click support and intercept plain clicks
  // via onNavigateTo/onNavigateUp; files link to the blob viewer).
  import { buildBlobHref, buildTreeHref } from '$lib/utils/repoUrls';
  import { createT } from '$lib/i18n';
  import Icon from '$lib/components/Icon.svelte';
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

  const parentPath = $derived.by(() => {
    if (!path) return '';
    const parts = path.split('/');
    parts.pop();
    return parts.join('/');
  });

  function formatFileSize(size: number) {
    if (size < 1024) return size + t('repo.file_size.b');
    if (size < 1024 * 1024) return (size / 1024).toFixed(1) + t('repo.file_size.kb');
    return (size / (1024 * 1024)).toFixed(1) + t('repo.file_size.mb');
  }

  function handleTreeClick(e: MouseEvent, fn: () => void) {
    if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey || e.button !== 0) return;
    e.preventDefault();
    fn();
  }
</script>

<div class="gh-card tree-panel">
  {#if path}
    <a
      href={buildTreeHref(owner, repo, ref, parentPath)}
      class="entry"
      onclick={(e) => handleTreeClick(e, onNavigateUp)}
    >
      <span class="entry-icon"><Icon name="folder-up" size={15} /></span>
      <span class="entry-name up">..</span>
    </a>
  {/if}
  {#each entries as entry (entry.name)}
    {#if entry.kind === 'tree' || entry.kind === 'dir'}
      <a
        href={buildTreeHref(owner, repo, ref, path ? `${path}/${entry.name}` : entry.name)}
        class="entry"
        onclick={(e) => handleTreeClick(e, () => onNavigateTo(entry.name))}
      >
        <span class="entry-icon dir-icon"><Icon name="folder" size={15} /></span>
        <span class="entry-name dir">{entry.name}</span>
      </a>
    {:else}
      <a
        href={buildBlobHref(owner, repo, ref, path ? path + '/' + entry.name : entry.name)}
        class="entry file-entry"
      >
        <span class="entry-icon"><Icon name="file" size={15} /></span>
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
  .entry:last-child {
    border-bottom: none;
  }
  .entry:hover {
    background: var(--bg-hover);
    text-decoration: none;
  }

  .entry-icon {
    display: inline-flex;
    align-items: center;
    color: var(--text-secondary);
  }
  .entry-icon.dir-icon {
    color: var(--accent);
  }
  .entry-name { flex: 1; }
  .entry-name.dir { color: var(--text-primary); font-weight: 500; }
  .entry-name.up { color: var(--text-muted); }
  .entry-size { font-size: 12px; color: var(--text-muted); font-family: var(--font-mono); }
</style>
