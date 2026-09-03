import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const labelsDelete = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  labels: { delete: (...a: unknown[]) => labelsDelete(...a) },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

import LabelDeleteModal from './LabelDeleteModal.svelte';
import type { Label } from '$lib/types/entities';

const label = { id: 3, name: 'bug', color: '#ff0000', description: '' } as Label;

describe('LabelDeleteModal.svelte', () => {
  beforeEach(() => {
    labelsDelete.mockReset();
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  it('renders the confirmation dialog naming the label', () => {
    render(LabelDeleteModal, {
      owner: 'alice',
      repo: 'demo',
      label,
      onClose: () => {},
      onDeleted: () => {},
    });

    expect(screen.getByText('Confirm Delete')).toBeInTheDocument();
    expect(screen.getByText('settings.confirm_delete_label')).toBeInTheDocument();
    expect(screen.getByText('bug')).toBeInTheDocument();
    expect(screen.getByText('Delete')).toBeInTheDocument();
  });

  it('deletes via the API exactly once, toasts, closes and refreshes', async () => {
    labelsDelete.mockResolvedValue(undefined);
    const onClose = vi.fn();
    const onDeleted = vi.fn();
    render(LabelDeleteModal, {
      owner: 'alice',
      repo: 'demo',
      label,
      onClose,
      onDeleted,
    });

    await fireEvent.click(screen.getByText('Delete'));
    await waitFor(() => expect(labelsDelete).toHaveBeenCalledWith('alice', 'demo', 3));
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledTimes(1));
    // Exactly one close call: the click must not bubble to the overlay.
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(onDeleted).toHaveBeenCalledTimes(1);
  });

  it('toasts the failure and keeps the dialog open', async () => {
    labelsDelete.mockRejectedValue(new Error('in use'));
    const onClose = vi.fn();
    render(LabelDeleteModal, {
      owner: 'alice',
      repo: 'demo',
      label,
      onClose,
      onDeleted: () => {},
    });

    await fireEvent.click(screen.getByText('Delete'));
    await waitFor(() => expect(toastError).toHaveBeenCalled());
    expect(onClose).not.toHaveBeenCalled();
  });
});
