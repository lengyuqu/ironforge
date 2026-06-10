<script lang="ts">
  import { createT } from '$lib/i18n';
  import { getUser, isLoggedIn } from '$lib/stores/auth';
  import { repos } from '$lib/api/client';
  import { goto } from '$app/navigation';

  const t = createT();

  interface Props {
    owner: string;
    repo: string;
    activeTab?: string;
    starsCount?: number;
  }

  let { owner, repo, activeTab = 'code', starsCount = 0 }: Props = $props();

  // Action button states
  let starred = $state(false);
  let watchState = $state<'not_watching' | 'watching' | 'ignoring'>('not_watching');
  let forking = $state(false);
  let starsLocalCount = $state(starsCount);

  // Check auth and load initial states
  $effect(() => {
    if (isLoggedIn()) {
      loadStates();
    }
  });

  async function loadStates() {
    try {
      // Load star state from API
      const starRes = await repos.star(owner, repo);
      starred = starRes.starred;
    } catch {
      starred = false;
    }
    try {
      // Load watch state - need to check via a different approach
      // For now default to not_watching
      watchState = 'not_watching';
    } catch {
      watchState = 'not_watching';
    }
  }

  async function toggleStar() {
    if (!isLoggedIn()) return;

    const prevStarred = starred;
    const prevCount = starsLocalCount;

    // Optimistic update
    starred = !starred;
    starsLocalCount = starred ? starsLocalCount + 1 : starsLocalCount - 1;

    try {
      const res = await repos.star(owner, repo);
      starred = res.starred;
      starsLocalCount = starred ? prevCount + 1 : prevCount - 1;
    } catch {
      // Revert on error
      starred = prevStarred;
      starsLocalCount = prevCount;
    }
  }

  async function cycleWatch() {
    if (!isLoggedIn()) return;

    const states: Array<'not_watching' | 'watching' | 'ignoring'> = ['not_watching', 'watching', 'ignoring'];
    const currentIndex = states.indexOf(watchState);
    const nextState = states[(currentIndex + 1) % states.length];
    const prevState = watchState;

    // Optimistic update
    watchState = nextState;

    try {
      if (nextState === 'not_watching') {
        await repos.unwatch(owner, repo);
      } else {
        await repos.watch(owner, repo, nextState);
      }
    } catch {
      // Revert on error
      watchState = prevState;
    }
  }

  async function handleFork() {
    if (!isLoggedIn()) return;

    const prevForking = forking;
    forking = true;

    try {
      const result = await repos.fork(owner, repo);
      // Fork returns 202, navigate to the new repo
      // The result should contain the new repo info
      const user = getUser();
      if (user?.username) {
        goto(`/${user.username}/${repo}`);
      }
    } catch {
      // Revert on error
      forking = false;
    }
  }

  function getWatchLabel() {
    switch (watchState) {
      case 'watching':
        return t('repo.watching');
      case 'ignoring':
        return t('repo.ignoring');
      default:
        return t('repo.watch');
    }
  }

  const tabs = $derived([
    { id: 'code', label: t('repo.tabs.code'), icon: '📁' },
    { id: 'issues', label: t('repo.tabs.issues'), icon: '◉' },
    { id: 'pulls', label: t('repo.tabs.pulls'), icon: '⑂' },
    { id: 'wiki', label: t('repo.tabs.wiki'), icon: '📖' },
    { id: 'pipelines', label: t('repo.tabs.pipelines'), icon: '▶' },
    { id: 'releases', label: t('repo.tabs.releases'), icon: '🏷' },
    { id: 'packages', label: t('repo.tabs.packages'), icon: '📦' },
    { id: 'board', label: t('repo.tabs.board'), icon: '◫' },
    { id: 'commits', label: t('repo.tabs.commits'), icon: '📜' },
    { id: 'settings', label: t('repo.tabs.settings'), icon: '⚙' },
  ]);
</script>

<div class="repo-header">
  <div class="repo-top">
    <div class="repo-name">
      <a href="/{owner}">{owner}</a>
      <span class="separator">/</span>
      <a href="/{owner}/{repo}">{repo}</a>
    </div>

    <div class="repo-actions">
      <button
        class="action-btn"
        class:starred
        class:disabled={!isLoggedIn()}
        onclick={toggleStar}
        title={isLoggedIn() ? (starred ? t('repo.unstar') : t('repo.star')) : 'Login to star'}
      >
        <span class="star-icon">{starred ? '⭐' : '☆'}</span>
        <span class="count">{starsLocalCount}</span>
      </button>

      <button
        class="action-btn"
        class:watching={watchState !== 'not_watching'}
        class:ignoring={watchState === 'ignoring'}
        class:disabled={!isLoggedIn()}
        onclick={cycleWatch}
        title={isLoggedIn() ? getWatchLabel() : 'Login to watch'}
      >
        <span class="watch-icon">👁</span>
        <span class="label">{getWatchLabel()}</span>
      </button>

      <button
        class="action-btn fork-btn"
        class:loading={forking}
        class:disabled={!isLoggedIn()}
        onclick={handleFork}
        disabled={forking || !isLoggedIn()}
        title={isLoggedIn() ? (forking ? t('repo.forking') : t('repo.fork')) : 'Login to fork'}
      >
        <span class="fork-icon">⚡</span>
        <span class="label">{forking ? t('repo.forking') : t('repo.fork')}</span>
      </button>
    </div>
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

  .repo-top {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    flex-wrap: wrap;
    gap: 12px;
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

  .repo-actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .action-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .action-btn:hover:not(.disabled) {
    background: var(--bg-hover);
    border-color: var(--text-muted);
  }

  .action-btn.disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .action-btn.starred {
    background: linear-gradient(135deg, #fff8e1 0%, #ffecb3 100%);
    border-color: #ffc107;
  }

  .action-btn.starred .star-icon {
    color: #ffc107;
  }

  .action-btn.watching {
    background: linear-gradient(135deg, #e3f2fd 0%, #bbdefb 100%);
    border-color: var(--accent);
  }

  .action-btn.ignoring {
    background: linear-gradient(135deg, #fafafa 0%, #eeeeee 100%);
    border-color: var(--text-muted);
    color: var(--text-muted);
  }

  .fork-btn.loading {
    opacity: 0.8;
    cursor: wait;
  }

  .star-icon,
  .watch-icon,
  .fork-icon {
    font-size: 16px;
  }

  .count {
    min-width: 20px;
    text-align: left;
  }

  .label {
    white-space: nowrap;
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

  @media (max-width: 640px) {
    .repo-top {
      flex-direction: column;
    }

    .repo-actions {
      width: 100%;
      justify-content: flex-start;
    }

    .action-btn {
      padding: 5px 10px;
      font-size: 13px;
    }
  }
</style>
