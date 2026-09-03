import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import RepoInfoSection from './RepoInfoSection.svelte';
import type { RepoInfo } from '$lib/types/entities';

function makeRepo(overrides: Partial<RepoInfo> = {}): RepoInfo {
  return {
    id: 1,
    owner: 'alice',
    name: 'demo',
    description: 'A demo repo',
    is_private: false,
    default_branch: 'main',
    created_at: '2026-09-01T00:00:00Z',
    updated_at: '2026-09-01T00:00:00Z',
    ...overrides,
  } as RepoInfo;
}

describe('RepoInfoSection.svelte', () => {
  it('renders name, description and public visibility for a public repo', () => {
    render(RepoInfoSection, { repository: makeRepo() });

    expect(screen.getByText('demo')).toBeInTheDocument();
    expect(screen.getByText('A demo repo')).toBeInTheDocument();
    const badge = document.querySelector('.badge');
    expect(badge?.textContent?.trim()).toBe('Public');
    expect(badge?.classList.contains('private')).toBe(false);
  });

  it('falls back to a dash for a missing description', () => {
    render(RepoInfoSection, { repository: makeRepo({ description: null }) });

    expect(screen.getByText('-')).toBeInTheDocument();
  });

  it('marks private repos with the private badge style', () => {
    render(RepoInfoSection, { repository: makeRepo({ is_private: true }) });

    const badge = document.querySelector('.badge');
    expect(badge?.textContent?.trim()).toBe('Private');
    expect(badge?.classList.contains('private')).toBe(true);
  });
});
