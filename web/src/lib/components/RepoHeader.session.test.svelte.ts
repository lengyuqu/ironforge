// RepoHeader behaviour that needs a signed-in user: initial star/watch state,
// optimistic star + watch updates (and their rollback), forking, and the
// default-branch lookup that feeds the clone modal. The logged-out and layout
// behaviour lives in RepoHeader.test.svelte.ts.
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, act } from '@testing-library/svelte';

const api = vi.hoisted(() => ({
  get: vi.fn(),
  starred: vi.fn(),
  watchStatus: vi.fn(),
  star: vi.fn(),
  fork: vi.fn(),
  watch: vi.fn(),
  unwatch: vi.fn(),
  me: vi.fn(),
  login: vi.fn(),
  logout: vi.fn(),
}));

vi.mock('$lib/api/client.svelte', () => ({
  repos: {
    get: (...a: unknown[]) => api.get(...a),
    starred: (...a: unknown[]) => api.starred(...a),
    watchStatus: (...a: unknown[]) => api.watchStatus(...a),
    star: (...a: unknown[]) => api.star(...a),
    fork: (...a: unknown[]) => api.fork(...a),
    watch: (...a: unknown[]) => api.watch(...a),
    unwatch: (...a: unknown[]) => api.unwatch(...a),
  },
  auth: {
    login: (...a: unknown[]) => api.login(...a),
    me: (...a: unknown[]) => api.me(...a),
    logout: (...a: unknown[]) => api.logout(...a),
  },
}));

const goto = vi.hoisted(() => vi.fn());
vi.mock('$app/navigation', () => ({ goto: (...a: unknown[]) => goto(...a) }));

vi.mock('./CloneModal.svelte', async () => {
  const stub = await import('../../test-stubs/CloneModalStub.svelte');
  return { default: stub.default };
});

const toast = vi.hoisted(() => ({ success: vi.fn(), warning: vi.fn(), error: vi.fn() }));
vi.mock('$lib/components/toast.svelte', () => ({ toast }));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import { login, logout } from '$lib/stores/auth.svelte';
import RepoHeader from './RepoHeader.svelte';

async function signInAs(username: string) {
  api.login.mockResolvedValue({ mfa_required: false });
  api.me.mockResolvedValue({ id: 1, username, email: `${username}@x.io`, is_admin: false });
  await login(username, 'pw');
}

const starIcon = () => document.querySelector('.star-icon')?.textContent;
const starCount = () => document.querySelector('.count')?.textContent;

