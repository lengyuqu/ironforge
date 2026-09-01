<script lang="ts">
  // Release card — renders one release with badges, body preview, assets and
  // actions. Delete confirmation and asset download are self-contained
  // (API + toast); edit/browse links are built from owner/repo.
  import { releases } from '$lib/api/client.svelte';
  import { createT, formatDate } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Release } from '$lib/types/entities';
  import type { ReleaseAsset } from '$lib/api/releases';

  interface Props {
    owner: string;
    repo: string;
    release: Release;
    assets: ReleaseAsset[];
    isLatest?: boolean;
    /** Called after a successful delete so the page can reload. */
    onChanged: () => void;
  }

  let { owner, repo, release, assets, isLatest = false, onChanged }: Props = $props();

  const t = createT();

  let confirmDelete = $state(false);
  let deleting = $state(false);
  let downloadingAssetId = $state<number | null>(null);

  function buildBrowseLink(tag: string) {
    const params = new URLSearchParams();
    if (tag) params.set('ref', tag);
    const qs = params.toString();
    return `/${owner}/${repo}${qs ? `?${qs}` : ''}`;
  }

  function buildEditReleaseLink(id: number) {
    return `/${owner}/${repo}/releases/edit/${id}`;
  }

  function relativeTime(dateStr: string): string {
    const date = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffSecs = Math.floor(diffMs / 1000);
    const diffMins = Math.floor(diffSecs / 60);
    const diffHours = Math.floor(diffMins / 60);
    const diffDays = Math.floor(diffHours / 24);

    if (diffDays > 30) return formatDate(dateStr);
    if (diffDays > 0) return `${diffDays} day${diffDays > 1 ? 's' : ''} ago`;
    if (diffHours > 0) return `${diffHours} hour${diffHours > 1 ? 's' : ''} ago`;
    if (diffMins > 0) return `${diffMins} minute${diffMins > 1 ? 's' : ''} ago`;
    return 'just now';
  }

  function formatBytes(size: number): string {
    if (size < 1024) return `${size} B`;
    if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
    return `${(size / (1024 * 1024)).toFixed(1)} MB`;
  }

  async function handleDelete() {
    try {
      deleting = true;
      await releases.delete(owner, repo, release.id);
      toast.success(t('releases.deleted', 'Release deleted'));
      onChanged();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e, t('errors.delete_failed', 'Delete failed')));
    } finally {
      deleting = false;
    }
  }

  async function handleAssetDownload(asset: ReleaseAsset) {
    try {
      downloadingAssetId = asset.id;
      await releases.downloadAsset(owner, repo, asset.id, asset.filename);
    } catch (e: unknown) {
      toast.error(toErrorMessage(e, t('releases.download_failed', 'Download failed')));
    } finally {
      downloadingAssetId = null;
    }
  }
</script>

<div class="release-card">
  <div class="release-header">
    <div class="tag-section">
      <span class="tag-badge">🏷 {release.tag_name}</span>
      {#if isLatest}
        <span class="badge latest">{t('releases.latest')}</span>
      {/if}
      {#if release.is_prerelease}
        <span class="badge prerelease">{t('releases.prerelease')}</span>
      {/if}
      {#if release.is_draft}
        <span class="badge draft">{t('releases.draft')}</span>
      {/if}
    </div>
  </div>

  <h2 class="release-title">{release.title}</h2>

  {#if release.body}
    <p class="release-body">{release.body.slice(0, 200)}{release.body.length > 200 ? '...' : ''}</p>
  {/if}

  <div class="release-meta">
    <span class="release-date">{t('releases.created', { date: relativeTime(release.created_at || '') })}</span>
  </div>

  {#if assets.length}
    <div class="asset-list" aria-label="Release assets">
      {#each assets as asset (asset.id)}
        <button
          type="button"
          class="asset-link"
          onclick={() => handleAssetDownload(asset)}
          disabled={downloadingAssetId === asset.id}
        >
          <span class="asset-name">{asset.filename}</span>
          <span class="asset-meta">{formatBytes(asset.size)} · {asset.download_count || 0} downloads</span>
        </button>
      {/each}
    </div>
  {/if}

  <div class="release-actions">
    <a href={buildBrowseLink(release.tag_name)} class="action-link">{t('releases.browse_files')}</a>
    <a href={buildEditReleaseLink(release.id)} class="action-link">{t('releases.edit')}</a>

    {#if confirmDelete}
      <div class="delete-confirm">
        <span>Are you sure?</span>
        <button class="btn-danger" onclick={handleDelete} disabled={deleting}>
          {deleting ? '...' : t('common.delete')}
        </button>
        <button class="btn-secondary" onclick={() => (confirmDelete = false)}>{t('common.cancel')}</button>
      </div>
    {:else}
      <button class="action-link danger" onclick={() => (confirmDelete = true)}>{t('releases.delete')}</button>
    {/if}
  </div>
</div>

<style>
  .release-card {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 20px;
  }

  .release-header {
    margin-bottom: 12px;
  }

  .tag-section {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .tag-badge {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .badge {
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 12px;
    font-weight: 600;
  }

  .badge.latest {
    background: var(--green-dim);
    color: #fff;
  }

  .badge.prerelease {
    background: var(--yellow-dim);
    color: #fff;
  }

  .badge.draft {
    background: var(--bg-tertiary);
    color: var(--text-muted);
    border: 1px solid var(--border);
  }

  .release-title {
    font-size: 18px;
    font-weight: 600;
    margin-bottom: 8px;
  }

  .release-body {
    font-size: 14px;
    color: var(--text-secondary);
    line-height: 1.6;
    margin-bottom: 12px;
    white-space: pre-wrap;
  }

  .release-meta {
    font-size: 13px;
    color: var(--text-muted);
    margin-bottom: 12px;
  }

  .asset-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 12px;
    padding: 10px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .asset-link {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    width: 100%;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-primary);
    text-decoration: none;
    font-size: 13px;
    cursor: pointer;
    text-align: left;
  }

  .asset-link:hover .asset-name {
    color: var(--accent);
    text-decoration: underline;
  }

  .asset-link:disabled {
    cursor: wait;
    opacity: 0.65;
  }

  .asset-name {
    min-width: 0;
    overflow-wrap: anywhere;
    font-weight: 500;
  }

  .asset-meta {
    flex-shrink: 0;
    color: var(--text-muted);
    font-size: 12px;
  }

  .release-actions {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }

  .action-link {
    font-size: 13px;
    color: var(--accent);
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
  }
  .action-link:hover { text-decoration: underline; }
  .action-link.danger { color: var(--red); }

  .delete-confirm {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .delete-confirm span { color: var(--text-secondary); }

  .btn-secondary {
    padding: 5px 12px;
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
  }
  .btn-secondary:hover { background: var(--bg-hover); }

  .btn-danger {
    padding: 5px 12px;
    background: var(--red-dim);
    border: 1px solid var(--red);
    border-radius: var(--radius);
    color: #fff;
    font-size: 13px;
    cursor: pointer;
  }
  .btn-danger:hover { background: var(--red); }
  .btn-danger:disabled { opacity: 0.5; cursor: not-allowed; }

  @media (max-width: 600px) {
    .release-actions {
      flex-direction: column;
      align-items: flex-start;
    }

    .delete-confirm {
      flex-direction: column;
      align-items: flex-start;
    }
  }
</style>
