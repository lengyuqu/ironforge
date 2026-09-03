import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';

import StatusChecksPanel from './StatusChecksPanel.svelte';
import type { CombinedCommitStatus, CommitStatus } from '$lib/types/entities';

const futureDate = new Date(Date.now() + 3600 * 1000).toISOString();

const combined: CombinedCommitStatus = {
  state: 'success',
  sha: 'abc',
  total_count: 2,
  statuses: [],
};

const statuses: CommitStatus[] = [
  {
    id: 1,
    sha: 'abc',
    state: 'success',
    context: 'ci/build',
    description: 'Build passed',
    target_url: 'https://ci.example.com/build/1',
    created_at: futureDate,
  },
  {
    id: 2,
    sha: 'abc',
    state: 'pending',
    context: 'ci/lint',
    description: 'Lint running',
    target_url: null,
    created_at: futureDate,
  },
];

describe('StatusChecksPanel.svelte', () => {
  it('renders the combined banner and individual status cards with a details link', () => {
    render(StatusChecksPanel, { combined, statuses });

    expect(screen.getByText('All checks passed')).toBeInTheDocument();
    expect(screen.getByText('2 checks')).toBeInTheDocument();
    expect(screen.getByText('ci/build')).toBeInTheDocument();
    expect(screen.getByText('Build passed')).toBeInTheDocument();

    const link = screen.getByText('View Details →').closest('a') as HTMLAnchorElement;
    expect(link).not.toBeNull();
    expect(link.getAttribute('href')).toBe('https://ci.example.com/build/1');
    expect(link.getAttribute('rel')).toContain('noopener');
    expect(link.getAttribute('target')).toBe('_blank');
  });

  it('renders the empty state when there are no statuses', () => {
    render(StatusChecksPanel, { combined: null, statuses: [] });

    expect(screen.getByText('No status checks reported yet.')).toBeInTheDocument();
    expect(document.querySelector('.combined-status')).not.toBeInTheDocument();
  });

  it('shows the Status Checks section even without a combined status', () => {
    render(StatusChecksPanel, { combined: null, statuses });

    expect(document.querySelector('.combined-status')).not.toBeInTheDocument();
    expect(screen.getByText('Status Checks')).toBeInTheDocument();
    expect(screen.getByText('ci/lint')).toBeInTheDocument();
  });
});
