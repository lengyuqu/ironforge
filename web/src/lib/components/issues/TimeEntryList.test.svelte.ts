import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const deleteMock = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  timeTracking: { delete: (...a: unknown[]) => deleteMock(...a) },
}));

import TimeEntryList from './TimeEntryList.svelte';
import type { TimeEntry } from '$lib/types/entities';

const entries: TimeEntry[] = [
  { id: 1, issue_id: 1, user_id: 1, duration_minutes: 90, description: 'did work', created_at: '2024-03-10T08:00:00Z' },
  { id: 2, issue_id: 1, user_id: 1, duration_minutes: 45, description: null, created_at: '2024-03-11T09:30:00Z' },
];

let confirmMock: ReturnType<typeof vi.fn>;
beforeEach(() => {
  deleteMock.mockReset();
  deleteMock.mockResolvedValue(undefined);
  confirmMock = vi.fn(() => true);
  vi.stubGlobal('confirm', confirmMock);
});
afterEach(() => {
  vi.unstubAllGlobals();
});

describe('TimeEntryList.svelte', () => {
  it('shows a loading indicator while loading', () => {
    render(TimeEntryList, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      entries: [],
      loading: true,
      currentPage: 1,
      totalPages: 1,
      onPageChange: vi.fn(),
      onRefresh: vi.fn(),
    });
    expect(screen.getByText('Loading…')).toBeInTheDocument();
  });

  it('shows an empty message when there are no entries', () => {
    render(TimeEntryList, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      entries: [],
      loading: false,
      currentPage: 1,
      totalPages: 1,
      onPageChange: vi.fn(),
      onRefresh: vi.fn(),
    });
    expect(screen.getByText('No time entries for this issue yet.')).toBeInTheDocument();
  });

  it('renders entries with formatted durations and pagination', async () => {
    const onPageChange = vi.fn();
    render(TimeEntryList, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      entries,
      loading: false,
      currentPage: 2,
      totalPages: 3,
      onPageChange,
      onRefresh: vi.fn(),
    });

    expect(screen.getByText('1h 30m')).toBeInTheDocument();
    expect(screen.getByText('45m')).toBeInTheDocument();
    expect(screen.getByText('did work')).toBeInTheDocument();
    expect(screen.getByText('—')).toBeInTheDocument();
    expect(screen.getByText('2024-03-10')).toBeInTheDocument();
    expect(screen.getByText('2 / 3')).toBeInTheDocument();

    const next = screen.getByText('Next').closest('button') as HTMLButtonElement;
    expect(next.disabled).toBe(false);
    await fireEvent.click(next);
    expect(onPageChange).toHaveBeenCalledWith(3);
  });

  it('deletes an entry after confirmation', async () => {
    const onRefresh = vi.fn().mockResolvedValue(undefined);
    render(TimeEntryList, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      entries,
      loading: false,
      currentPage: 1,
      totalPages: 1,
      onPageChange: vi.fn(),
      onRefresh,
    });

    const delButtons = screen.getAllByText('Delete');
    await fireEvent.click(delButtons[0]);

    await waitFor(() => expect(deleteMock).toHaveBeenCalledWith('acme', 'web', 7, 1));
    await waitFor(() => expect(onRefresh).toHaveBeenCalledTimes(1));
  });

  it('does not delete when confirmation is dismissed', async () => {
    confirmMock.mockReturnValue(false);
    const onRefresh = vi.fn().mockResolvedValue(undefined);
    render(TimeEntryList, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      entries,
      loading: false,
      currentPage: 1,
      totalPages: 1,
      onPageChange: vi.fn(),
      onRefresh,
    });

    const delButtons = screen.getAllByText('Delete');
    await fireEvent.click(delButtons[0]);

    expect(deleteMock).not.toHaveBeenCalled();
    expect(onRefresh).not.toHaveBeenCalled();
  });
});
