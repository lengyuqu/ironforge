import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const reviewsRequested = vi.fn();
const reviewsRequest = vi.fn();
const reviewsRemove = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  reviews: {
    requestedReviewers: (...a: unknown[]) => reviewsRequested(...a),
    requestReviewer: (...a: unknown[]) => reviewsRequest(...a),
    removeRequestedReviewer: (...a: unknown[]) => reviewsRemove(...a),
  },
}));

const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: vi.fn(), error: (...a: unknown[]) => toastError(...a) },
}));

import PrReviewersBox from './PrReviewersBox.svelte';
import type { RequestedReviewer } from '$lib/types/entities';

beforeEach(() => {
  reviewsRequested.mockReset();
  reviewsRequest.mockReset();
  reviewsRemove.mockReset();
  toastError.mockClear();
  reviewsRequested.mockResolvedValue([]);
});

function makeReviewer(overrides: Partial<RequestedReviewer> = {}): RequestedReviewer {
  return {
    id: 1,
    reviewer_id: 2,
    username: 'bob',
    requested_by_id: 3,
    created_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

describe('PrReviewersBox.svelte', () => {
  it('shows a loading message while reviewers are being fetched', async () => {
    reviewsRequested.mockReturnValue(new Promise<RequestedReviewer[]>(() => {}));
    render(PrReviewersBox, { owner: 'o', repo: 'r', prNumber: 1 });
    expect(await screen.findByText('common.loading')).toBeInTheDocument();
  });

  it('shows the empty message when there are no reviewers', async () => {
    reviewsRequested.mockResolvedValue([]);
    render(PrReviewersBox, { owner: 'o', repo: 'r', prNumber: 1 });
    expect(await screen.findByText('pulls.reviewers.empty')).toBeInTheDocument();
  });

  it('renders reviewer chips and removes one on click', async () => {
    reviewsRequested.mockResolvedValue([makeReviewer({ username: 'bob' }), makeReviewer({ id: 2, username: 'sue' })]);
    reviewsRemove.mockResolvedValue(undefined);
    render(PrReviewersBox, { owner: 'o', repo: 'r', prNumber: 4 });

    expect(await screen.findByText('@bob')).toBeInTheDocument();
    expect(screen.getByText('@sue')).toBeInTheDocument();

    await fireEvent.click(screen.getAllByLabelText('pulls.reviewers.remove', { exact: false })[0]);
    await waitFor(() => expect(reviewsRemove).toHaveBeenCalledWith('o', 'r', 4, 'bob'));
  });

  it('requests a new reviewer from the input', async () => {
    reviewsRequested.mockResolvedValue([]);
    reviewsRequest.mockResolvedValue({
      id: 9,
      reviewer_id: 9,
      username: 'carol',
      requested_by_id: 1,
      created_at: '2026-01-01T00:00:00Z',
    });
    render(PrReviewersBox, { owner: 'o', repo: 'r', prNumber: 1 });

    await screen.findByText('pulls.reviewers.empty');
    const input = screen.getByPlaceholderText('pulls.reviewers.placeholder') as HTMLInputElement;
    await fireEvent.input(input, { target: { value: 'carol' } });
    await fireEvent.click(screen.getByText('pulls.reviewers.request'));

    await waitFor(() => expect(reviewsRequest).toHaveBeenCalledWith('o', 'r', 1, 'carol'));
  });
});
