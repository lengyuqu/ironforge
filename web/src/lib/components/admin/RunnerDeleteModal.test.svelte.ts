import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const runnersDelete = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  runners: { delete: (...a: unknown[]) => runnersDelete(...a) },
}));

const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: vi.fn(), error: (...a: unknown[]) => toastError(...a), warning: vi.fn() },
}));

import RunnerDeleteModal from './RunnerDeleteModal.svelte';
import type { RunnerListItem } from '$lib/api/client.svelte';

const runner = {
  id: 7,
  name: 'linux-runner-01',
  status: 'online',
  labels: [],
} as unknown as RunnerListItem;

describe('RunnerDeleteModal.svelte', () => {
  beforeEach(() => {
    runnersDelete.mockReset();
    toastError.mockClear();
  });

  it('renders the confirmation dialog', () => {
    render(RunnerDeleteModal, { runner, onClose: () => {}, onDeleted: () => {} });

    expect(screen.getByText('admin.runners.delete_confirm')).toBeInTheDocument();
    expect(screen.getByText('admin.runners.delete_warning')).toBeInTheDocument();
    expect(screen.getByText('common.delete')).toBeEnabled();
    expect(screen.getByText('common.cancel')).toBeInTheDocument();
  });

  it('deletes, closes and refreshes on confirm', async () => {
    runnersDelete.mockResolvedValue(undefined);
    const onClose = vi.fn();
    const onDeleted = vi.fn().mockResolvedValue(undefined);
    render(RunnerDeleteModal, { runner, onClose, onDeleted });

    await fireEvent.click(screen.getByText('common.delete'));
    await waitFor(() => expect(runnersDelete).toHaveBeenCalledWith(7));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(onDeleted).toHaveBeenCalledTimes(1));
  });

  it('keeps the modal open and toasts on failure', async () => {
    runnersDelete.mockRejectedValue(new Error('still busy'));
    const onClose = vi.fn();
    render(RunnerDeleteModal, { runner, onClose, onDeleted: () => {} });

    await fireEvent.click(screen.getByText('common.delete'));
    await waitFor(() => expect(toastError).toHaveBeenCalled());
    expect(onClose).not.toHaveBeenCalled();
  });
});
