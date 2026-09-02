import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

// i18n mock: t() returns the fallback for string fallbacks and the key
// otherwise (including when called with interpolation params).
vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import PullList from './PullList.svelte';
import type { PullRequest } from '$lib/types/entities';

const pr: PullRequest = {
  id: 1,
  number: 12,
  title: 'Add feature X',
  body: null,
  state: 'open',
  is_draft: false,
  author_id: 5,
  author: 'alice',
  head_branch: 'feat/x',
  base_branch: 'main',
  created_at: '2026-09-01T00:00:00Z',
};

describe('PullList.svelte', () => {
  it('shows the loading placeholder while loading', () => {
    render(PullList, {
      owner: 'alice',
      repo: 'demo',
      pullRequests: [],
      loading: true,
      filter: 'all',
    });
    expect(screen.getByText('common.loading')).toBeInTheDocument();
  });

  it('renders title, branch pair and author for each PR', () => {
    render(PullList, {
      owner: 'alice',
      repo: 'demo',
      pullRequests: [pr],
      loading: false,
      filter: 'all',
    });

    expect(screen.getByText(/Add feature X/)).toBeInTheDocument();
    expect(screen.getByText('feat/x')).toBeInTheDocument();
    expect(screen.getByText('main')).toBeInTheDocument();
    expect(screen.getByText(/alice/)).toBeInTheDocument();
  });

  it('links each item to the PR detail route', () => {
    render(PullList, {
      owner: 'alice',
      repo: 'demo',
      pullRequests: [pr],
      loading: false,
      filter: 'all',
    });

    const link = screen.getByRole('link', { name: /Add feature X/ });
    expect(link).toHaveAttribute('href', '/alice/demo/pulls/12');
  });

  it('marks draft PRs with the draft badge', () => {
    render(PullList, {
      owner: 'alice',
      repo: 'demo',
      pullRequests: [{ ...pr, is_draft: true }],
      loading: false,
      filter: 'all',
    });
    expect(screen.getByText('pulls.draft')).toBeInTheDocument();
  });

  it('shows the empty state when the filtered list is empty', () => {
    render(PullList, {
      owner: 'alice',
      repo: 'demo',
      pullRequests: [],
      loading: false,
      filter: 'open',
    });
    // t('pulls.empty', { state }) → mocked key
    expect(screen.getByText('pulls.empty')).toBeInTheDocument();
  });
});
