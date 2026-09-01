<script lang="ts">
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repos,
  }: {
    owner: string;
    repos: { id: number; name: string; description: string | null; is_private: boolean; created_at: string }[];
  } = $props();
</script>

<div class="repo-list">
  {#each repos as repo (repo.id)}
    <a href={`/${owner}/${repo.name}`} class="repo-item">
      <div class="repo-icon">
        {repo.is_private ? '🔒' : '📂'}
      </div>
      <div class="repo-info">
        <div class="repo-name">
          {owner}/{repo.name}
          {#if repo.is_private}
            <span class="badge-private">{t('dashboard.repo.private')}</span>
          {/if}
        </div>
        <div class="repo-desc">{repo.description || t('common.no_description')}</div>
        <div class="repo-meta">{t('common.created', { date: formatDate(repo.created_at) })}</div>
      </div>
    </a>
  {/each}
</div>

<style>
  .repo-list {
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  .repo-item {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 16px 20px;
    border-bottom: 1px solid var(--border-light);
    text-decoration: none;
    color: var(--text-primary);
  }
  .repo-item:hover { background: var(--bg-secondary); text-decoration: none; }
  .repo-item:first-child { border-top: 1px solid var(--border-light); }

  .repo-icon { font-size: 20px; margin-top: 2px; }

  .repo-info { flex: 1; }

  .repo-name {
    font-weight: 600;
    font-size: 15px;
    color: var(--accent);
  }

  .badge-private {
    font-size: 11px;
    font-weight: 500;
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--text-secondary);
    margin-left: 8px;
    vertical-align: middle;
  }

  .repo-desc {
    font-size: 13px;
    color: var(--text-secondary);
    margin-top: 2px;
  }

  .repo-meta {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 4px;
  }
</style>
