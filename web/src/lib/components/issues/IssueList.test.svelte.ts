import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

// i18n mock: t() returns the fallback for string fallbacks and the key
// otherwise (including when called with interpolation params).
vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import IssueList from './IssueList.svelte';
import type { Issue } from '$lib/types/entities';

const issue: Issue = {
  id: 1,
  number: 7,
  title: 'Fix the bug',
  body: 'desc',
  state: 'open',
  author: 'alice',
  labels: ['bug', 'ui'],
  created_at: '2026-09-01T00:00:00Z',
};

describe('IssueList.svelte', () => {
  it('shows the loading placeholder while loading', () => {
    render(IssueList, {
      owner: 'alice',
      repo: 'demo',
      issues: [],
      loading: true,
      filter: 'all',
    });
    expect(screen.getByText('common.loading')).toBeInTheDocument();
  });

  it('renders titles and labels for each issue', () => {
    render(IssueList, {
      owner: 'alice',
      repo: 'demo',
      issues: [issue],
      loading: false,
      filter: 'all',
    });

    expect(screen.getByText('Fix the bug')).toBeInTheDocument();
    expect(screen.getByText('bug')).toBeInTheDocument();
    expect(screen.getByText('ui')).toBeInTheDocument();
  });

  it('links each item to the issue detail route', () => {
    render(IssueList, {
      owner: 'alice',
      repo: 'demo',
      issues: [issue],
      loading: false,
      filter: 'all',
    });

    const link = screen.getByRole('link', { name: /Fix the bug/ });
    expect(link).toHaveAttribute('href', '/alice/demo/issues/7');
  });

  it('shows the empty state when the filtered list is empty', () => {
    render(IssueList, {
      owner: 'alice',
      repo: 'demo',
      issues: [],
      loading: false,
      filter: 'closed',
    });
    // t('issues.empty', { state: ... }) → mocked key
    expect(screen.getByText('issues.empty')).toBeInTheDocument();
  });

  it('renders a milestone badge from the enriched milestone_title (M2-2 A3)', () => {
    render(IssueList, {
      owner: 'alice',
      repo: 'demo',
      issues: [{ ...issue, milestone_id: 3, milestone_title: 'v1.0 Launch' }],
      loading: false,
      filter: 'all',
    });
    expect(screen.getByText('v1.0 Launch')).toBeInTheDocument();
  });

  it('omits the milestone badge when milestone_title is unset', () => {
    render(IssueList, {
      owner: 'alice',
      repo: 'demo',
      issues: [issue],
      loading: false,
      filter: 'all',
    });
    expect(screen.queryByTitle('issues.milestone')).not.toBeInTheDocument();
  });
});
