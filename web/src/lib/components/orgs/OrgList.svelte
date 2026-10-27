<script lang="ts">
  import EmptyState from '$lib/components/EmptyState.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import type { Organization } from '$lib/types/entities';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let {
    organizations,
    loading,
    onCreate,
  }: {
    organizations: Organization[];
    loading: boolean;
    onCreate: () => void;
  } = $props();
</script>

<section class="org-section">
  <div class="section-header">
    <h2>{t('orgs.list_title', { count: String(organizations.length) })}</h2>
  </div>

  {#if loading}
    <LoadingState />
  {:else if organizations.length === 0}
    <EmptyState title={t('orgs.no_orgs')} description={t('orgs.no_orgs_desc')}>
      <button type="button" class="btn-primary" onclick={onCreate}>
        {t('orgs.create_action')}
      </button>
    </EmptyState>
  {:else}
    <div class="org-list">
      {#each organizations as org (org.id)}
        <a href={`/orgs/${org.name}`} class="org-card">
          <div class="org-avatar" aria-hidden="true">{org.name[0]?.toUpperCase() || '?'}</div>
          <div class="org-body">
            <div class="org-card-header">
              <h3>{org.display_name || org.name}</h3>
              <span class="visibility">{t(`orgs.visibility_${org.visibility}`)}</span>
            </div>
            <p class="org-name">@{org.name}</p>
            <p class="org-desc">{org.description || t('common.no_description')}</p>
            <p class="org-meta">{t('common.created', { date: formatDate(org.created_at) })}</p>
          </div>
        </a>
      {/each}
    </div>
  {/if}
</section>

<style>
  .org-section {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.25rem;
  }

  h2 {
    color: var(--text-primary);
    margin: 0;
  }

  .section-header {
    margin-bottom: 1rem;
  }
  .org-desc,
  .org-meta,
  .org-name {
    color: var(--text-secondary);
  }

  .org-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 0.75rem;
  }

  .org-card {
    display: flex;
    gap: 0.85rem;
    min-width: 0;
    padding: 1rem;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--text-primary);
    text-decoration: none;
  }

  .org-card:hover {
    border-color: var(--accent);
    text-decoration: none;
  }

  .org-avatar {
    flex: 0 0 42px;
    width: 42px;
    height: 42px;
    border-radius: 50%;
    background: var(--accent);
    color: white;
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 700;
  }

  .org-body {
    min-width: 0;
    flex: 1;
  }

  .org-card-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 0.75rem;
  }

  .org-card h3 {
    margin: 0;
    font-size: 1rem;
    color: var(--text-primary);
    overflow-wrap: anywhere;
  }

  .visibility {
    flex: 0 0 auto;
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0.1rem 0.5rem;
    color: var(--text-secondary);
    font-size: 0.75rem;
  }

  .org-name,
  .org-desc,
  .org-meta {
    margin: 0.3rem 0 0;
    font-size: 0.85rem;
    overflow-wrap: anywhere;
  }
</style>
