import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import RecentCommitsPanel from './RecentCommitsPanel.svelte';
import type { RepoCommitEntry } from '$lib/types/entities';

const commits: RepoCommitEntry[] = [
  {
    sha: 'abcdef1234567890',
    message: 'Fix parser bug\n\nMulti-line body ignored by the panel',
    author: 'alice',
    date: '2026-09-01T00:00:00Z',
  },
  {
    sha: '1234567890abcdef',
    message: 'Add feature',
    author: 'bob',
    date: '2026-08-31T00:00:00Z',
  },
];

describe('RecentCommitsPanel.svelte', () => {
  it('renders only the first line of each commit message', () => {
    render(RecentCommitsPanel, { owner: 'alice', repo: 'demo', ref: 'main', commits });

    expect(screen.getByText('Fix parser bug')).toBeInTheDocument();
    expect(screen.getByText('Add feature')).toBeInTheDocument();
    expect(screen.queryByText(/Multi-line body/)).not.toBeInTheDocument();
  });

  it('links each commit to its detail view, preserving the ref', () => {
    render(RecentCommitsPanel, { owner: 'alice', repo: 'demo', ref: 'dev', commits });

    expect(screen.getByText('Fix parser bug').closest('a')).toHaveAttribute(
      'href',
      '/alice/demo/commits/abcdef1234567890?ref=dev'
    );
    expect(screen.getByText('Add feature').closest('a')).toHaveAttribute(
      'href',
      '/alice/demo/commits/1234567890abcdef?ref=dev'
    );
  });

  it('renders the shortened sha and the formatted date per commit', () => {
    render(RecentCommitsPanel, { owner: 'alice', repo: 'demo', ref: 'main', commits });

    expect(screen.getByText('abcdef1')).toBeInTheDocument();
    expect(screen.getByText('1234567')).toBeInTheDocument();
    // formatDate mock echoes the ISO string.
    expect(screen.getByText('fmt(2026-09-01T00:00:00Z)')).toBeInTheDocument();
    expect(screen.getByText('fmt(2026-08-31T00:00:00Z)')).toBeInTheDocument();
  });
});
