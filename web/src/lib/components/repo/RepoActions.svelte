<script lang="ts">
  import { createT } from '$lib/i18n';
  import { getUser, isLoggedIn } from '$lib/stores/auth.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { goto } from '$app/navigation';
  import CloneModal from '$lib/components/CloneModal.svelte';
  import { toast } from '$lib/components/toast.svelte';

  const t = createT();

  interface Props {
    owner: string;
    repo: string;
    starsCount?: number;
    defaultBranch?: string;
  }

  let { owner, repo, starsCount = 0, defaultBranch }: Props = $props();

  type WatchState = 'not_watching' | 'watching' | 'ignoring';

  // Action button states
  let starred = $state(false);
  let watchState = $state<WatchState>('not_watching');
  let forking = $state(false);
  let starsLocalCount = $state(0);
  let archiveRef = $state('main');
  let showClone = $state(false);

  // Bookkeeping for the async state load. A load remembers the repository it was
  // started for and, per field, the action version at that moment; it may only
  // write when both still match. Otherwise a slow response would either undo a
  // click the viewer made in the meantime or leak the previous repository's
  // state into the one now on screen. Plain (non-reactive) values: they exist to
  // be compared when a response lands, not to drive renders.
  // Empty until the first load runs, so the initial value never has to capture
  // props (and the "switched repositories" branch is skipped on first run).
  let stateScope = '';
  let starVersion = 0;
  let watchVersion = 0;

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
    const scope = `${owner}/${repo}`;
    if (scope !== stateScope) {
      if (stateScope !== '') {
        // Switching repositories: drop what the old one showed and invalidate
        // any load still in flight for it.
        starVersion += 1;
        watchVersion += 1;
        starred = false;
        watchState = 'not_watching';
      }
      stateScope = scope;
    }
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
    const scope = `${owner}/${repo}`;
    const starTicket = starVersion;
    const watchTicket = watchVersion;
    // Each field is guarded on its own so a star click does not also discard a
    // watch answer that is still useful.
    const starIsCurrent = () => starTicket === starVersion && scope === `${owner}/${repo}`;
    const watchIsCurrent = () => watchTicket === watchVersion && scope === `${owner}/${repo}`;

    let nextStarred = false;
    try {
      nextStarred = (await repos.starred(owner, repo)).starred;
    } catch {
      nextStarred = false;
    }
    if (starIsCurrent()) starred = nextStarred;

    let nextWatchState: WatchState = 'not_watching';
    try {
      nextWatchState = (await repos.watchStatus(owner, repo)).watch_state;
    } catch {
      nextWatchState = 'not_watching';
    }
    if (watchIsCurrent()) watchState = nextWatchState;
  }

  async function toggleStar() {
    if (!isLoggedIn()) return;

    // A load that started before this click must not write its stale answer.
    starVersion += 1;

    // The answer belongs to the repository that was on screen when it was asked
    // for; if the header moved on, drop it instead of writing it to the new one.
    const scope = `${owner}/${repo}`;
    const isCurrent = () => scope === `${owner}/${repo}`;

    const prevStarred = starred;
    const prevCount = starsLocalCount;

    // Optimistic update
    starred = !starred;
    starsLocalCount = starred ? starsLocalCount + 1 : starsLocalCount - 1;

    try {
      const res = await repos.star(owner, repo);
      if (!isCurrent()) return;
      starred = res.starred;
      starsLocalCount = starred ? prevCount + 1 : prevCount - 1;
    } catch (e: unknown) {
      if (!isCurrent()) return;
      // Revert on error and show feedback
      starred = prevStarred;
      starsLocalCount = prevCount;
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(t('repo.star_failed', 'Failed to update star') + ': ' + msg);
    }
  }

  async function cycleWatch() {
    if (!isLoggedIn()) return;

    // Same guards as toggleStar, for the watch field only.
    watchVersion += 1;

    const scope = `${owner}/${repo}`;
    const isCurrent = () => scope === `${owner}/${repo}`;

    const states: WatchState[] = ['not_watching', 'watching', 'ignoring'];
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
      if (!isCurrent()) return;
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
      const forkOwner = getUser()?.username;
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
</script>

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

<style>
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
    background: var(--accent-subtle);
    border-color: var(--accent);
    color: var(--accent);
  }

  .action-btn.watching {
    background: var(--accent-subtle);
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
    color: var(--text-on-accent);
    border: 1px solid rgba(27,31,36,0.15);
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    line-height: 20px;
  }
  .btn-code:hover {
    background: var(--green-dim);
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
