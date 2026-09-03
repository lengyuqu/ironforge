import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const boardsMoveCard = vi.fn();
const boardsCreateCard = vi.fn();
const boardsDeleteCard = vi.fn();
const boardsDeleteColumn = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  boards: {
    moveCard: (...a: unknown[]) => boardsMoveCard(...a),
    createCard: (...a: unknown[]) => boardsCreateCard(...a),
    deleteCard: (...a: unknown[]) => boardsDeleteCard(...a),
    deleteColumn: (...a: unknown[]) => boardsDeleteColumn(...a),
  },
}));

const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: vi.fn(), error: (...a: unknown[]) => toastError(...a), warning: vi.fn() },
}));

import BoardColumn from './BoardColumn.svelte';
import type { BoardCard as BoardCardType, BoardColumn as BoardColumnType } from '$lib/types/entities';

const column = { id: 1, name: 'To Do', color: '#6366f1' } as BoardColumnType;
const cards = [
  { id: 11, note: 'first task', column_id: 1, position: 0 },
  { id: 12, note: 'second task', column_id: 1, position: 1 },
] as unknown as BoardCardType[];

function renderColumn(overrides: Record<string, unknown> = {}) {
  render(BoardColumn, {
    owner: 'alice',
    repo: 'demo',
    boardId: 5,
    column,
    cards,
    onRefresh: () => {},
    ...overrides,
  });
}

describe('BoardColumn.svelte', () => {
  let confirmMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    confirmMock = vi.fn(() => true);
    vi.stubGlobal('confirm', confirmMock);
    [boardsMoveCard, boardsCreateCard, boardsDeleteCard, boardsDeleteColumn].forEach((m) =>
      m.mockReset()
    );
    toastError.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders the column name, card count and its cards', () => {
    renderColumn();

    expect(screen.getByText('To Do')).toBeInTheDocument();
    expect(screen.getByText('2')).toBeInTheDocument();
    expect(screen.getByText('first task')).toBeInTheDocument();
    expect(screen.getByText('second task')).toBeInTheDocument();
    expect(screen.getByText('+ Add card')).toBeInTheDocument();
  });

  it('adds a card through the inline form and refreshes', async () => {
    boardsCreateCard.mockResolvedValue(undefined);
    const onRefresh = vi.fn().mockResolvedValue(undefined);
    renderColumn({ onRefresh });

    await fireEvent.click(screen.getByText('+ Add card'));
    await fireEvent.input(screen.getByPlaceholderText('Add a note…'), {
      target: { value: '  new note ' },
    });
    await fireEvent.click(screen.getByText('Add'));

    await waitFor(() =>
      expect(boardsCreateCard).toHaveBeenCalledWith('alice', 'demo', 5, 1, { note: 'new note' })
    );
    await waitFor(() => expect(onRefresh).toHaveBeenCalledTimes(1));
  });

  it('drops a foreign card into the column via the API', async () => {
    boardsMoveCard.mockResolvedValue(undefined);
    const onRefresh = vi.fn().mockResolvedValue(undefined);
    renderColumn({ onRefresh });

    const columnEl = screen.getByRole('list', { name: 'To Do' });
    await fireEvent.drop(columnEl, {
      dataTransfer: { getData: () => '99' },
    });

    await waitFor(() =>
      expect(boardsMoveCard).toHaveBeenCalledWith('alice', 'demo', 5, 99, {
        column_id: 1,
        position: 2,
      })
    );
    await waitFor(() => expect(onRefresh).toHaveBeenCalledTimes(1));
  });

  it('ignores drops of cards already in this column', async () => {
    const onRefresh = vi.fn();
    renderColumn({ onRefresh });

    const columnEl = screen.getByRole('list', { name: 'To Do' });
    await fireEvent.drop(columnEl, {
      dataTransfer: { getData: () => '11' },
    });

    expect(boardsMoveCard).not.toHaveBeenCalled();
    expect(onRefresh).not.toHaveBeenCalled();
  });

  it('deletes the column after confirmation', async () => {
    boardsDeleteColumn.mockResolvedValue(undefined);
    const onRefresh = vi.fn().mockResolvedValue(undefined);
    renderColumn({ onRefresh });

    await fireEvent.click(screen.getByTitle('Delete column'));
    await waitFor(() =>
      expect(boardsDeleteColumn).toHaveBeenCalledWith('alice', 'demo', 5, 1)
    );
    await waitFor(() => expect(onRefresh).toHaveBeenCalledTimes(1));
  });
});
