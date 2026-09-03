import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const boardsCreate = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  boards: { create: (...a: unknown[]) => boardsCreate(...a) },
}));

const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: vi.fn(), error: (...a: unknown[]) => toastError(...a), warning: vi.fn() },
}));

import BoardCreateForm from './BoardCreateForm.svelte';

describe('BoardCreateForm.svelte', () => {
  beforeEach(() => {
    boardsCreate.mockReset();
    toastError.mockClear();
  });

  it('ignores creation when the name is blank', async () => {
    render(BoardCreateForm, {
      owner: 'alice',
      repo: 'demo',
      onCreated: () => {},
      onCancel: () => {},
    });

    await fireEvent.click(screen.getByText('Create'));
    expect(boardsCreate).not.toHaveBeenCalled();
  });

  it('creates a board with the trimmed name and reports the new id', async () => {
    boardsCreate.mockResolvedValue({ id: 8 });
    const onCreated = vi.fn();
    const onCancel = vi.fn();
    render(BoardCreateForm, { owner: 'alice', repo: 'demo', onCreated, onCancel });

    await fireEvent.input(screen.getByPlaceholderText('Board name'), {
      target: { value: '  Roadmap ' },
    });
    await fireEvent.click(screen.getByText('Create'));

    await waitFor(() =>
      expect(boardsCreate).toHaveBeenCalledWith('alice', 'demo', { name: 'Roadmap' })
    );
    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(8));
    // The input is cleared after a successful create.
    expect((screen.getByPlaceholderText('Board name') as HTMLInputElement).value).toBe('');
  });

  it('toasts when creation fails and stays interactive', async () => {
    boardsCreate.mockRejectedValue(new Error('quota'));
    render(BoardCreateForm, {
      owner: 'alice',
      repo: 'demo',
      onCreated: () => {},
      onCancel: () => {},
    });

    await fireEvent.input(screen.getByPlaceholderText('Board name'), {
      target: { value: 'x' },
    });
    await fireEvent.click(screen.getByText('Create'));

    await waitFor(() => expect(toastError).toHaveBeenCalledWith('quota'));
    expect((screen.getByText('Create') as HTMLButtonElement).disabled).toBe(false);
  });
});
