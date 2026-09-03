import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const reviewsSubmit = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  reviews: { submit: (...a: unknown[]) => reviewsSubmit(...a) },
}));

const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: vi.fn(), error: (...a: unknown[]) => toastError(...a) },
}));

import PrReviewForm from './PrReviewForm.svelte';

describe('PrReviewForm.svelte', () => {
  beforeEach(() => {
    reviewsSubmit.mockReset();
    toastError.mockClear();
  });

  it('renders the title and the three verdict radios', () => {
    render(PrReviewForm, { owner: 'o', repo: 'r', prNumber: 1, onSubmitted: () => {} });
    expect(screen.getByText('pulls.review.title')).toBeInTheDocument();
    expect(screen.getByText('pulls.review.verdict_comment')).toBeInTheDocument();
    expect(screen.getByText('pulls.review.verdict_approve')).toBeInTheDocument();
    expect(screen.getByText('pulls.review.verdict_changes')).toBeInTheDocument();
  });

  it('disables submit while the comment body is empty', () => {
    render(PrReviewForm, { owner: 'o', repo: 'r', prNumber: 1, onSubmitted: () => {} });
    expect((screen.getByText('pulls.review.submit') as HTMLButtonElement).disabled).toBe(true);
  });

  it('submits a review with the selected approve verdict', async () => {
    reviewsSubmit.mockResolvedValue({});
    const onSubmitted = vi.fn();
    render(PrReviewForm, { owner: 'o', repo: 'r', prNumber: 7, onSubmitted });

    await fireEvent.input(screen.getByPlaceholderText('pulls.review.placeholder'), {
      target: { value: 'Looks good' },
    });
    await fireEvent.click(screen.getByLabelText('pulls.review.verdict_approve'));
    await fireEvent.click(screen.getByText('pulls.review.submit'));

    await waitFor(() => expect(reviewsSubmit).toHaveBeenCalledWith('o', 'r', 7, 'Looks good', 'approve'));
    await waitFor(() => expect(onSubmitted).toHaveBeenCalledTimes(1));
  });

  it('defaults to the comment verdict when none is picked', async () => {
    reviewsSubmit.mockResolvedValue({});
    const onSubmitted = vi.fn();
    render(PrReviewForm, { owner: 'o', repo: 'r', prNumber: 2, onSubmitted });
    await fireEvent.input(screen.getByPlaceholderText('pulls.review.placeholder'), {
      target: { value: 'note' },
    });
    await fireEvent.click(screen.getByText('pulls.review.submit'));
    await waitFor(() => expect(reviewsSubmit).toHaveBeenCalledWith('o', 'r', 2, 'note', 'comment'));
  });

  it('shows an error toast when the submit fails', async () => {
    reviewsSubmit.mockRejectedValue(new Error('boom'));
    render(PrReviewForm, { owner: 'o', repo: 'r', prNumber: 3, onSubmitted: () => {} });
    await fireEvent.input(screen.getByPlaceholderText('pulls.review.placeholder'), {
      target: { value: 'x' },
    });
    await fireEvent.click(screen.getByText('pulls.review.submit'));
    await waitFor(() => expect(toastError).toHaveBeenCalledWith('boom'));
  });
});
