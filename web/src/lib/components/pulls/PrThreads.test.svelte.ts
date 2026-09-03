import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const reviewsSetResolved = vi.fn();
const reviewsApplySuggestion = vi.fn();
const reviewsApplySuggestions = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  reviews: {
    setThreadResolved: (...a: unknown[]) => reviewsSetResolved(...a),
    applySuggestion: (...a: unknown[]) => reviewsApplySuggestion(...a),
    applySuggestions: (...a: unknown[]) => reviewsApplySuggestions(...a),
  },
  // Keep the nested AttachmentPanel from performing real network calls.
  attachments: { list: () => Promise.resolve([]) },
}));

const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: vi.fn(), error: (...a: unknown[]) => toastError(...a) },
}));

import PrThreads from './PrThreads.svelte';
import type { PullRequest, ReviewComment } from '$lib/types/entities';

beforeEach(() => {
  [reviewsSetResolved, reviewsApplySuggestion, reviewsApplySuggestions].forEach((f) => f.mockReset());
  toastError.mockClear();
  reviewsApplySuggestion.mockResolvedValue({ comment: {}, commit_sha: 's' });
  reviewsApplySuggestions.mockResolvedValue({ comments: [], commit_sha: 's' });
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
    head_sha: 'sha1',
    ...overrides,
  } as PullRequest;
}

function makeComment(overrides: Partial<ReviewComment> = {}): ReviewComment {
  return {
    id: 1,
    body: 'root',
    reply_to_id: null,
    path: 'src/a.ts',
    line: 3,
    ...overrides,
  } as ReviewComment;
}

describe('PrThreads.svelte', () => {
  it('renders nothing when there are no root comments', () => {
    render(PrThreads, { owner: 'o', repo: 'r', prNumber: 1, pr: makePr(), comments: [], onChanged: () => {} });
    expect(screen.queryByText('pulls.threads.title')).toBeNull();
  });

  it('renders a root comment with replies and a resolve button', async () => {
    const onChanged = vi.fn();
    const comments: ReviewComment[] = [
      makeComment({ id: 1, body: 'thoughts', path: 'src/a.ts', line: 3, side: 'RIGHT', resolved_at: null }),
      makeComment({ id: 2, body: 'reply', reply_to_id: 1 }),
    ];
    render(PrThreads, { owner: 'o', repo: 'r', prNumber: 1, pr: makePr(), comments, onChanged });
    expect(screen.getByText('thoughts')).toBeInTheDocument();
    expect(screen.getByText('reply')).toBeInTheDocument();
    expect(screen.getByText('pulls.threads.open')).toBeInTheDocument();
    await fireEvent.click(screen.getByText('pulls.threads.resolve'));
    await waitFor(() => expect(reviewsSetResolved).toHaveBeenCalledWith('o', 'r', 1, 1, true));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('reopens a resolved thread', async () => {
    const onChanged = vi.fn();
    const comments: ReviewComment[] = [
      makeComment({ id: 3, body: 'done', resolved_at: '2026-01-01T00:00:00Z' }),
    ];
    render(PrThreads, { owner: 'o', repo: 'r', prNumber: 1, pr: makePr(), comments, onChanged });
    await fireEvent.click(screen.getByText('pulls.threads.reopen'));
    await waitFor(() => expect(reviewsSetResolved).toHaveBeenCalledWith('o', 'r', 1, 3, false));
  });

  it('applies a single applicable suggestion', async () => {
    const onChanged = vi.fn();
    const comments: ReviewComment[] = [
      makeComment({ id: 4, body: 'use this', suggestion: 'new code', suggestion_applied_at: null, commit_id: 'sha1' }),
    ];
    render(PrThreads, { owner: 'o', repo: 'r', prNumber: 1, pr: makePr(), comments, onChanged });
    expect(screen.getByText('new code')).toBeInTheDocument();
    await fireEvent.click(screen.getByText('pulls.suggestion.apply'));
    await waitFor(() => expect(reviewsApplySuggestion).toHaveBeenCalledWith('o', 'r', 1, 4));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('batch-applies several selected suggestions', async () => {
    const onChanged = vi.fn();
    const comments: ReviewComment[] = [
      makeComment({ id: 10, body: 's1', suggestion: 'a', suggestion_applied_at: null, commit_id: 'sha1' }),
      makeComment({ id: 11, body: 's2', suggestion: 'b', suggestion_applied_at: null, commit_id: 'sha1' }),
    ];
    render(PrThreads, { owner: 'o', repo: 'r', prNumber: 1, pr: makePr(), comments, onChanged });
    expect(screen.getByText('pulls.suggestion.apply_selected')).toBeInTheDocument();
    const checkboxes = screen.getAllByLabelText('pulls.suggestion.select');
    expect(checkboxes).toHaveLength(2);
    await fireEvent.click(checkboxes[0]);
    await fireEvent.click(checkboxes[1]);
    const applyBtn = screen.getByText('pulls.suggestion.apply_selected') as HTMLButtonElement;
    expect(applyBtn.disabled).toBe(false);
    await fireEvent.click(applyBtn);
    await waitFor(() => expect(reviewsApplySuggestions).toHaveBeenCalledWith('o', 'r', 1, [10, 11]));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });
});
