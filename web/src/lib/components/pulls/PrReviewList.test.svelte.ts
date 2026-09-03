import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

const reviewsDismiss = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  reviews: { dismiss: (...a: unknown[]) => reviewsDismiss(...a) },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: (...a: unknown[]) => toastSuccess(...a), error: (...a: unknown[]) => toastError(...a) },
}));

import PrReviewList from './PrReviewList.svelte';
import type { PrReview } from '$lib/types/entities';

let promptMock: ReturnType<typeof vi.fn>;
beforeEach(() => {
  promptMock = vi.fn(() => 'my reason');
  vi.stubGlobal('prompt', promptMock);
  reviewsDismiss.mockReset();
  toastSuccess.mockClear();
  toastError.mockClear();
});
afterEach(() => {
  vi.unstubAllGlobals();
});

function makeReview(overrides: Partial<PrReview> = {}): PrReview {
  return {
    id: 1,
    pr_id: 1,
    repo_id: 1,
    reviewer_id: 9,
    action: 'comment',
    body: null,
    commit_id: null,
    created_at: '2026-05-06T00:00:00Z',
    ...overrides,
  } as PrReview;
}

describe('PrReviewList.svelte', () => {
  it('shows the empty message when there are no reviews', () => {
    render(PrReviewList, { owner: 'o', repo: 'r', prNumber: 1, reviews: [], onDismissed: () => {} });
    expect(screen.getByText('No reviews yet.')).toBeInTheDocument();
  });

  it('renders each review with its action label, body and formatted date', () => {
    const reviews: PrReview[] = [
      makeReview({ id: 1, action: 'approve', body: 'lgtm', commit_id: 'abc1234' }),
      makeReview({ id: 2, action: 'request_changes', body: null }),
    ];
    render(PrReviewList, { owner: 'o', repo: 'r', prNumber: 1, reviews, onDismissed: () => {} });
    expect(screen.getByText('Approved')).toBeInTheDocument();
    expect(screen.getByText('Changes requested')).toBeInTheDocument();
    expect(screen.getByText('lgtm')).toBeInTheDocument();
    expect(screen.getAllByText('fmt(2026-05-06T00:00:00Z)')).toHaveLength(2);
    expect(screen.getByText('abc1234')).toBeInTheDocument();
  });

  it('dismisses an approve review with the prompt reason', async () => {
    reviewsDismiss.mockResolvedValue({});
    const onDismissed = vi.fn();
    const reviews: PrReview[] = [makeReview({ id: 5, action: 'approve' })];
    render(PrReviewList, { owner: 'o', repo: 'r', prNumber: 3, reviews, onDismissed });

    await fireEvent.click(screen.getByText('Dismiss'));
    await waitFor(() => expect(reviewsDismiss).toHaveBeenCalledWith('o', 'r', 3, 5, 'my reason'));
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith('Dismissed'));
    await waitFor(() => expect(onDismissed).toHaveBeenCalledTimes(1));
  });

  it('does not dismiss when the prompt is cancelled', async () => {
    vi.stubGlobal('prompt', () => null);
    const onDismissed = vi.fn();
    const reviews: PrReview[] = [makeReview({ id: 6, action: 'approve' })];
    render(PrReviewList, { owner: 'o', repo: 'r', prNumber: 1, reviews, onDismissed });
    await fireEvent.click(screen.getByText('Dismiss'));
    expect(reviewsDismiss).not.toHaveBeenCalled();
    expect(onDismissed).not.toHaveBeenCalled();
  });

  it('does not show a dismiss button for plain comments', () => {
    const reviews: PrReview[] = [makeReview({ id: 7, action: 'comment' })];
    render(PrReviewList, { owner: 'o', repo: 'r', prNumber: 1, reviews, onDismissed: () => {} });
    expect(screen.queryByText('Dismiss')).toBeNull();
  });
});
