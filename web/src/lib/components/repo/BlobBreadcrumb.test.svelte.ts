import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';

// No i18n mock needed: BlobBreadcrumb is pure presentation over repoUrls,
// which is exercised for real here (its own unit tests live in repoUrls.test.ts).

import BlobBreadcrumb from './BlobBreadcrumb.svelte';

describe('BlobBreadcrumb.svelte', () => {
  it('renders the repo root link plus one crumb per directory segment', () => {
    render(BlobBreadcrumb, {
      owner: 'alice',
      repo: 'demo',
      ref: 'main',
      filePath: 'src/lib/utils/foo.ts',
    });

    const root = screen.getByText('demo');
    expect(root).toHaveAttribute('href', '/alice/demo?ref=main');

    // Directory crumbs accumulate the path; the file name itself is not a crumb.
    expect(screen.getByText('src')).toHaveAttribute('href', '/alice/demo?ref=main&path=src');
    expect(screen.getByText('lib')).toHaveAttribute(
      'href',
      '/alice/demo?ref=main&path=src%2Flib'
    );
    expect(screen.getByText('utils')).toHaveAttribute(
      'href',
      '/alice/demo?ref=main&path=src%2Flib%2Futils'
    );
    expect(screen.queryByText('foo.ts')).not.toBeInTheDocument();
  });

  it('renders only the repo link for a root-level file', () => {
    render(BlobBreadcrumb, {
      owner: 'alice',
      repo: 'demo',
      ref: 'main',
      filePath: 'README.md',
    });

    expect(screen.getByText('demo')).toBeInTheDocument();
    // No directory segments → no separators, no crumbs.
    expect(document.querySelectorAll('.crumb-sep').length).toBe(0);
    expect(document.querySelectorAll('.crumb-link').length).toBe(1);
  });
});
