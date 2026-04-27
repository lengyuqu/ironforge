<script lang="ts">
  import { createT } from '$lib/i18n';

  const t = createT();

  interface Props {
    owner: string;
    repo: string;
    activeTab?: string;
  }

  let { owner, repo, activeTab = 'code' }: Props = $props();

  const tabs = $derived([
    { id: 'code', label: $t('repo.tabs.code'), icon: '📁' },
    { id: 'issues', label: $t('repo.tabs.issues'), icon: '◉' },
    { id: 'pulls', label: $t('repo.tabs.pulls'), icon: '⑂' },
    { id: 'wiki', label: $t('repo.tabs.wiki'), icon: '📖' },
    { id: 'pipelines', label: $t('repo.tabs.pipelines'), icon: '▶' },
  ]);
</script>

<div class="repo-header">
  <div class="repo-name">
    <a href="/{owner}">{owner}</a>
    <span class="separator">/</span>
    <a href="/{owner}/{repo}">{repo}</a>
  </div>

  <nav class="repo-tabs">
    {#each tabs as tab}
      <a
        href="/{owner}/{repo}/{tab.id === 'code' ? '' : tab.id}"
        class="tab"
        class:active={activeTab === tab.id}
      >
        <span class="tab-icon">{tab.icon}</span>
        {tab.label}
      </a>
    {/each}
  </nav>
</div>

<style>
  .repo-header {
    border-bottom: 1px solid var(--border);
    padding-bottom: 0;
    margin-bottom: 24px;
  }

  .repo-name {
    font-size: 20px;
    margin-bottom: 12px;
  }
  .repo-name a {
    color: var(--accent);
    font-weight: 600;
  }
  .separator {
    margin: 0 4px;
    color: var(--text-muted);
  }

  .repo-tabs {
    display: flex;
    gap: 0;
    overflow-x: auto;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 16px;
    color: var(--text-secondary);
    font-size: 14px;
    border-bottom: 2px solid transparent;
    text-decoration: none;
    white-space: nowrap;
  }
  .tab:hover {
    color: var(--text-primary);
    text-decoration: none;
  }
  .tab.active {
    color: var(--text-primary);
    font-weight: 600;
    border-bottom-color: var(--orange);
  }
  .tab-icon {
    font-size: 14px;
  }
</style>
