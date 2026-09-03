import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

import ReactionBar from './ReactionBar.svelte';
import type { ReactionSummary } from '$lib/api/client.svelte';

function reactionButton(emoji: string): HTMLElement {
  return screen.getByText(emoji).closest('button') as HTMLElement;
}

describe('ReactionBar.svelte', () => {
  it('renders the full fixed emoji set', () => {
    render(ReactionBar, { rows: [] as ReactionSummary[], onToggle: vi.fn() });
    expect(document.querySelectorAll('.reaction-btn')).toHaveLength(8);
  });

  it('delegates a toggle with the reaction content', async () => {
    const onToggle = vi.fn();
    render(ReactionBar, { rows: [] as ReactionSummary[], onToggle });
    await fireEvent.click(reactionButton('👍'));
    expect(onToggle).toHaveBeenCalledWith('+1');
  });

  it('shows the count and mine state for reacted rows', () => {
    const rows: ReactionSummary[] = [{ content: '+1', count: 3, reacted_by_me: true }];
    render(ReactionBar, { rows, onToggle: vi.fn() });
    const btn = reactionButton('👍');
    expect(btn.classList.contains('mine')).toBe(true);
    expect(btn.querySelector('.reaction-count')?.textContent).toBe('3');
  });
});
