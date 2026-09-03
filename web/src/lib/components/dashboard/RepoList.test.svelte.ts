import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import RepoList from './RepoList.svelte';

const repos = [
  {
    id: 1,
    name: 'public-repo',
    description: 'A public one',
    is_private: false,
    created_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 2,
    name: 'secret-repo',
    description: null,
    is_private: true,
    created_at: '2026-02-01T00:00:00Z',
  },
];

describe('RepoList.svelte', () => {
  it('renders repo links, descriptions and fallbacks', () => {
    render(RepoList, { owner: 'alice', repos });

    expect(
      screen.getByText('alice/public-repo').closest('a')
    ).toHaveAttribute('href', '/alice/public-repo');
    expect(
      screen.getByText('alice/secret-repo').closest('a')
    ).toHaveAttribute('href', '/alice/secret-repo');
    expect(screen.getByText('A public one')).toBeInTheDocument();
    // Null description falls back to the no-description key.
    expect(screen.getByText('common.no_description')).toBeInTheDocument();
    // Created meta keeps the t() key (interpolated date is dropped by the mock).
    expect(screen.getAllByText('common.created')).toHaveLength(2);
  });

  it('marks private repos with a badge and lock icon', () => {
    render(RepoList, { owner: 'alice', repos });

    expect(screen.getByText('dashboard.repo.private')).toBeInTheDocument();
    expect(screen.getByText('🔒')).toBeInTheDocument();
    expect(screen.getByText('📂')).toBeInTheDocument();
  });
});
