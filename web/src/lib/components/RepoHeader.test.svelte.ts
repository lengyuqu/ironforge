import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const reposStar = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  repos: {
    get: vi.fn().mockResolvedValue({ default_branch: 'main' }),
    starred: vi.fn().mockResolvedValue({ starred: false }),
    watchStatus: vi.fn().mockResolvedValue({ watch_state: 'not_watching' }),
    star: (...a: unknown[]) => reposStar(...a),
    fork: vi.fn(),
    watch: vi.fn(),
    unwatch: vi.fn(),
  },
}));

vi.mock('$app/navigation', () => ({ goto: (...a: unknown[]) => goto(...a) }));
const goto = vi.fn();

// Stub the clone modal — its own contract is covered by CloneModal.test.
vi.mock('./CloneModal.svelte', async () => {
  const stub = await import('../../test-stubs/CloneModalStub.svelte');
  return { default: stub.default };
});

const toastWarning = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: vi.fn(),
    warning: (...a: unknown[]) => toastWarning(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

// Real auth store, always logged out in these tests.
import RepoHeader from './RepoHeader.svelte';

describe('RepoHeader.svelte', () => {
  beforeEach(() => {
    reposStar.mockReset();
    goto.mockClear();
    toastWarning.mockClear();
    toastError.mockClear();
  });

  it('renders owner/repo links and all tabs with correct hrefs', () => {
    render(RepoHeader, { owner: 'alice', repo: 'demo', activeTab: 'issues' });

    expect(screen.getByText('alice')).toHaveAttribute('href', '/alice');
    expect(screen.getByText('demo')).toHaveAttribute('href', '/alice/demo');

    expect(screen.getByText('repo.tabs.code').closest('a')).toHaveAttribute('href', '/alice/demo');
    expect(screen.getByText('repo.tabs.issues').closest('a')).toHaveAttribute(
      'href',
      '/alice/demo/issues'
    );
    // Board tab maps to the /boards path.
    expect(screen.getByText('repo.tabs.board').closest('a')).toHaveAttribute(
      'href',
      '/alice/demo/boards'
    );

    // Only the active tab carries the active class.
    const active = document.querySelector('.tab.active');
    expect(active?.textContent).toContain('repo.tabs.issues');
  });

  it('shows the local star count', () => {
    render(RepoHeader, { owner: 'alice', repo: 'demo', starsCount: 42 });

    expect(document.querySelector('.count')?.textContent).toBe('42');
  });

  it('ignores star clicks while logged out (no API call, no toggle)', async () => {
    render(RepoHeader, { owner: 'alice', repo: 'demo' });

    const starBtn = screen.getByLabelText('repo.login_to_star');
    expect(document.querySelector('.star-icon')?.textContent).toBe('☆');

    await fireEvent.click(starBtn);
    expect(reposStar).not.toHaveBeenCalled();
    expect(document.querySelector('.star-icon')?.textContent).toBe('☆');
  });

  it('toggles the clone modal open state from the Code button', async () => {
    render(RepoHeader, { owner: 'alice', repo: 'demo' });

    const codeBtn = screen.getByLabelText('repo.clone_title');
    expect(codeBtn.getAttribute('aria-expanded')).toBe('false');

    await fireEvent.click(codeBtn);
    expect(codeBtn.getAttribute('aria-expanded')).toBe('true');
    const stubEl = await screen.findByTestId('clone-modal-stub');
    expect(stubEl.getAttribute('data-owner')).toBe('alice');
    expect(stubEl.getAttribute('data-repo')).toBe('demo');

    await fireEvent.click(codeBtn);
    expect(codeBtn.getAttribute('aria-expanded')).toBe('false');
    await waitFor(() => expect(screen.queryByTestId('clone-modal-stub')).toBeNull());
  });
});
