import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

const releasesDelete = vi.fn();
const releasesDownloadAsset = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  releases: {
    delete: (...a: unknown[]) => releasesDelete(...a),
    downloadAsset: (...a: unknown[]) => releasesDownloadAsset(...a),
  },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

import ReleaseCard from './ReleaseCard.svelte';
import type { Release } from '$lib/types/entities';
import type { ReleaseAsset } from '$lib/api/releases';

const release = {
  id: 11,
  tag_name: 'v1.2.0',
  title: 'First stable',
  body: 'Initial release notes',
  is_prerelease: false,
  is_draft: false,
  created_at: '2020-01-01T00:00:00Z',
} as unknown as Release;

const assets: ReleaseAsset[] = [
  { id: 1, filename: 'demo-1.2.0.tar.gz', size: 2048, download_count: 3 } as ReleaseAsset,
];

describe('ReleaseCard.svelte', () => {
  beforeEach(() => {
    releasesDelete.mockReset();
    releasesDownloadAsset.mockReset();
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  it('renders tag, badges, body preview and asset rows', () => {
    render(ReleaseCard, {
      owner: 'alice',
      repo: 'demo',
      release,
      assets,
      isLatest: true,
      onChanged: () => {},
    });

    expect(screen.getByText('🏷 v1.2.0')).toBeInTheDocument();
    expect(screen.getByText('releases.latest')).toBeInTheDocument();
    expect(screen.getByText('First stable')).toBeInTheDocument();
    expect(screen.getByText('Initial release notes')).toBeInTheDocument();
    expect(screen.getByText('demo-1.2.0.tar.gz')).toBeInTheDocument();
    expect(screen.getByText(/2\.0 KB · 3 downloads/)).toBeInTheDocument();
    // The t() mock keeps the key for interpolated strings; the created
    // badge is rendered (the real formatDate path is diff > 30 days).
    expect(screen.getByText('releases.created')).toBeInTheDocument();
  });

  it('builds browse and edit links from owner/repo', () => {
    render(ReleaseCard, { owner: 'alice', repo: 'demo', release, assets, onChanged: () => {} });

    expect(screen.getByText('releases.browse_files').closest('a')).toHaveAttribute(
      'href',
      '/alice/demo?ref=v1.2.0'
    );
    expect(screen.getByText('releases.edit').closest('a')).toHaveAttribute(
      'href',
      '/alice/demo/releases/edit/11'
    );
  });

  it('downloads assets through the API and toasts on failure', async () => {
    releasesDownloadAsset.mockRejectedValueOnce(new Error('gone'));
    render(ReleaseCard, { owner: 'alice', repo: 'demo', release, assets, onChanged: () => {} });

    await fireEvent.click(screen.getByText('demo-1.2.0.tar.gz'));
    await waitFor(() =>
      expect(releasesDownloadAsset).toHaveBeenCalledWith('alice', 'demo', 1, 'demo-1.2.0.tar.gz')
    );
    await waitFor(() => expect(toastError).toHaveBeenCalled());
  });

  it('confirm-then-delete flow reloads via onChanged', async () => {
    releasesDelete.mockResolvedValue(undefined);
    const onChanged = vi.fn();
    render(ReleaseCard, { owner: 'alice', repo: 'demo', release, assets, onChanged });

    await fireEvent.click(screen.getByText('releases.delete'));
    expect(screen.getByText('Are you sure?')).toBeInTheDocument();

    await fireEvent.click(screen.getByText('common.delete'));
    await waitFor(() => expect(releasesDelete).toHaveBeenCalledWith('alice', 'demo', 11));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
    expect(toastSuccess).toHaveBeenCalledTimes(1);
  });
});
