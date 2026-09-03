import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const pullsMerge = vi.fn();
const pullsEnableAuto = vi.fn();
const pullsDisableAuto = vi.fn();
const pullsEnqueue = vi.fn();
const pullsCancel = vi.fn();
vi.mock('$lib/api/pulls', () => ({
  pulls: {
    merge: (...a: unknown[]) => pullsMerge(...a),
    enableAutoMerge: (...a: unknown[]) => pullsEnableAuto(...a),
    disableAutoMerge: (...a: unknown[]) => pullsDisableAuto(...a),
    enqueueMerge: (...a: unknown[]) => pullsEnqueue(...a),
    cancelQueuedMerge: (...a: unknown[]) => pullsCancel(...a),
  },
}));

const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: vi.fn(), error: (...a: unknown[]) => toastError(...a) },
}));

import PrMergeBox from './PrMergeBox.svelte';
import type { PullRequest } from '$lib/types/entities';
import type { MergeQueueEntry } from '$lib/api/pulls';

beforeEach(() => {
  [pullsMerge, pullsEnableAuto, pullsDisableAuto, pullsEnqueue, pullsCancel].forEach((f) => f.mockReset());
  toastError.mockClear();
  pullsMerge.mockResolvedValue({});
  pullsEnableAuto.mockResolvedValue({ reason: '' });
  pullsDisableAuto.mockResolvedValue({});
  pullsEnqueue.mockResolvedValue({});
  pullsCancel.mockResolvedValue({});
});

function makePr(overrides: Partial<PullRequest> = {}): PullRequest {
  return {
    id: 1,
    number: 1,
    title: 't',
    body: null,
    state: 'open',
    is_draft: false,
    author_id: 1,
    head_branch: 'feat',
    base_branch: 'main',
    ...overrides,
  } as PullRequest;
}

function makeEntry(overrides: Partial<MergeQueueEntry> = {}): MergeQueueEntry {
  return {
    id: 1,
    position: 2,
    pr_id: 1,
    pr_number: 1,
    title: 'x',
    strategy: 'squash',
    status: 'queued',
    enqueued_by_id: 1,
    created_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

describe('PrMergeBox.svelte', () => {
  it('blocks merging for a draft PR', () => {
    render(PrMergeBox, {
      owner: 'o',
      repo: 'r',
      prNumber: 1,
      pr: makePr({ is_draft: true }),
      mergeQueue: [],
      onChanged: () => {},
    });
    expect(screen.getByText('pulls.merge.draft_blocked')).toBeInTheDocument();
    expect(screen.queryByText('pulls.merge.button')).toBeNull();
  });

  it('merges with the default strategy and reloads', async () => {
    const onChanged = vi.fn();
    render(PrMergeBox, { owner: 'o', repo: 'r', prNumber: 5, pr: makePr(), mergeQueue: [], onChanged });
    await fireEvent.click(screen.getByText('pulls.merge.button'));
    await waitFor(() => expect(pullsMerge).toHaveBeenCalledWith('o', 'r', 5, 'merge'));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('enables auto-merge', async () => {
    const onChanged = vi.fn();
    render(PrMergeBox, { owner: 'o', repo: 'r', prNumber: 5, pr: makePr(), mergeQueue: [], onChanged });
    await fireEvent.click(screen.getByText('pulls.merge.enable_auto'));
    await waitFor(() => expect(pullsEnableAuto).toHaveBeenCalledWith('o', 'r', 5, 'merge'));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('enqueues the PR into the merge queue', async () => {
    const onChanged = vi.fn();
    render(PrMergeBox, { owner: 'o', repo: 'r', prNumber: 5, pr: makePr(), mergeQueue: [], onChanged });
    await fireEvent.click(screen.getByText('pulls.merge.join_queue'));
    await waitFor(() => expect(pullsEnqueue).toHaveBeenCalledWith('o', 'r', 5, 'merge'));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('cancels a queued merge', async () => {
    const onChanged = vi.fn();
    render(PrMergeBox, {
      owner: 'o',
      repo: 'r',
      prNumber: 1,
      pr: makePr(),
      mergeQueue: [makeEntry()],
      onChanged,
    });
    await fireEvent.click(screen.getByText('pulls.merge.leave_queue'));
    await waitFor(() => expect(pullsCancel).toHaveBeenCalledWith('o', 'r', 1));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('disables auto-merge when already enabled', async () => {
    const onChanged = vi.fn();
    render(PrMergeBox, {
      owner: 'o',
      repo: 'r',
      prNumber: 1,
      pr: makePr({ auto_merge_enabled: true }),
      mergeQueue: [],
      onChanged,
    });
    expect(screen.getByText('pulls.merge.auto_enabled')).toBeInTheDocument();
    await fireEvent.click(screen.getByText('pulls.merge.disable_auto'));
    await waitFor(() => expect(pullsDisableAuto).toHaveBeenCalledWith('o', 'r', 1));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('shows an error toast when the merge fails', async () => {
    pullsMerge.mockRejectedValue(new Error('conflict'));
    render(PrMergeBox, {
      owner: 'o',
      repo: 'r',
      prNumber: 5,
      pr: makePr(),
      mergeQueue: [],
      onChanged: () => {},
    });
    await fireEvent.click(screen.getByText('pulls.merge.button'));
    await waitFor(() => expect(toastError).toHaveBeenCalledWith('conflict'));
  });
});
