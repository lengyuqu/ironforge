import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const boardsCreateColumn = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  boards: { createColumn: (...a: unknown[]) => boardsCreateColumn(...a) },
}));

const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: vi.fn(), error: (...a: unknown[]) => toastError(...a), warning: vi.fn() },
}));

import ColumnCreateForm from './ColumnCreateForm.svelte';

describe('ColumnCreateForm.svelte', () => {
  beforeEach(() => {
    boardsCreateColumn.mockReset();
    toastError.mockClear();
  });

  it('ignores blank names', async () => {
    render(ColumnCreateForm, {
      owner: 'alice',
      repo: 'demo',
      boardId: 5,
      onCreated: () => {},
      onCancel: () => {},
    });

    await fireEvent.click(screen.getByText('Add'));
    expect(boardsCreateColumn).not.toHaveBeenCalled();
  });

  it('creates a column with the trimmed name and refreshes', async () => {
    boardsCreateColumn.mockResolvedValue(undefined);
    const onCreated = vi.fn();
    render(ColumnCreateForm, { owner: 'alice', repo: 'demo', boardId: 5, onCreated, onCancel: () => {} });

    await fireEvent.input(screen.getByPlaceholderText('Column name'), {
      target: { value: '  In Progress ' },
    });
    await fireEvent.click(screen.getByText('Add'));

    await waitFor(() =>
      expect(boardsCreateColumn).toHaveBeenCalledWith('alice', 'demo', 5, {
        name: 'In Progress',
      })
    );
    await waitFor(() => expect(onCreated).toHaveBeenCalledTimes(1));
  });

  it('toasts when creation fails', async () => {
    boardsCreateColumn.mockRejectedValue(new Error('limit reached'));
    render(ColumnCreateForm, {
      owner: 'alice',
      repo: 'demo',
      boardId: 5,
      onCreated: () => {},
      onCancel: () => {},
    });

    await fireEvent.input(screen.getByPlaceholderText('Column name'), {
      target: { value: 'c' },
    });
    await fireEvent.click(screen.getByText('Add'));

    await waitFor(() => expect(toastError).toHaveBeenCalledWith('limit reached'));
  });
});
