import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const reviewsAddComment = vi.fn();
const reviewsApplySuggestion = vi.fn();
const reviewsSetResolved = vi.fn();
vi.mock('$lib/api/pulls', () => ({
  reviews: {
    addComment: (...a: unknown[]) => reviewsAddComment(...a),
    applySuggestion: (...a: unknown[]) => reviewsApplySuggestion(...a),
    setThreadResolved: (...a: unknown[]) => reviewsSetResolved(...a),
  },
}));

const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: vi.fn(), error: (...a: unknown[]) => toastError(...a) },
}));

import PrDiffView from './PrDiffView.svelte';
import type { PullRequest, ReviewComment } from '$lib/types/entities';
import type { PrDiff } from '$lib/api/pulls';

beforeEach(() => {
  [reviewsAddComment, reviewsApplySuggestion, reviewsSetResolved].forEach((f) => f.mockReset());
  toastError.mockClear();
  reviewsAddComment.mockResolvedValue({});
  reviewsApplySuggestion.mockResolvedValue({ comment: {}, commit_sha: 's' });
  reviewsSetResolved.mockResolvedValue({});
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
    head_sha: 'sha',
    ...overrides,
  } as PullRequest;
}

function makeDiff(): PrDiff {
  return {
    base_branch: 'main',
    head_branch: 'feat',
    files_changed: [
      {
        path: 'src/a.ts',
        status: 'modified',
        additions: 1,
        deletions: 0,
        patch: null,
        lines: [
          { kind: 'meta', content: '@@ -0,0 +1,2 @@', old_line: null, new_line: null },
          { kind: 'context', content: 'const a = 1;', old_line: 1, new_line: 1 },
          { kind: 'addition', content: 'const b = 2;', old_line: null, new_line: 5 },
        ],
      },
    ],
    stats: { total_additions: 1, total_deletions: 0, files_changed: 1 },
  };
}

