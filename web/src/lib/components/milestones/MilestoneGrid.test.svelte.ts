import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

// The i18n module uses module-level runes in a plain .ts file, which the
// vitest pipeline cannot compile (rune_outside_svelte). Mock it so the
// component under test renders without the reactive locale core.
// Mock the i18n core to keep assertions decoupled from the translation
// files (the module-level runes now compile thanks to i18n.svelte.ts, but
// stable keys beat locale-dependent strings in tests).
vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import MilestoneGrid from './MilestoneGrid.svelte';
import type { Milestone } from '$lib/types/entities';

const baseMilestone: Milestone = {
  id: 1,
  repo_id: 10,
  title: 'v1.0 Launch',
  description: 'First stable release',
  state: 'open',
  due_date: '2026-09-15T00:00:00Z',
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-01T00:00:00Z',
};

describe('MilestoneGrid.svelte', () => {
  it('renders title, state badge and description for each milestone', () => {
    render(MilestoneGrid, {
      items: [
        baseMilestone,
        { ...baseMilestone, id: 2, title: 'v2.0', state: 'closed', description: null },
      ],
      onEdit: () => {},
      onDelete: () => {},
    });

    expect(screen.getByText('v1.0 Launch')).toBeInTheDocument();
    expect(screen.getByText('v2.0')).toBeInTheDocument();
    // state badges render the raw state string, styled by class
    expect(screen.getByText('open')).toBeInTheDocument();
    expect(screen.getByText('closed')).toBeInTheDocument();
    expect(screen.getByText('First stable release')).toBeInTheDocument();
  });

  it('omits the description node when description is null', () => {
    render(MilestoneGrid, {
      items: [{ ...baseMilestone, description: null }],
      onEdit: () => {},
      onDelete: () => {},
    });
    expect(screen.queryByText('First stable release')).not.toBeInTheDocument();
  });

  it('renders the due date through formatDate', () => {
    render(MilestoneGrid, {
      items: [baseMilestone],
      onEdit: () => {},
      onDelete: () => {},
    });
    expect(screen.getByText(/fmt\(2026-09-15T00:00:00Z\)/)).toBeInTheDocument();
  });

  it('delegates edit and delete intents with the milestone payload', () => {
    const onEdit = vi.fn();
    const onDelete = vi.fn();
    render(MilestoneGrid, {
      items: [baseMilestone],
      onEdit,
      onDelete,
    });

    // mocked createT returns keys — the icon buttons expose them via title
    fireEvent.click(screen.getByTitle('settings.edit_milestone'));
    expect(onEdit).toHaveBeenCalledWith(baseMilestone);

    fireEvent.click(screen.getByTitle('settings.delete_milestone'));
    expect(onDelete).toHaveBeenCalledWith(baseMilestone);
  });

  it('renders a progress bar from enriched open/closed counts (M2-2 A2)', () => {
    render(MilestoneGrid, {
      items: [
        { ...baseMilestone, open_issues: 3, closed_issues: 1 },
        { ...baseMilestone, id: 2, title: 'empty', open_issues: 0, closed_issues: 0 },
      ],
      onEdit: () => {},
      onDelete: () => {},
    });

    // One issue closed out of 4 total → 25% fill on the first card.
    const bars = document.querySelectorAll('.progress-fill');
    expect(bars).toHaveLength(2);
    expect((bars[0] as HTMLElement).style.width).toBe('25%');
    // Zero-total milestone keeps the track but never fills.
    expect((bars[1] as HTMLElement).style.width).toBe('0%');
    // Counts render as i18n keys (mocked t returns the key) — one set per card.
    expect(screen.getAllByText('settings.milestone_open_count')).toHaveLength(2);
    expect(screen.getAllByText('settings.milestone_closed_count')).toHaveLength(2);
  });

  it('hides the progress area when counts are absent (bare model)', () => {
    render(MilestoneGrid, {
      items: [baseMilestone],
      onEdit: () => {},
      onDelete: () => {},
    });
    expect(document.querySelector('.progress-track')).not.toBeInTheDocument();
  });
});
