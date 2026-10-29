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
    { id: 'code', label: t('repo.tabs.code'), icon: '📁' },
    { id: 'issues', label: t('repo.tabs.issues'), icon: '◉' },
    { id: 'pulls', label: t('repo.tabs.pulls'), icon: '⑂' },
    { id: 'wiki', label: t('repo.tabs.wiki'), icon: '📖' },
    { id: 'pipelines', label: t('repo.tabs.pipelines'), icon: '▶' },
    { id: 'releases', label: t('repo.tabs.releases'), icon: '🏷' },
    { id: 'packages', label: t('repo.tabs.packages'), icon: '📦' },
    { id: 'board', label: t('repo.tabs.board'), icon: '◫', path: 'boards' },
    { id: 'time_tracking', label: t('repo.tabs.time_tracking'), icon: '⏱' },
    { id: 'commits', label: t('repo.tabs.commits'), icon: '📜' },
    { id: 'settings', label: t('repo.tabs.settings'), icon: '⚙' },
  ]);

  function tabHref(tab: { id: string; path?: string }) {
    const suffix = tab.id === 'code' ? '' : `/${tab.path || tab.id}`;
    return `/${owner}/${repo}${suffix}`;
  }
</script>

<nav class="repo-tabs">
  {#each tabs as tab}
    <a
      href={tabHref(tab)}
      class="tab"
      class:active={activeTab === tab.id}
    >
      <span class="tab-icon">{tab.icon}</span>
      {tab.label}
    </a>
  {/each}
</nav>

<style>
  .repo-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 0;
    overflow: visible;
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
    border-bottom-color: var(--accent);
  }
  .tab-icon {
    font-size: 14px;
  }
</style>
