import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import FileTreePanel from './FileTreePanel.svelte';
import type { RepoTreeEntry } from '$lib/types/entities';

const dir: RepoTreeEntry = { name: 'src', kind: 'dir' };
const dirTree: RepoTreeEntry = { name: 'docs', kind: 'tree' };
const smallFile: RepoTreeEntry = { name: 'file.txt', kind: 'blob', size: 2048 };
const noSizeFile: RepoTreeEntry = { name: 'LICENSE', kind: 'blob', size: null };

function renderPanel(overrides: Record<string, unknown> = {}) {
  const onNavigateTo = vi.fn();
  const onNavigateUp = vi.fn();
  const props = {
    owner: 'alice',
    repo: 'demo',
    ref: 'main',
    path: '',
    entries: [dir, dirTree, smallFile, noSizeFile] as RepoTreeEntry[],
    onNavigateTo,
    onNavigateUp,
    ...overrides,
  };
  const utils = render(FileTreePanel, props);
  return { ...utils, onNavigateTo, onNavigateUp };
}

describe('FileTreePanel.svelte', () => {
  it('delegates directory clicks to onNavigateTo (both dir and tree kinds)', () => {
    const { onNavigateTo } = renderPanel();

    fireEvent.click(screen.getByText('src'));
    expect(onNavigateTo).toHaveBeenCalledWith('src');

    fireEvent.click(screen.getByText('docs'));
    expect(onNavigateTo).toHaveBeenCalledWith('docs');
  });

  it('links file entries to the blob viewer, prefixing the current path', () => {
    const { rerender } = renderPanel({ path: 'src' });

    expect(screen.getByText('file.txt').closest('a')).toHaveAttribute(
      'href',
      '/alice/demo/blob/src/file.txt?ref=main'
    );

    rerender({
      owner: 'alice',
      repo: 'demo',
      ref: 'main',
      path: '',
      entries: [smallFile] as RepoTreeEntry[],
      onNavigateTo: () => {},
      onNavigateUp: () => {},
    });
    expect(screen.getByText('file.txt').closest('a')).toHaveAttribute(
      'href',
      '/alice/demo/blob/file.txt?ref=main'
    );
  });

  it('shows the parent (..) entry only when nested, wired to onNavigateUp', () => {
    // Root level: no parent entry.
    const { rerender } = renderPanel({ path: '' });
    expect(screen.queryByText('..')).not.toBeInTheDocument();

    // Nested: parent entry appears and delegates upward.
    const onNavigateUp = vi.fn();
    rerender({
      owner: 'alice',
      repo: 'demo',
      ref: 'main',
      path: 'src',
      entries: [smallFile] as RepoTreeEntry[],
      onNavigateTo: () => {},
      onNavigateUp,
    });
    fireEvent.click(screen.getByText('..'));
    expect(onNavigateUp).toHaveBeenCalledTimes(1);
  });

  it('formats file sizes and hides the size column when size is null', () => {
    renderPanel();

    // mock t() returns the key → 2048 bytes renders as "2.0" + kb key.
    expect(screen.getByText('2.0repo.file_size.kb')).toBeInTheDocument();
    expect(document.querySelectorAll('.entry-size').length).toBe(1);
  });
});
