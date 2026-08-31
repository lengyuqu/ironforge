<script lang="ts">
  import { createT } from '$lib/i18n';
  import { getUser, isLoggedIn } from '$lib/stores/auth.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { goto } from '$app/navigation';
  import CloneModal from './CloneModal.svelte';
  import { toast } from './toast.svelte';

  const t = createT();

  interface Props {
    owner: string;
    repo: string;
    activeTab?: string;
    starsCount?: number;
    defaultBranch?: string;
  }

  let { owner, repo, activeTab = 'code', starsCount = 0, defaultBranch }: Props = $props();

  // Action button states
  let starred = $state(false);
  let watchState = $state<'not_watching' | 'watching' | 'ignoring'>('not_watching');
  let forking = $state(false);
  let starsLocalCount = $state(0);
  let archiveRef = $state('main');
  let showClone = $state(false);

  let currentUsername = $derived(getUser()?.username || '');
  let isOwnRepo = $derived(Boolean(currentUsername) && currentUsername === owner);

  // Sync when prop changes
  $effect(() => {
    starsLocalCount = starsCount;
    if (!isLoggedIn()) {
      starred = false;
    }
  });

  function handleCloneClick() {
    showClone = !showClone;
  }

  // Check auth and load initial states
  $effect(() => {
    if (isLoggedIn()) {
      loadStates();
    }
  });

  $effect(() => {
    archiveRef = defaultBranch || 'main';
    if (!defaultBranch) {
      loadArchiveRef();
    }
  });

  async function loadArchiveRef() {
    const expectedOwner = owner;
    const expectedRepo = repo;
    try {
      const repoInfo = await repos.get(expectedOwner, expectedRepo);
      if (owner === expectedOwner && repo === expectedRepo && repoInfo.default_branch) {
        archiveRef = repoInfo.default_branch;
      }
    } catch {
      archiveRef = defaultBranch || 'main';
    }
  }

  async function loadStates() {
    try {
      const stateRes = await repos.starred(owner, repo);
      starred = stateRes.starred;
    } catch {
      starred = false;
    }
    try {
      const watchRes = await repos.watchStatus(owner, repo);
      watchState = watchRes.watch_state;
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
    } catch (e: unknown) {
      // Revert on error and show feedback
      starred = prevStarred;
      starsLocalCount = prevCount;
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(t('repo.star_failed', 'Failed to update star') + ': ' + msg);
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
    } catch (e: unknown) {
      // Revert on error and show feedback
      watchState = prevState;
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(t('repo.watch_failed', 'Failed to update watch status') + ': ' + msg);
    }
  }

  async function handleFork() {
    if (!isLoggedIn()) {
      toast.warning(t('repo.fork.login_required'));
      return;
    }

    if (isOwnRepo) {
      toast.warning(t('repo.fork.own_repo'));
      return;
    }

    forking = true;

    try {
      const result = await repos.fork(owner, repo);
      const forkOwner = result?.owner?.username || result?.owner_name || result?.owner || getUser()?.username;
      const forkName = result?.name || repo;
      toast.success(t('repo.fork.success'));
      if (forkOwner) {
        goto(`/${forkOwner}/${forkName}`);
      }
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(t('repo.fork.failed') + ': ' + msg);
      forking = false;
    }
  }

  function getForkTitle() {
    if (!isLoggedIn()) return t('repo.fork.login_title');
    if (isOwnRepo) return t('repo.fork.own_repo');
    return forking ? t('repo.forking') : t('repo.fork.label');
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

<div class="repo-header">
  <div class="repo-top">
    <div class="repo-name">
      <a href={`/${owner}`}>{owner}</a>
      <span class="separator">/</span>
      <a href={`/${owner}/${repo}`}>{repo}</a>
    </div>

    <div class="repo-actions">
      <button
        class="action-btn btn btn-outline btn-sm"
        class:starred={starred}
        class:btn-primary={starred}
        class:disabled={!isLoggedIn()}
        onclick={toggleStar}
        title={isLoggedIn() ? (starred ? t('repo.unstar') : t('repo.star')) : t('repo.login_to_star')}
        aria-label={isLoggedIn() ? (starred ? t('repo.unstar') : t('repo.star')) : t('repo.login_to_star')}
      >
        <span class="star-icon" aria-hidden="true">{starred ? '⭐' : '☆'}</span>
        <span class="count">{starsLocalCount}</span>
      </button>

      <button
        class="action-btn btn btn-outline btn-sm"
        class:watching={watchState !== 'not_watching'}
        class:ignoring={watchState === 'ignoring'}
        class:disabled={!isLoggedIn()}
        onclick={cycleWatch}
        title={isLoggedIn() ? getWatchLabel() : t('repo.login_to_watch')}
        aria-label={isLoggedIn() ? getWatchLabel() : t('repo.login_to_watch')}
      >
        <span class="watch-icon" aria-hidden="true">👁</span>
        <span class="label">{getWatchLabel()}</span>
      </button>

      <button
        class="action-btn btn btn-outline btn-sm fork-btn"
        class:loading={forking}
        class:disabled={!isLoggedIn() || isOwnRepo}
        onclick={handleFork}
        disabled={forking || !isLoggedIn()}
        title={getForkTitle()}
        aria-label={getForkTitle()}
      >
        <span class="fork-icon">⚡</span>
        <span class="label">{forking ? t('repo.forking') : t('repo.fork.label')}</span>
      </button>

      <div class="clone-area" style="position:relative">
        <button
          class="btn-code btn btn-primary"
          onclick={handleCloneClick}
          aria-haspopup="true"
          aria-expanded={showClone}
          aria-controls="clone-dropdown"
          aria-label={t('repo.clone_title')}
        >
          <span class="code-icon" aria-hidden="true">⌄</span>
          <span class="label">{t('repo.code')}</span>
          <span class="caret" aria-hidden="true">▾</span>
        </button>
        <CloneModal owner={owner} repo={repo} archiveRef={archiveRef} open={showClone} onClose={() => (showClone = false)} />
      </div>
    </div>
  </div>

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
    display: inline-flex;
    align-items: center;
    gap: 6px;
    line-height: 1.2;
    white-space: nowrap;
  }

  .action-btn.disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .action-btn.starred {
    background: var(--accent-weak);
    border-color: var(--accent);
    color: var(--accent);
  }

  .action-btn.watching {
    background: var(--accent-weak);
    border-color: var(--accent);
    color: var(--accent);
  }

  .action-btn.ignoring {
    background: var(--bg-secondary);
    border-color: var(--border);
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

  /* Clone button — GitHub style */
  .clone-area {
    position: relative;
  }

  .btn-code {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 14px;
    background: var(--green-dim);
    color: #fff;
    border: 1px solid rgba(27,31,36,0.15);
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    line-height: 20px;
  }
  .btn-code:hover {
    background: #1a7f37;
  }

  .code-icon {
    font-size: 14px;
    line-height: 1;
  }

  .caret {
    font-size: 10px;
    margin-left: 2px;
    opacity: 0.7;
  }

  @media (max-width: 600px) {
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