describe('PrDiffView.svelte', () => {
  it('renders the diff summary and file lines', () => {
    render(PrDiffView, {
      owner: 'o',
      repo: 'r',
      prNumber: 1,
      pr: makePr(),
      diffData: makeDiff(),
      comments: [],
      onChanged: () => {},
    });
    expect(screen.getByText('1 files')).toBeInTheDocument();
    expect(screen.getAllByText('+1')).toHaveLength(2);
    expect(screen.getByText('src/a.ts')).toBeInTheDocument();
    expect(screen.getByText('const b = 2;')).toBeInTheDocument();
  });

  it('shows the no-diff message when diffData is null', () => {
    render(PrDiffView, {
      owner: 'o',
      repo: 'r',
      prNumber: 1,
      pr: makePr(),
      diffData: null,
      comments: [],
      onChanged: () => {},
    });
    expect(screen.getByText('repo.browser.no_diff')).toBeInTheDocument();
  });

  it('submits an inline comment on a RIGHT-side line', async () => {
    const onChanged = vi.fn();
    render(PrDiffView, {
      owner: 'o',
      repo: 'r',
      prNumber: 9,
      pr: makePr(),
      diffData: makeDiff(),
      comments: [],
      onChanged,
    });

    const addButtons = screen.getAllByLabelText('pulls.diff.add_comment');
    // addButtons[0] = context line (new_line 1), addButtons[1] = addition line (new_line 5)
    await fireEvent.click(addButtons[1]);
    await fireEvent.input(screen.getByPlaceholderText('pulls.diff.comment_placeholder'), {
      target: { value: 'nice line' },
    });
    await fireEvent.click(screen.getByText('pulls.diff.submit_comment'));

    await waitFor(() =>
      expect(reviewsAddComment).toHaveBeenCalledWith('o', 'r', 9, {
        path: 'src/a.ts',
        line: 5,
        side: 'RIGHT',
        body: 'nice line',
      })
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('proposes a suggestion with a range when the toggle is enabled', async () => {
    render(PrDiffView, {
      owner: 'o',
      repo: 'r',
      prNumber: 9,
      pr: makePr(),
      diffData: makeDiff(),
      comments: [],
      onChanged: () => {},
    });
    const addButtons = screen.getAllByLabelText('pulls.diff.add_comment');
    await fireEvent.click(addButtons[1]); // RIGHT-side addition line
    expect(screen.getByText('pulls.suggestion.propose')).toBeInTheDocument();
    await fireEvent.click(screen.getByText('pulls.suggestion.propose'));
    await fireEvent.input(screen.getByPlaceholderText('pulls.suggestion.placeholder'), {
      target: { value: 'replacement' },
    });
    await fireEvent.input(screen.getByPlaceholderText('pulls.diff.comment_placeholder'), {
      target: { value: 'see suggestion' },
    });
    await fireEvent.click(screen.getByText('pulls.diff.submit_comment'));

    await waitFor(() =>
      expect(reviewsAddComment).toHaveBeenCalledWith('o', 'r', 9, {
        path: 'src/a.ts',
        line: 5,
        start_line: 5,
        side: 'RIGHT',
        start_side: 'RIGHT',
        body: 'see suggestion',
        suggestion: 'replacement',
      })
    );
  });

  it('resolves an existing thread', async () => {
    const onChanged = vi.fn();
    const comments: ReviewComment[] = [
      {
        id: 1,
        body: 'old comment',
        path: 'src/a.ts',
        line: 5,
        side: 'RIGHT',
        resolved_at: null,
        reply_to_id: null,
      },
    ];
    render(PrDiffView, {
      owner: 'o',
      repo: 'r',
      prNumber: 1,
      pr: makePr(),
      diffData: makeDiff(),
      comments,
      onChanged,
    });

    expect(screen.getByText('old comment')).toBeInTheDocument();
    await fireEvent.click(screen.getByText('pulls.threads.resolve'));
    await waitFor(() => expect(reviewsSetResolved).toHaveBeenCalledWith('o', 'r', 1, 1, true));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('reopens a resolved thread', async () => {
    const onChanged = vi.fn();
    const comments: ReviewComment[] = [
      {
        id: 2,
        body: 'done',
        path: 'src/a.ts',
        line: 5,
        side: 'RIGHT',
        resolved_at: '2026-01-01T00:00:00Z',
        reply_to_id: null,
      },
    ];
    render(PrDiffView, {
      owner: 'o',
      repo: 'r',
      prNumber: 1,
      pr: makePr(),
      diffData: makeDiff(),
      comments,
      onChanged,
    });
    await fireEvent.click(screen.getByText('pulls.threads.reopen'));
    await waitFor(() => expect(reviewsSetResolved).toHaveBeenCalledWith('o', 'r', 1, 2, false));
  });

  it('applies a suggestion from an inline thread', async () => {
    const onChanged = vi.fn();
    const comments: ReviewComment[] = [
      {
        id: 3,
        body: 'try this',
        path: 'src/a.ts',
        line: 5,
        side: 'RIGHT',
        resolved_at: null,
        reply_to_id: null,
        suggestion: 'fix it',
        suggestion_applied_at: null,
      },
    ];
    render(PrDiffView, {
      owner: 'o',
      repo: 'r',
      prNumber: 1,
      pr: makePr(),
      diffData: makeDiff(),
      comments,
      onChanged,
    });
    expect(screen.getByText('fix it')).toBeInTheDocument();
    await fireEvent.click(screen.getByText('pulls.suggestion.apply'));
    await waitFor(() => expect(reviewsApplySuggestion).toHaveBeenCalledWith('o', 'r', 1, 3));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('shows an error toast when submitting a comment fails', async () => {
    reviewsAddComment.mockRejectedValue(new Error('nope'));
    render(PrDiffView, {
      owner: 'o',
      repo: 'r',
      prNumber: 1,
      pr: makePr(),
      diffData: makeDiff(),
      comments: [],
      onChanged: () => {},
    });
    const addButtons = screen.getAllByLabelText('pulls.diff.add_comment');
    await fireEvent.click(addButtons[1]);
    await fireEvent.input(screen.getByPlaceholderText('pulls.diff.comment_placeholder'), {
      target: { value: 'x' },
    });
    await fireEvent.click(screen.getByText('pulls.diff.submit_comment'));
    await waitFor(() => expect(toastError).toHaveBeenCalledWith('nope'));
  });
});
