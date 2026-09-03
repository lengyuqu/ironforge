import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

import SuggestionBlock from './SuggestionBlock.svelte';
import type { ReviewComment } from '$lib/types/entities';

function makeComment(overrides: Partial<ReviewComment> = {}): ReviewComment {
  return { id: 1, body: 'b', ...overrides } as ReviewComment;
}

describe('SuggestionBlock.svelte', () => {
  it('renders the suggestion code and fires onApply through the apply button', async () => {
    const onApply = vi.fn();
    render(SuggestionBlock, { comment: makeComment({ suggestion: 'const x = 1;' }), onApply });

    expect(screen.getByText('const x = 1;')).toBeInTheDocument();
    const btn = screen.getByText('pulls.suggestion.apply');
    expect(btn).toBeInTheDocument();
    await fireEvent.click(btn);
    expect(onApply).toHaveBeenCalledTimes(1);
  });

  it('shows the delete-range note when the suggestion is an empty string', () => {
    render(SuggestionBlock, { comment: makeComment({ suggestion: '' }) });
    expect(screen.getByText('pulls.suggestion.delete_range')).toBeInTheDocument();
  });

  it('shows the applied label and hides the apply button once applied', () => {
    render(SuggestionBlock, {
      comment: makeComment({ suggestion: 'x', suggestion_applied_at: '2026-01-01T00:00:00Z' }),
    });
    expect(screen.getByText('pulls.suggestion.applied')).toBeInTheDocument();
    expect(screen.queryByText('pulls.suggestion.apply')).toBeNull();
  });

  it('renders a select checkbox and fires onToggleSelect when selectable', async () => {
    const onToggleSelect = vi.fn();
    render(SuggestionBlock, {
      comment: makeComment({ suggestion: 'x' }),
      selectable: true,
      onToggleSelect,
    });
    const checkbox = screen.getByLabelText('pulls.suggestion.select');
    expect(checkbox).toBeInTheDocument();
    await fireEvent.click(checkbox);
    expect(onToggleSelect).toHaveBeenCalledTimes(1);
  });

  it('renders nothing when there is no suggestion', () => {
    render(SuggestionBlock, { comment: makeComment({ suggestion: null }) });
    expect(screen.queryByText('pulls.suggestion.apply')).toBeNull();
    expect(screen.queryByText('pulls.suggestion.delete_range')).toBeNull();
  });
});
