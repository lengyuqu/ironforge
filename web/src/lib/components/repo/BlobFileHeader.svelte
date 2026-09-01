<script lang="ts">
  // Blob file header — filename, line/size meta and the action row
  // (rendered/source toggle, copy path/link, edit link, delete toggle).
  // Copy actions are self-contained clipboard operations.
  import { browser } from '$app/environment';
  import { createT } from '$lib/i18n';
  import { buildBlobHref } from '$lib/utils/repoUrls';

  interface Props {
    owner: string;
    repo: string;
    ref: string;
    filePath: string;
    isText: boolean;
    isMarkdown: boolean;
    lineCount: number;
    size: number;
    canEdit: boolean;
    viewMode: 'rendered' | 'source';
    sha?: string;
    onToggleView: () => void;
    onToggleDelete: () => void;
  }

  let {
    owner,
    repo,
    ref,
    filePath,
    isText,
    isMarkdown,
    lineCount,
    size,
    canEdit,
    viewMode,
    sha,
    onToggleView,
    onToggleDelete
  }: Props = $props();

  const t = createT();

  let copyStatus = $state('');

  const editHref = $derived.by(() => {
    const params = new URLSearchParams();
    if (sha) params.set('sha', sha);
    if (ref) params.set('ref', ref);
    const qs = params.toString();
    return `/${owner}/${repo}/edit/${filePath
      .split('/')
      .map(encodeURIComponent)
      .join('/')}${qs ? `?${qs}` : ''}`;
  });

  function formatFileSize(value: number) {
    if (value < 1024) return value + t('repo.file_size.b');
    if (value < 1024 * 1024) return (value / 1024).toFixed(1) + t('repo.file_size.kb');
    return (value / (1024 * 1024)).toFixed(1) + t('repo.file_size.mb');
  }

  async function copyText(value: string, label: string) {
    try {
      await navigator.clipboard.writeText(value);
      copyStatus = label;
      setTimeout(() => {
        copyStatus = '';
      }, 1600);
    } catch (err) {
      copyStatus = err instanceof Error ? err.message : String(err);
    }
  }

  function copyPath() {
    copyText(filePath, t('repo.blob.copied'));
  }

  function copyLink() {
    const href = buildBlobHref(owner, repo, ref, filePath);
    const absolute = browser ? `${window.location.origin}${href}` : href;
    copyText(absolute, t('repo.blob.copied'));
  }
</script>

<div class="file-header">
  <div class="file-info">
    <span class="file-path">{filePath.split('/').pop()}</span>
  </div>
  <div class="file-meta">
    {#if isText}
      <span class="file-lines">{t('repo.lines_count', { count: lineCount })}</span>
    {/if}
    <span class="file-size">({formatFileSize(size)})</span>
  </div>
  <div class="file-actions">
    {#if isMarkdown && isText}
      <button type="button" class="btn-outline btn-sm" onclick={onToggleView}>
        {viewMode === 'rendered' ? t('repo.blob.raw') : t('repo.blob.rendered')}
      </button>
    {/if}
    <button type="button" class="btn-outline btn-sm" onclick={copyPath}>
      {t('repo.blob.copy_path')}
    </button>
    <button type="button" class="btn-outline btn-sm" onclick={copyLink}>
      {t('repo.blob.copy_link')}
    </button>
    {#if canEdit}
      <a href={editHref} class="btn-outline btn-sm">
        {t('repo.edit_file')}
      </a>
    {:else}
      <span class="btn-outline btn-sm disabled" title={t('repo.blob.edit_unavailable')}>
        {t('repo.edit_file')}
      </span>
    {/if}
    <button type="button" class="btn-outline btn-sm danger" onclick={onToggleDelete}>
      {t('repo.blob.delete_file')}
    </button>
  </div>
</div>

{#if copyStatus}
  <div class="copy-status">{copyStatus}</div>
{/if}

<style>
  .file-header {
    display: grid;
    grid-template-columns: minmax(160px, 1fr) auto minmax(280px, auto);
    align-items: center;
    gap: 12px;
    padding: 10px 16px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-bottom: none;
    border-radius: var(--radius) var(--radius) 0 0;
    font-size: 13px;
  }

  .file-info {
    min-width: 0;
  }

  .file-path {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
    font-weight: 600;
  }

  .file-meta {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text-muted);
    font-size: 12px;
    white-space: nowrap;
  }

  .file-lines {
    color: var(--text-secondary);
  }

  .file-actions {
    display: flex;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 6px;
  }

  .btn-outline {
    border-radius: 6px;
    min-height: 32px;
    padding: 5px 10px;
    font-size: 13px;
    text-decoration: none;
    cursor: pointer;
    border: 1px solid var(--border);
    background: var(--bg-primary);
    color: var(--text-primary);
  }

  .btn-outline:hover {
    background: var(--bg-tertiary);
  }

  .btn-outline.disabled {
    opacity: 0.55;
    cursor: not-allowed;
    pointer-events: none;
  }

  .btn-outline.danger {
    color: #cf222e;
  }

  .copy-status {
    border: 1px solid var(--border);
    border-bottom: none;
    background: var(--bg-primary);
    padding: 10px 16px;
    font-size: 13px;
    color: var(--text-secondary);
  }

  @media (max-width: 820px) {
    .file-header {
      grid-template-columns: 1fr;
      align-items: stretch;
    }

    .file-actions {
      justify-content: flex-start;
    }
  }
</style>
