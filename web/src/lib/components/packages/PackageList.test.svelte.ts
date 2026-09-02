import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

// i18n mock: t() returns the fallback for string fallbacks and the key
// otherwise (including when called with interpolation params).
vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import PackageList from './PackageList.svelte';
import type { PackageSummaryResponse } from '$lib/api/client.svelte';

const pkg: PackageSummaryResponse = {
  id: 1,
  name: 'my-crate',
  description: 'A demo crate',
  homepage: null,
  version_count: 3,
  latest_version: '1.2.0',
  download_count: 42,
  keywords: null,
  format: 'cargo',
};

describe('PackageList.svelte', () => {
  it('renders package name, version, format label and stats', () => {
    render(PackageList, {
      owner: 'alice',
      repo: 'demo',
      packages: [pkg],
      currentPage: 1,
      totalPages: 1,
      onPageChange: () => {},
    });

    expect(screen.getByText('my-crate')).toBeInTheDocument();
    // version renders as "packages.version: 1.2.0" (t key mocked)
    expect(screen.getByText(/1\.2\.0/)).toBeInTheDocument();
    expect(screen.getByText('Cargo')).toBeInTheDocument();
    expect(screen.getByText('A demo crate')).toBeInTheDocument();
  });

  it('links to the package detail route with the encoded format', () => {
    render(PackageList, {
      owner: 'alice',
      repo: 'demo',
      packages: [pkg],
      currentPage: 1,
      totalPages: 1,
      onPageChange: () => {},
    });

    const link = screen.getByRole('link', { name: /my-crate/ });
    expect(link).toHaveAttribute('href', '/alice/demo/packages/cargo/my-crate');
  });

  it('encodes scoped package names for the route', () => {
    render(PackageList, {
      owner: 'alice',
      repo: 'demo',
      packages: [{ ...pkg, name: '@scope/pkg name' }],
      currentPage: 1,
      totalPages: 1,
      onPageChange: () => {},
    });

    const link = screen.getByRole('link', { name: /@scope/ });
    // encodePackageRouteName keeps '/' as the route separator
    expect(link).toHaveAttribute('href', '/alice/demo/packages/cargo/%40scope/pkg%20name');
  });

  it('shows the empty state when there are no packages', () => {
    render(PackageList, {
      owner: 'alice',
      repo: 'demo',
      packages: [],
      currentPage: 1,
      totalPages: 1,
      onPageChange: () => {},
    });
    expect(screen.getByText('packages.no_packages')).toBeInTheDocument();
  });

  it('delegates page changes to the parent', () => {
    const onPageChange = vi.fn();
    render(PackageList, {
      owner: 'alice',
      repo: 'demo',
      packages: [pkg],
      currentPage: 2,
      totalPages: 5,
      onPageChange,
    });

    const next = screen.getByRole('button', { name: /next/i });
    next.click();
    expect(onPageChange).toHaveBeenCalledWith(3);
  });
});
