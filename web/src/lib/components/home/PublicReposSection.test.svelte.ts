import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import PublicReposSection from './PublicReposSection.svelte';
import type { ExploreRepo } from '$lib/types/entities';

function makeRepo(overrides: Partial<ExploreRepo> = {}): ExploreRepo {
  return {
    id: 1,
    owner_name: 'alice',
    name: 'demo',
    description: 'A demo repo',
    stars_count: 3,
    updated_at: '2026-09-01T00:00:00Z',
    ...overrides,
  } as ExploreRepo;
}

describe('PublicReposSection.svelte', () => {
  it('renders the explore link and repo items', () => {
    render(PublicReposSection, { repos: [makeRepo()], loading: false, error: '', onRetry: () => {} });

    // The anchor text carries a trailing "→" — match by prefix.
    expect(screen.getByText(/home\.explore\.view_all/)).toHaveAttribute('href', '/explore');
    expect(screen.getByText('alice/demo').closest('a')).toHaveAttribute('href', '/alice/demo');
  });

  it('shows the empty state when no public repos exist', () => {
    render(PublicReposSection, { repos: [], loading: false, error: '', onRetry: () => {} });

    expect(screen.getByText('explore.empty')).toBeInTheDocument();
  });

  it('surfaces the error with a retry button delegating to onRetry', () => {
    const onRetry = vi.fn();
    render(PublicReposSection, { repos: [], loading: false, error: 'upstream down', onRetry });

    expect(screen.getByText('upstream down')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Retry'));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});
