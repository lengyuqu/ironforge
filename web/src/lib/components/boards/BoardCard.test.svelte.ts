import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

// No i18n needed: BoardCard renders raw card data and a hardcoded title.

import BoardCard from './BoardCard.svelte';
import type { BoardCard as BoardCardType } from '$lib/types/entities';

function makeCard(overrides: Partial<BoardCardType> = {}): BoardCardType {
  return {
    id: 42,
    column_id: 7,
    issue_id: 100,
    note: 'Remember the milk',
    position: 1,
    created_at: '2026-09-01T00:00:00Z',
    updated_at: '2026-09-01T00:00:00Z',
    issue: { id: 100, number: 12, title: 'Bug' },
    ...overrides,
  };
}

describe('BoardCard.svelte', () => {
  it('links to the issue and shows the note when both exist', () => {
    render(BoardCard, {
      owner: 'alice',
      repo: 'demo',
      card: makeCard(),
      onDragStart: () => {},
      onDelete: () => {},
    });

    expect(screen.getByText('#12')).toHaveAttribute('href', '/alice/demo/issues/12');
    expect(screen.getByText('Remember the milk')).toBeInTheDocument();
  });

  it('omits the issue link for plain note cards (issue = null)', () => {
    render(BoardCard, {
      owner: 'alice',
      repo: 'demo',
      card: makeCard({ issue: null, issue_id: null }),
      onDragStart: () => {},
      onDelete: () => {},
    });

    expect(screen.queryByText(/#\d+/)).not.toBeInTheDocument();
    // Note slot renders an empty span, card itself stays present.
    expect(document.querySelector('.card')).toBeInTheDocument();
  });

  it('delegates deletion and drag start to the parent with the card id', () => {
    const onDragStart = vi.fn();
    const onDelete = vi.fn();
    render(BoardCard, {
      owner: 'alice',
      repo: 'demo',
      card: makeCard(),
      onDragStart,
      onDelete,
    });

    fireEvent.click(screen.getByTitle('Remove card'));
    expect(onDelete).toHaveBeenCalledWith(42);

    fireEvent.dragStart(document.querySelector('.card') as HTMLElement);
    expect(onDragStart).toHaveBeenCalledWith(42, expect.anything());
  });
});
