import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const packagesDelete = vi.fn();
const packagesDownloadUrl = vi.fn(
  (owner: string, repo: string, format: string, name: string, ver: string, filename: string) =>
    `/api/v1/repos/${owner}/${repo}/packages/${format}/${name}/${ver}/${filename}`
);
vi.mock('$lib/api/client.svelte', () => ({
  packages: {
    delete: (...a: unknown[]) => packagesDelete(...a),
    downloadUrl: (
      owner: string,
      repo: string,
      pkg_type: string,
      pkg_name: string,
      version: string,
      filename: string
    ) => packagesDownloadUrl(owner, repo, pkg_type, pkg_name, version, filename),
  },
}));

import PackageVersions from './PackageVersions.svelte';
import type { PackageVersionResponse } from '$lib/api/client.svelte';

const versions: PackageVersionResponse[] = [
  {
    id: 1,
    version: '1.2.0',
    files: [{ id: 10, filename: 'mypkg-1.2.0.tar.gz', size: 512 }],
  },
] as unknown as PackageVersionResponse[];

function renderIt(props: Record<string, unknown> = {}) {
  render(PackageVersions, {
    owner: 'alice',
    repo: 'demo',
    format: 'npm',
    name: '@scope/mypkg',
    packageName: '@scope/mypkg',
    versions,
    onRefresh: () => {},
    ...props,
  });
}

describe('PackageVersions.svelte', () => {
  beforeEach(() => {
    packagesDelete.mockReset();
    packagesDownloadUrl.mockClear();
  });

  it('renders version rows with file links built from downloadUrl', () => {
    renderIt();

    expect(screen.getByText('v1.2.0')).toBeInTheDocument();
    const fileLink = screen.getByText('mypkg-1.2.0.tar.gz').closest('a');
    expect(fileLink).toHaveAttribute(
      'href',
      '/api/v1/repos/alice/demo/packages/npm/@scope/mypkg/1.2.0/mypkg-1.2.0.tar.gz'
    );
    expect(screen.getByText('512 B')).toBeInTheDocument();
  });

  it('deletes a version after confirmation and refreshes', async () => {
    packagesDelete.mockResolvedValue(undefined);
    const onRefresh = vi.fn().mockResolvedValue(undefined);
    renderIt({ onRefresh });

    // Two delete buttons pre-confirm; click the header one to arm confirm.
    await fireEvent.click(screen.getAllByText('common.delete')[0]);

    // Confirm the inline delete (the second button in the confirm row).
    await fireEvent.click(screen.getAllByText('common.delete')[1]);
    await waitFor(() =>
      expect(packagesDelete).toHaveBeenCalledWith('alice', 'demo', 'npm', '@scope/mypkg', '1.2.0')
    );
    await waitFor(() => expect(onRefresh).toHaveBeenCalledTimes(1));
  });

  it('cancelling the confirm keeps the version', async () => {
    renderIt();

    await fireEvent.click(screen.getAllByText('common.delete')[0]);
    await fireEvent.click(screen.getByText('common.cancel'));
    expect(packagesDelete).not.toHaveBeenCalled();
  });

  it('shows the error banner when deletion fails', async () => {
    packagesDelete.mockRejectedValue(new Error('still referenced'));
    renderIt();

    await fireEvent.click(screen.getAllByText('common.delete')[0]);
    await fireEvent.click(screen.getAllByText('common.delete')[1]);
    expect(await screen.findByText('still referenced')).toBeInTheDocument();
  });
});
