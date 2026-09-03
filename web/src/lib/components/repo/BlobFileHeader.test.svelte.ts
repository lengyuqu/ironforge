import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

vi.mock('$app/environment', () => ({ browser: true, dev: false }));

const writeText = vi.fn().mockResolvedValue(undefined);
beforeEach(() => {
  Object.defineProperty(navigator, 'clipboard', {
    value: { writeText },
    configurable: true,
    writable: true,
  });
  writeText.mockClear();
});

import BlobFileHeader from './BlobFileHeader.svelte';

const baseProps = {
  owner: 'alice',
  repo: 'demo',
  ref: 'main',
  filePath: 'src/main.rs',
  isText: true,
  isMarkdown: false,
  lineCount: 42,
  size: 1234,
  canEdit: false,
  viewMode: 'source' as const,
  sha: 'abc123',
};

describe('BlobFileHeader.svelte', () => {
  it('renders the file name and the size meta', () => {
    render(BlobFileHeader, { ...baseProps, onToggleView: () => {}, onToggleDelete: () => {} });
    expect(screen.getByText('main.rs')).toBeInTheDocument();
    expect(screen.getByText('repo.lines_count')).toBeInTheDocument();
    const size = document.querySelector('.file-size');
    expect(size?.textContent).toContain('repo.file_size.kb');
  });

  it('copies the file path to the clipboard when copy path is clicked', async () => {
    render(BlobFileHeader, { ...baseProps, onToggleView: () => {}, onToggleDelete: () => {} });

    await fireEvent.click(screen.getByText('repo.blob.copy_path'));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith('src/main.rs'));
    expect(screen.getByText('repo.blob.copied')).toBeInTheDocument();
  });

  it('copies an absolute blob link when copy link is clicked', async () => {
    render(BlobFileHeader, { ...baseProps, onToggleView: () => {}, onToggleDelete: () => {} });

    await fireEvent.click(screen.getByText('repo.blob.copy_link'));
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        expect.stringContaining('/alice/demo/blob/src/main.rs')
      )
    );
  });

  it('shows a disabled edit control when canEdit is false', () => {
    render(BlobFileHeader, { ...baseProps, onToggleView: () => {}, onToggleDelete: () => {} });
    const edit = screen.getByText('repo.edit_file');
    expect(edit.tagName).toBe('SPAN');
  });

  it('renders an edit link when canEdit is true', () => {
    render(BlobFileHeader, {
      ...baseProps,
      canEdit: true,
      onToggleView: () => {},
      onToggleDelete: () => {},
    });
    const link = screen.getByText('repo.edit_file').closest('a') as HTMLAnchorElement;
    expect(link).not.toBeNull();
    expect(link.getAttribute('href')).toContain('/edit/src/main.rs');
  });

  it('toggles view and toggles delete through their callbacks', async () => {
    const onToggleView = vi.fn();
    const onToggleDelete = vi.fn();
    render(BlobFileHeader, {
      ...baseProps,
      isMarkdown: true,
      onToggleView,
      onToggleDelete,
    });

    await fireEvent.click(screen.getByText('repo.blob.rendered'));
    expect(onToggleView).toHaveBeenCalledTimes(1);

    await fireEvent.click(screen.getByText('repo.blob.delete_file'));
    expect(onToggleDelete).toHaveBeenCalledTimes(1);
  });
});
