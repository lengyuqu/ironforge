import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import DashboardRepoGrid from './DashboardRepoGrid.svelte';
import type { ExploreRepo } from '$lib/types/entities';

function makeRepo(overrides: Partial<ExploreRepo> = {}): ExploreRepo {
  return {
    id: 1,
    owner_name: 'alice',
    name: 'demo',
    description: 'A demo repo',
    stars_count: 7,
    updated_at: '2026-09-01T00:00:00Z',
    ...overrides,
  } as ExploreRepo;
}

describe('DashboardRepoGrid.svelte', () => {
  it('shows the loading placeholder', () => {
    render(DashboardRepoGrid, { repos: [], loading: true });

    expect(screen.getByText('common.loading')).toBeInTheDocument();
  });

  it('shows the empty state with a getting-started hint', () => {
    render(DashboardRepoGrid, { repos: [], loading: false });

    expect(screen.getByText('dashboard.empty.no_repos')).toBeInTheDocument();
    expect(screen.getByText('dashboard.empty.get_started')).toBeInTheDocument();
  });

  it('renders repo cards linking to their pages with fallbacks', () => {
    render(DashboardRepoGrid, {
      repos: [
        makeRepo(),
        makeRepo({
          id: 2,
          owner_name: undefined,
          name: 'orphan',
          description: null,
          stars_count: 0,
        } as Partial<ExploreRepo>),
      ],
      loading: false,
    });

    expect(screen.getByText('alice/demo').closest('a')).toHaveAttribute('href', '/alice/demo');
    // Null owner falls back to 'unknown', null description to the i18n key.
    expect(screen.getByText('unknown/orphan').closest('a')).toHaveAttribute('href', '/unknown/orphan');
    expect(screen.getAllByText('common.no_description').length).toBe(1);
    // t('common.updated', {date}) with the key-returning mock → plain key
    // in every card's meta line.
    expect(screen.getAllByText(/common\.updated/).length).toBe(2);
  });
});
