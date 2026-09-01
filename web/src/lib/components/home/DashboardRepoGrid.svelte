<script lang="ts">
  // Dashboard repo card grid — shown on the home page for logged-in users.
  // Renders the user's explore-loaded repo cards with loading / empty states.
  import { createT, formatDate } from '$lib/i18n';
  import type { ExploreRepo } from '$lib/types/entities';

  interface Props {
    repos: ExploreRepo[];
    loading: boolean;
  }

  let { repos, loading }: Props = $props();

  const t = createT();
</script>

{#if loading}
  <p class="text-secondary">{t('common.loading')}</p>
{:else if repos.length === 0}
  <div class="empty">
    <p>{t('dashboard.empty.no_repos')}</p>
    <p class="text-secondary">{t('dashboard.empty.get_started')}</p>
  </div>
{:else}
  <div class="repo-grid">
    {#each repos as repo (repo.id)}
      <a href={`/${repo.owner_name || 'unknown'}/${repo.name}`} class="repo-card">
        <div class="rc-icon">📂</div>
        <div class="rc-body">
          <div class="rc-name">{repo.owner_name || 'unknown'}/{repo.name}</div>
          <div class="rc-desc">{repo.description || t('common.no_description')}</div>
          <div class="rc-meta">
            ⭐ {repo.stars_count || 0} · {t('common.updated', { date: formatDate(repo.updated_at) })}
          </div>
        </div>
      </a>
    {/each}
  </div>
{/if}

<style>
  .repo-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
    margin-bottom: 24px;
  }

  @media (max-width: 900px) {
    .repo-grid {
      grid-template-columns: repeat(2, 1fr);
    }
  }

  @media (max-width: 600px) {
    .repo-grid {
      grid-template-columns: 1fr;
    }
  }

  .repo-card {
    display: flex;
    gap: 12px;
    padding: 16px;
    border: 1px solid var(--border, #d0d7de);
    border-radius: 8px;
    text-decoration: none;
    color: inherit;
    transition: border-color 0.15s;
  }

  .repo-card:hover {
    border-color: var(--primary, #2da44e);
  }

  .rc-icon {
    font-size: 32px;
    flex-shrink: 0;
  }

  .rc-body {
    flex: 1;
    min-width: 0;
  }

  .rc-name {
    font-weight: 600;
    font-size: 15px;
    margin-bottom: 4px;
  }

  .rc-desc {
    font-size: 13px;
    color: var(--text-secondary, #656d76);
    margin-bottom: 6px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rc-meta {
    font-size: 12px;
    color: var(--text-tertiary, #888);
  }

  .empty {
    text-align: center;
    padding: 60px 24px;
    color: var(--text-secondary, #656d76);
  }
</style>