describe('RepoHeader.svelte (signed in)', () => {
  beforeEach(async () => {
    for (const fn of Object.values(api)) fn.mockReset();
    goto.mockReset();
    Object.values(toast).forEach((fn) => fn.mockReset());
    api.logout.mockResolvedValue(undefined);
    await logout();
    api.get.mockResolvedValue({ default_branch: 'trunk' });
    api.starred.mockResolvedValue({ starred: false });
    api.watchStatus.mockResolvedValue({ watch_state: 'not_watching' });
    await signInAs('bob');
  });

  describe('initial state', () => {
    it('loads the starred flag and watch state for the viewer', async () => {
      api.starred.mockResolvedValue({ starred: true });
      api.watchStatus.mockResolvedValue({ watch_state: 'watching' });
      render(RepoHeader, { owner: 'alice', repo: 'demo', starsCount: 3 });

      await waitFor(() => expect(starIcon()).toBe('⭐'));
      expect(api.starred).toHaveBeenCalledWith('alice', 'demo');
      expect(api.watchStatus).toHaveBeenCalledWith('alice', 'demo');
      expect(screen.getByLabelText('repo.unstar')).toBeInTheDocument();
      expect(screen.getByLabelText('repo.watching')).toBeInTheDocument();
    });

    it('falls back to not starred / not watching when the state calls fail', async () => {
      api.starred.mockRejectedValue(new Error('x'));
      api.watchStatus.mockRejectedValue(new Error('x'));
      render(RepoHeader, { owner: 'alice', repo: 'demo' });

      await waitFor(() => expect(api.watchStatus).toHaveBeenCalled());
      expect(starIcon()).toBe('☆');
      expect(screen.getByLabelText('repo.watch')).toBeInTheDocument();
    });
  });

  describe('starring', () => {
    it('updates optimistically, then settles on the server answer', async () => {
      let resolve!: (v: { starred: boolean }) => void;
      api.star.mockReturnValue(new Promise((r) => (resolve = r)));
      render(RepoHeader, { owner: 'alice', repo: 'demo', starsCount: 5 });
      // Let the initial state load finish first, or it would overwrite the click.
      await waitFor(() => expect(api.watchStatus).toHaveBeenCalled());

      await fireEvent.click(screen.getByLabelText('repo.star'));
      // The request is still pending here, so this is the optimistic state.
      await waitFor(() => expect(starIcon()).toBe('⭐'));
      expect(starCount()).toBe('6');

      resolve({ starred: true });
      await waitFor(() => expect(api.star).toHaveBeenCalledWith('alice', 'demo'));
      expect(starCount()).toBe('6');
    });

    it('unstarring decrements the count', async () => {
      api.starred.mockResolvedValue({ starred: true });
      api.star.mockResolvedValue({ starred: false });
      render(RepoHeader, { owner: 'alice', repo: 'demo', starsCount: 5 });
      await waitFor(() => expect(starIcon()).toBe('⭐'));

      await fireEvent.click(screen.getByLabelText('repo.unstar'));
      await waitFor(() => expect(starIcon()).toBe('☆'));
      expect(starCount()).toBe('4');
    });

    it('rolls back and reports the error when the request fails', async () => {
      api.star.mockRejectedValue(new Error('rate limited'));
      render(RepoHeader, { owner: 'alice', repo: 'demo', starsCount: 5 });

      await fireEvent.click(screen.getByLabelText('repo.star'));

      await waitFor(() => expect(toast.error).toHaveBeenCalledTimes(1));
      expect(toast.error.mock.calls[0][0]).toContain('rate limited');
      expect(starIcon()).toBe('☆');
      expect(starCount()).toBe('5');
    });
  });

  describe('watching', () => {
    it('cycles not_watching -> watching -> ignoring -> not_watching', async () => {
      api.watch.mockResolvedValue(undefined);
      api.unwatch.mockResolvedValue(undefined);
      render(RepoHeader, { owner: 'alice', repo: 'demo' });
      await waitFor(() => expect(api.watchStatus).toHaveBeenCalled());

      await fireEvent.click(screen.getByLabelText('repo.watch'));
      await waitFor(() => expect(api.watch).toHaveBeenLastCalledWith('alice', 'demo', 'watching'));
      expect(screen.getByLabelText('repo.watching')).toBeInTheDocument();

      await fireEvent.click(screen.getByLabelText('repo.watching'));
      await waitFor(() => expect(api.watch).toHaveBeenLastCalledWith('alice', 'demo', 'ignoring'));
      expect(screen.getByLabelText('repo.ignoring')).toBeInTheDocument();

      await fireEvent.click(screen.getByLabelText('repo.ignoring'));
      await waitFor(() => expect(api.unwatch).toHaveBeenCalledWith('alice', 'demo'));
      expect(screen.getByLabelText('repo.watch')).toBeInTheDocument();
    });

    it('reverts and reports the error when changing the state fails', async () => {
      api.watch.mockRejectedValue(new Error('nope'));
      render(RepoHeader, { owner: 'alice', repo: 'demo' });
      await waitFor(() => expect(api.watchStatus).toHaveBeenCalled());

      await fireEvent.click(screen.getByLabelText('repo.watch'));

      await waitFor(() => expect(toast.error).toHaveBeenCalledTimes(1));
      expect(toast.error.mock.calls[0][0]).toContain('nope');
      expect(screen.getByLabelText('repo.watch')).toBeInTheDocument();
    });
  });

  describe('forking', () => {
    it('forks and navigates to the viewer\'s copy', async () => {
      api.fork.mockResolvedValue({ name: 'demo-fork' });
      render(RepoHeader, { owner: 'alice', repo: 'demo' });

      await fireEvent.click(screen.getByLabelText('repo.fork.label'));

      await waitFor(() => expect(goto).toHaveBeenCalledWith('/bob/demo-fork'));
      expect(api.fork).toHaveBeenCalledWith('alice', 'demo');
      expect(toast.success).toHaveBeenCalledWith('repo.fork.success');
    });

    it('keeps the repo name when the server does not return one', async () => {
      api.fork.mockResolvedValue({});
      render(RepoHeader, { owner: 'alice', repo: 'demo' });

      await fireEvent.click(screen.getByLabelText('repo.fork.label'));
      await waitFor(() => expect(goto).toHaveBeenCalledWith('/bob/demo'));
    });

    it('refuses to fork your own repository', async () => {
      render(RepoHeader, { owner: 'bob', repo: 'mine' });

      await fireEvent.click(screen.getByLabelText('repo.fork.own_repo'));

      expect(toast.warning).toHaveBeenCalledWith('repo.fork.own_repo');
      expect(api.fork).not.toHaveBeenCalled();
    });

    it('reports a failed fork and re-enables the button', async () => {
      api.fork.mockRejectedValue(new Error('quota'));
      render(RepoHeader, { owner: 'alice', repo: 'demo' });

      await fireEvent.click(screen.getByLabelText('repo.fork.label'));

      await waitFor(() => expect(toast.error).toHaveBeenCalledTimes(1));
      expect(toast.error.mock.calls[0][0]).toContain('quota');
      expect(goto).not.toHaveBeenCalled();
      expect(screen.getByLabelText('repo.fork.label')).toBeEnabled();
    });
  });

  describe('clone archive ref', () => {
    async function openClone() {
      await fireEvent.click(screen.getByLabelText('repo.clone_title'));
      return screen.findByTestId('clone-modal-stub');
    }

    it('uses the defaultBranch prop without asking the API', async () => {
      render(RepoHeader, { owner: 'alice', repo: 'demo', defaultBranch: 'develop' });
      const stub = await openClone();
      expect(stub.getAttribute('data-archive-ref')).toBe('develop');
      expect(api.get).not.toHaveBeenCalled();
    });

    it('looks the default branch up when none is given', async () => {
      render(RepoHeader, { owner: 'alice', repo: 'demo' });
      await waitFor(() => expect(api.get).toHaveBeenCalledWith('alice', 'demo'));
      await waitFor(async () => {
        const stub = await openClone();
        expect(stub.getAttribute('data-archive-ref')).toBe('trunk');
      });
    });

    it('falls back to main when the lookup fails', async () => {
      api.get.mockRejectedValue(new Error('x'));
      render(RepoHeader, { owner: 'alice', repo: 'demo' });
      await waitFor(() => expect(api.get).toHaveBeenCalled());
      const stub = await openClone();
      expect(stub.getAttribute('data-archive-ref')).toBe('main');
    });
  });

  // The header loads star/watch state asynchronously. A user who clicks before
  // that load settles used to lose the click: the late response wrote the old
  // server value back over the optimistic update. These cases pin the ordering.
  describe('racing the initial state load', () => {
    it('does not let a late star-state load overwrite a click made before it returned', async () => {
      let resolveStarred!: (v: { starred: boolean }) => void;
      api.starred.mockReturnValue(new Promise((r) => (resolveStarred = r)));
      api.star.mockResolvedValue({ starred: true });
      render(RepoHeader, { owner: 'alice', repo: 'demo', starsCount: 5 });

      // The viewer stars the repo while the initial load is still in flight.
      await fireEvent.click(screen.getByLabelText('repo.star'));
      await act(async () => {});
      expect(api.star).toHaveBeenCalledWith('alice', 'demo');
      expect(starIcon()).toBe('⭐');
      expect(starCount()).toBe('6');

      // The stale load now answers "not starred"; it must not undo the click.
      await act(async () => {
        resolveStarred({ starred: false });
      });
      expect(starIcon()).toBe('⭐');
      expect(starCount()).toBe('6');
      expect(screen.getByLabelText('repo.unstar')).toBeInTheDocument();
    });

    it('does not let a late watch-state load overwrite a click made before it returned', async () => {
      let resolveWatch!: (v: { watch_state: 'not_watching' | 'watching' | 'ignoring' }) => void;
      api.watchStatus.mockReturnValue(new Promise((r) => (resolveWatch = r)));
      api.watch.mockResolvedValue({ watch_state: 'watching' });
      render(RepoHeader, { owner: 'alice', repo: 'demo' });

      // The star half of the load settles first, so watchStatus is now in flight.
      await waitFor(() => expect(api.watchStatus).toHaveBeenCalledWith('alice', 'demo'));

      await fireEvent.click(screen.getByLabelText('repo.watch'));
      await act(async () => {});
      expect(api.watch).toHaveBeenCalledWith('alice', 'demo', 'watching');
      expect(screen.getByLabelText('repo.watching')).toBeInTheDocument();

      // The stale load answers "not watching"; it must not undo the click.
      await act(async () => {
        resolveWatch({ watch_state: 'not_watching' });
      });
      expect(screen.getByLabelText('repo.watching')).toBeInTheDocument();
    });

    it('ignores a late load from the previous repository after the header switched repos', async () => {
      const pending: Array<(v: { starred: boolean }) => void> = [];
      api.starred.mockImplementation(() => new Promise((r) => pending.push(r)));

      const view = render(RepoHeader, { owner: 'alice', repo: 'demo', starsCount: 0 });
      await waitFor(() => expect(api.starred).toHaveBeenCalledWith('alice', 'demo'));

      await view.rerender({ owner: 'bob', repo: 'other', starsCount: 0 });
      await waitFor(() => expect(api.starred).toHaveBeenCalledWith('bob', 'other'));

      // The previous repo's answer arrives last and claims "starred".
      await act(async () => {
        pending[0]({ starred: true });
      });
      expect(starIcon()).toBe('☆');

      // The new repo's own answer still lands.
      await act(async () => {
        pending[1]({ starred: true });
      });
      expect(starIcon()).toBe('⭐');
    });

    it('ignores a late watch load from the previous repository after the header switched repos', async () => {
      const pending: Array<(v: { watch_state: 'not_watching' | 'watching' | 'ignoring' }) => void> = [];
      api.watchStatus.mockImplementation(() => new Promise((r) => pending.push(r)));

      const view = render(RepoHeader, { owner: 'alice', repo: 'demo' });
      await waitFor(() => expect(api.watchStatus).toHaveBeenCalledWith('alice', 'demo'));

      await view.rerender({ owner: 'bob', repo: 'other' });
      await waitFor(() => expect(api.watchStatus).toHaveBeenCalledWith('bob', 'other'));

      // The previous repo's answer arrives last and claims "watching".
      await act(async () => {
        pending[0]({ watch_state: 'watching' });
      });
      expect(screen.getByLabelText('repo.watch')).toBeInTheDocument();

      // The new repo's own answer still lands.
      await act(async () => {
        pending[1]({ watch_state: 'ignoring' });
      });
      expect(screen.getByLabelText('repo.ignoring')).toBeInTheDocument();
    });

    it('drops a star answer that arrives after the header switched repos', async () => {
      let resolveStar!: (v: { starred: boolean }) => void;
      api.star.mockReturnValue(new Promise((r) => (resolveStar = r)));
      api.starred.mockResolvedValue({ starred: false });

      const view = render(RepoHeader, { owner: 'alice', repo: 'demo', starsCount: 5 });
      await waitFor(() => expect(api.starred).toHaveBeenCalledWith('alice', 'demo'));

      await fireEvent.click(screen.getByLabelText('repo.star'));
      await act(async () => {});
      expect(starCount()).toBe('6');

      // The viewer navigates to another repo before the star request settles.
      await view.rerender({ owner: 'bob', repo: 'other', starsCount: 0 });
      await act(async () => {});

      await act(async () => {
        resolveStar({ starred: true });
      });
      // The stale answer must not be counted into the new repo.
      expect(starIcon()).toBe('☆');
      expect(starCount()).toBe('0');
    });

    it('drops a watch failure that arrives after the header switched repos', async () => {
      let rejectWatch!: (e: Error) => void;
      api.watch.mockReturnValue(new Promise((_, reject) => (rejectWatch = reject)));
      api.starred.mockResolvedValue({ starred: false });
      api.watchStatus.mockResolvedValue({ watch_state: 'not_watching' });

      const view = render(RepoHeader, { owner: 'alice', repo: 'demo' });
      await waitFor(() => expect(api.watchStatus).toHaveBeenCalledWith('alice', 'demo'));

      await fireEvent.click(screen.getByLabelText('repo.watch'));
      await act(async () => {});
      expect(screen.getByLabelText('repo.watching')).toBeInTheDocument();

      await view.rerender({ owner: 'bob', repo: 'other' });
      await act(async () => {});

      await act(async () => {
        rejectWatch(new Error('nope'));
      });
      // No toast about a repository that is no longer on screen.
      expect(toast.error).not.toHaveBeenCalled();
      expect(screen.getByLabelText('repo.watch')).toBeInTheDocument();
    });
  });
});
