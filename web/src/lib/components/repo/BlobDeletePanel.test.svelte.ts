import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const deleteContent = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  repos: { deleteContent: (...a: unknown[]) => deleteContent(...a) },
}));

import BlobDeletePanel from './BlobDeletePanel.svelte';

const baseProps = {
  owner: 'alice',
  repo: 'demo',
  ref: 'main',
  filePath: 'src/foo.rs',
  sha: 'deadbeef',
};

describe('BlobDeletePanel.svelte', () => {
  beforeEach(() => {
    deleteContent.mockReset();
  });

  it('renders the title with the file path and a prefilled message', () => {
    const onClose = vi.fn();
    const onDeleted = vi.fn();
    render(BlobDeletePanel, { ...baseProps, onClose, onDeleted });

    expect(screen.getByText('repo.blob.delete_title')).toBeInTheDocument();
    expect(screen.getByText('src/foo.rs')).toBeInTheDocument();
    const input = screen.getByLabelText('repo.blob.delete_message') as HTMLInputElement;
    expect(input.value).toBe('Delete src/foo.rs');
    expect(screen.getByText('repo.blob.delete_file')).toBeInTheDocument();
    expect(screen.getByText('common.cancel')).toBeInTheDocument();
  });

  it('deletes via the API and fires onDeleted but not onClose on success', async () => {
    deleteContent.mockResolvedValue(undefined);
    const onClose = vi.fn();
    const onDeleted = vi.fn();
    render(BlobDeletePanel, { ...baseProps, onClose, onDeleted });

    await fireEvent.click(screen.getByText('repo.blob.delete_file'));
    await waitFor(() =>
      expect(deleteContent).toHaveBeenCalledWith('alice', 'demo', 'src/foo.rs', {
        branch: 'main',
        message: 'Delete src/foo.rs',
        sha: 'deadbeef',
      })
    );
    expect(onDeleted).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();
  });

  it('fires onClose when cancel is clicked', async () => {
    const onClose = vi.fn();
    const onDeleted = vi.fn();
    render(BlobDeletePanel, { ...baseProps, onClose, onDeleted });

    await fireEvent.click(screen.getByText('common.cancel'));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(deleteContent).not.toHaveBeenCalled();
    expect(onDeleted).not.toHaveBeenCalled();
  });

  it('shows the conflict error and stays open on a conflict failure', async () => {
    deleteContent.mockRejectedValue(new Error('SHA mismatch: conflict detected'));
    const onClose = vi.fn();
    const onDeleted = vi.fn();
    render(BlobDeletePanel, { ...baseProps, onClose, onDeleted });

    await fireEvent.click(screen.getByText('repo.blob.delete_file'));
    await waitFor(() => expect(screen.getByText('repo.blob.delete_conflict')).toBeInTheDocument());
    expect(onClose).not.toHaveBeenCalled();
    expect(onDeleted).not.toHaveBeenCalled();
  });

  it('shows the generic failure error on a non-conflict failure', async () => {
    deleteContent.mockRejectedValue(new Error('boom'));
    const onClose = vi.fn();
    const onDeleted = vi.fn();
    render(BlobDeletePanel, { ...baseProps, onClose, onDeleted });

    await fireEvent.click(screen.getByText('repo.blob.delete_file'));
    await waitFor(() => expect(screen.getByText('repo.blob.delete_failed')).toBeInTheDocument());
    expect(onDeleted).not.toHaveBeenCalled();
  });
});
