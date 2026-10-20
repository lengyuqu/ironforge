import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (d: string) => `fmt:${d}`,
}));

const blame = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  repos: { blame: (...a: unknown[]) => blame(...a) },
}));

import BlobBlameView from './BlobBlameView.svelte';

const baseProps = { owner: 'alice', repo: 'demo', ref: 'main', filePath: 'src/a.txt' };

const result = {
  path: 'src/a.txt',
  ref: 'main',
  commit_sha: 'f'.repeat(40),
  total_lines: 3,
  hunks: [
    {
      start_line: 1,
      original_start_line: 1,
      commit: {
        sha: 'a'.repeat(40),
        author: 'Alice',
        email: 'a@example.com',
        date: '2026-01-01T00:00:00+00:00',
        message: 'first',
      },
      lines: ['one', 'two'],
    },
    {
      start_line: 3,
      original_start_line: 1,
      original_path: 'old.txt',
      commit: {
        sha: 'b'.repeat(40),
        author: 'Bob',
        email: 'b@example.com',
        date: '2026-02-01T00:00:00+00:00',
        message: 'second',
      },
      lines: ['three'],
    },
  ],
};

describe('BlobBlameView.svelte', () => {
  beforeEach(() => {
    blame.mockReset();
  });

  it('requests blame for the ref and path', async () => {
    blame.mockResolvedValue(result);
    render(BlobBlameView, baseProps);
    await waitFor(() => expect(blame).toHaveBeenCalledWith('alice', 'demo', 'src/a.txt', 'main'));
  });

  it('renders every line with its absolute number', async () => {
    blame.mockResolvedValue(result);
    render(BlobBlameView, baseProps);
    await waitFor(() => expect(screen.getByText('three')).toBeInTheDocument());

    const numbers = [...document.querySelectorAll('.line-number')].map((n) => n.textContent);
    expect(numbers).toEqual(['1', '2', '3']);
    expect(screen.getByText('one')).toBeInTheDocument();
    expect(screen.getByText('two')).toBeInTheDocument();
  });

  it('shows commit info once per hunk, linked to the commit', async () => {
    blame.mockResolvedValue(result);
    render(BlobBlameView, baseProps);
    await waitFor(() => expect(screen.getByText('three')).toBeInTheDocument());

    // Two hunks → two commit cells, not three (one per line).
    expect(document.querySelectorAll('.blame-sha')).toHaveLength(2);
    const link = screen.getByText('aaaaaaa').closest('a') as HTMLAnchorElement;
    expect(link.getAttribute('href')).toBe(`/alice/demo/commits/${'a'.repeat(40)}?ref=main`);
    expect(screen.getByText('first')).toBeInTheDocument();
    expect(screen.getByText('Bob')).toBeInTheDocument();
    expect(screen.getByText('fmt:2026-02-01T00:00:00+00:00')).toBeInTheDocument();
  });

  it('shows the pre-rename path on hunks from a renamed file', async () => {
    blame.mockResolvedValue(result);
    render(BlobBlameView, baseProps);
    await waitFor(() => expect(screen.getByText('three')).toBeInTheDocument());

    expect(screen.getByText('bbbbbbb').getAttribute('title')).toBe('repo.blob.blame_renamed_from');
    expect(screen.getByText('aaaaaaa').getAttribute('title')).toBe('first');
  });

  it('shows an error when the request fails', async () => {
    blame.mockRejectedValue(new Error('binary files cannot be blamed'));
    render(BlobBlameView, baseProps);
    await waitFor(() => expect(screen.getByText('repo.blob.blame_failed')).toBeInTheDocument());
    expect(document.querySelector('.blame-table')).toBeNull();
  });

  it('shows a loading state before the response arrives', () => {
    blame.mockReturnValue(new Promise(() => {}));
    render(BlobBlameView, baseProps);
    expect(screen.getByText('repo.blob.blame_loading')).toBeInTheDocument();
  });
});
