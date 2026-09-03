import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

import BoardSwitcher from './BoardSwitcher.svelte';
import type { Board } from '$lib/types/entities';

function makeBoard(id: number, name: string): Board {
  return {
    id,
    repo_id: 1,
    org_id: null,
    name,
    description: null,
    created_by: 1,
    created_at: '2026-09-01T00:00:00Z',
    updated_at: '2026-09-01T00:00:00Z',
  } as Board;
}

const boards = [makeBoard(1, 'Backlog'), makeBoard(2, 'Sprint 1'), makeBoard(3, 'Done')];

describe('BoardSwitcher.svelte', () => {
  it('renders one tab per board plus the add button', () => {
    render(BoardSwitcher, { boards, activeBoardId: 1, onSelect: () => {}, onAddBoard: () => {} });

    expect(screen.getByText('Backlog')).toBeInTheDocument();
    expect(screen.getByText('Sprint 1')).toBeInTheDocument();
    expect(screen.getByText('Done')).toBeInTheDocument();
    expect(screen.getByText('+ Board')).toBeInTheDocument();
  });

  it('marks only the active board', () => {
    render(BoardSwitcher, { boards, activeBoardId: 2, onSelect: () => {}, onAddBoard: () => {} });

    const active = document.querySelector('.board-tab.active');
    expect(active?.textContent).toBe('Sprint 1');
    expect(document.querySelectorAll('.board-tab.active').length).toBe(1);
  });

  it('delegates board selection and board creation to the parent', () => {
    const onSelect = vi.fn();
    const onAddBoard = vi.fn();
    render(BoardSwitcher, { boards, activeBoardId: 1, onSelect, onAddBoard });

    fireEvent.click(screen.getByText('Done'));
    expect(onSelect).toHaveBeenCalledWith(3);

    fireEvent.click(screen.getByText('+ Board'));
    expect(onAddBoard).toHaveBeenCalledTimes(1);
  });
});
