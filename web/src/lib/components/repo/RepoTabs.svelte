<script lang="ts">
  import { createT } from '$lib/i18n';
  import Icon, { type IconName } from '$lib/components/Icon.svelte';

  const t = createT();

  interface Props {
    owner: string;
    repo: string;
    activeTab?: string;
  }

  let { owner, repo, activeTab = 'code' }: Props = $props();

  const tabs: Array<{ id: string; label: string; icon: IconName; path?: string }> = $derived([
    { id: 'code', label: t('repo.tabs.code'), icon: 'code' },
    { id: 'issues', label: t('repo.tabs.issues'), icon: 'issue' },
    { id: 'pulls', label: t('repo.tabs.pulls'), icon: 'pull-request' },
    { id: 'wiki', label: t('repo.tabs.wiki'), icon: 'book' },
    { id: 'pipelines', label: t('repo.tabs.pipelines'), icon: 'play' },
    { id: 'releases', label: t('repo.tabs.releases'), icon: 'tag' },
    { id: 'packages', label: t('repo.tabs.packages'), icon: 'package' },
    { id: 'board', label: t('repo.tabs.board'), icon: 'kanban', path: 'boards' },
    { id: 'time_tracking', label: t('repo.tabs.time_tracking'), icon: 'clock' },
    { id: 'collaboration', label: t('repo.tabs.collaboration'), icon: 'agents' },
    { id: 'commits', label: t('repo.tabs.commits'), icon: 'git-commit' },
    { id: 'settings', label: t('repo.tabs.settings'), icon: 'settings' },
  ]);

  function tabHref(tab: { id: string; path?: string }) {
    const suffix = tab.id === 'code' ? '' : `/${tab.path || tab.id}`;
    return `/${owner}/${repo}${suffix}`;
  }
</script>

<nav class="repo-tabs" aria-label={t('repo.tabs.nav_label', 'Repository navigation')}>
  {#each tabs as tab (tab.id)}
    <a
      href={tabHref(tab)}
      class="tab"
      class:active={activeTab === tab.id}
      aria-current={activeTab === tab.id ? 'page' : undefined}
    >
      <span class="tab-icon"><Icon name={tab.icon} size={15} /></span>
      {tab.label}
    </a>
  {/each}
</nav>

<style>
  .repo-tabs {
    display: flex;
    flex-wrap: nowrap;
    gap: 2px;
    overflow-x: auto;
    scrollbar-width: none;
    -webkit-overflow-scrolling: touch;
  }

  .repo-tabs::-webkit-scrollbar {
    display: none;
  }

  .tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 8px 12px;
    color: var(--text-secondary);
    font-size: 13.5px;
    border-bottom: 2px solid transparent;
    text-decoration: none;
    white-space: nowrap;
    flex-shrink: 0;
    transition: color 0.15s, border-color 0.15s;
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
    display: inline-flex;
    align-items: center;
  }
</style>
