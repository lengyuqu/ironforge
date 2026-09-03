import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import PrHeader from './PrHeader.svelte';
import type { PullRequest } from '$lib/types/entities';

function makePr(overrides: Partial<PullRequest> = {}): PullRequest {
  return {
    id: 1,
    number: 1,
    title: 'Add feature',
    body: null,
    state: 'open',
    is_draft: false,
    author_id: 5,
    author: 'alice',
    head_branch: 'feat',
    base_branch: 'main',
    created_at: '2026-01-02T00:00:00Z',
    ...overrides,
  } as PullRequest;
}

describe('PrHeader.svelte', () => {
  it('renders title, state badge and branch pair for an open PR', () => {
    render(PrHeader, { pr: makePr(), updatingDraft: false, onToggleDraft: () => {} });
    expect(screen.getByText('Add feature')).toBeInTheDocument();
    expect(screen.getByText('pulls.state.open')).toBeInTheDocument();
    expect(screen.getByText('feat')).toBeInTheDocument();
    expect(screen.getByText('main')).toBeInTheDocument();
    expect(screen.getByText(/opened fmt\(2026-01-02T00:00:00Z\) by/)).toBeInTheDocument();
    expect(screen.getByText('alice')).toBeInTheDocument();
  });

  it('shows the draft badge and the mark-ready button for a draft PR', () => {
    render(PrHeader, { pr: makePr({ is_draft: true }), updatingDraft: false, onToggleDraft: () => {} });
    expect(screen.getByText('pulls.draft')).toBeInTheDocument();
    expect(screen.getByText('pulls.mark_ready')).toBeInTheDocument();
  });

  it('shows the convert-to-draft button for an open non-draft PR', () => {
    render(PrHeader, { pr: makePr({ is_draft: false }), updatingDraft: false, onToggleDraft: () => {} });
    expect(screen.getByText('pulls.convert_draft')).toBeInTheDocument();
  });

  it('fires onToggleDraft when the draft toggle is clicked', async () => {
    const onToggleDraft = vi.fn();
    render(PrHeader, { pr: makePr({ is_draft: false }), updatingDraft: false, onToggleDraft });
    await fireEvent.click(screen.getByText('pulls.convert_draft'));
    expect(onToggleDraft).toHaveBeenCalledTimes(1);
  });

  it('renders the PR body section when body is present', () => {
    render(PrHeader, { pr: makePr({ body: 'Hello world' }), updatingDraft: false, onToggleDraft: () => {} });
    expect(screen.getByText('Hello world')).toBeInTheDocument();
  });

  it('does not render a draft toggle for a closed PR', () => {
    render(PrHeader, { pr: makePr({ state: 'closed' }), updatingDraft: false, onToggleDraft: () => {} });
    expect(screen.queryByText('pulls.convert_draft')).toBeNull();
    expect(screen.queryByText('pulls.mark_ready')).toBeNull();
  });
});
