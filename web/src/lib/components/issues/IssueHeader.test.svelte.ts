import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import IssueHeader from './IssueHeader.svelte';
import type { Issue } from '$lib/types/entities';

describe('IssueHeader.svelte', () => {
  it('renders title, number, state and labels for an open issue', () => {
    const issue = {
      id: 1,
      number: 12,
      title: 'Broken build',
      state: 'open',
      author: 'alice',
      labels: ['bug', 'enhancement'],
      created_at: '2024-01-01T00:00:00Z',
    } as Issue;
    render(IssueHeader, { issue });
    expect(screen.getByRole('heading', { level: 1 }).textContent).toBe('Broken build');
    expect(screen.getByText('#12')).toBeInTheDocument();
    expect(screen.getByText('issues.state.open')).toBeInTheDocument();
    expect(screen.getByText('issues.opened_by')).toBeInTheDocument();
    expect(document.querySelectorAll('.label-badge')).toHaveLength(2);
  });

  it('marks a closed issue with the closed state class', () => {
    const issue = { id: 2, number: 13, title: 'Done', state: 'closed', author: 'bob' } as Issue;
    render(IssueHeader, { issue });
    const badge = screen.getByText('issues.state.closed');
    expect(badge.classList.contains('closed')).toBe(true);
  });

  it('renders no label badges when the issue has none', () => {
    const issue = { id: 3, number: 14, title: 'No labels', state: 'open', author: 'bob' } as Issue;
    render(IssueHeader, { issue });
    expect(document.querySelectorAll('.label-badge')).toHaveLength(0);
  });
});
