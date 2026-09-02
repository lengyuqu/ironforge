import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

// i18n mock: t() returns the fallback for string fallbacks and the key
// otherwise (including when called with interpolation params). Toast is
// mocked so ReleaseCard's self-contained actions stay inert.
vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: () => {}, error: () => {}, info: () => {} },
}));

import ReleaseList from './ReleaseList.svelte';
import type { Release } from '$lib/types/entities';
import type { ReleaseAsset } from '$lib/api/releases';

const releases: Release[] = [
  {
    id: 1,
    tag_name: 'v1.0.0',
    target_commitish: 'main',
    title: 'First stable',
    body: 'Initial release',
    is_draft: false,
    is_prerelease: false,
    created_at: '2026-09-01T00:00:00Z',
  },
  {
    id: 2,
    tag_name: 'v0.9.0',
    target_commitish: 'main',
    title: 'RC build',
    body: null,
    is_draft: false,
    is_prerelease: true,
    created_at: '2026-08-01T00:00:00Z',
  },
];

const assets: Record<number, ReleaseAsset[]> = { 1: [], 2: [] };

describe('ReleaseList.svelte', () => {
  it('renders one card per release with tag and title', () => {
    render(ReleaseList, {
      owner: 'alice',
      repo: 'demo',
      releases,
      assets,
      currentPage: 1,
      totalPages: 1,
      onReload: () => {},
      onPageChange: () => {},
    });

    expect(screen.getByText('First stable')).toBeInTheDocument();
    expect(screen.getByText('RC build')).toBeInTheDocument();
    expect(screen.getAllByText(/v1\.0\.0/).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/v0\.9\.0/).length).toBeGreaterThan(0);
  });

  it('shows pagination controls only when there is more than one page', () => {
    const { rerender } = render(ReleaseList, {
      owner: 'alice',
      repo: 'demo',
      releases: [],
      assets: {},
      currentPage: 1,
      totalPages: 1,
      onReload: () => {},
      onPageChange: () => {},
    });
    expect(screen.queryByText(/Page 1 of 1/)).not.toBeInTheDocument();

    rerender({
      owner: 'alice',
      repo: 'demo',
      releases: [],
      assets: {},
      currentPage: 2,
      totalPages: 5,
      onReload: () => {},
      onPageChange: () => {},
    });
    expect(screen.getByText('Page 2 of 5')).toBeInTheDocument();
  });

  it('delegates page changes to the parent', () => {
    const onPageChange = vi.fn();
    render(ReleaseList, {
      owner: 'alice',
      repo: 'demo',
      releases: [],
      assets: {},
      currentPage: 2,
      totalPages: 3,
      onReload: () => {},
      onPageChange,
    });

    fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    expect(onPageChange).toHaveBeenCalledWith(3);

    fireEvent.click(screen.getByRole('button', { name: 'Previous' }));
    expect(onPageChange).toHaveBeenCalledWith(1);
  });

  it('disables Previous on the first page and Next on the last', () => {
    render(ReleaseList, {
      owner: 'alice',
      repo: 'demo',
      releases: [],
      assets: {},
      currentPage: 1,
      totalPages: 2,
      onReload: () => {},
      onPageChange: () => {},
    });

    expect(screen.getByRole('button', { name: 'Previous' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Next' })).toBeEnabled();
  });
});
