import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import CommentCard from './CommentCard.svelte';
import type { ReactionSummary } from '$lib/api/client.svelte';

function reactionButton(emoji: string): HTMLElement {
  return screen.getByText(emoji).closest('button') as HTMLElement;
}

describe('CommentCard.svelte', () => {
  it('renders header, markdown body and the reaction bar', () => {
    render(CommentCard, {
      author: 'alice',
      createdAt: '2024-01-01T00:00:00Z',
      body: 'hello world',
      reactions: [] as ReactionSummary[],
      onToggleReaction: vi.fn(),
    });
    expect(screen.getByText('issues.commented')).toBeInTheDocument();
    expect(screen.getByText('hello world')).toBeInTheDocument();
    expect(document.querySelectorAll('.reaction-btn')).toHaveLength(8);
  });

  it('forwards a reaction toggle to onToggleReaction', async () => {
    const onToggleReaction = vi.fn();
    render(CommentCard, {
      author: 'alice',
      createdAt: '2024-01-01T00:00:00Z',
      body: 'hi',
      reactions: [] as ReactionSummary[],
      onToggleReaction,
    });
    await fireEvent.click(reactionButton('👍'));
    expect(onToggleReaction).toHaveBeenCalledWith('+1');
  });

  it('reflects count and mine state on the matching reaction', () => {
    const reactions: ReactionSummary[] = [{ content: 'heart', count: 5, reacted_by_me: true }];
    render(CommentCard, {
      author: 'alice',
      createdAt: '2024-01-01T00:00:00Z',
      body: 'hi',
      reactions,
      onToggleReaction: vi.fn(),
    });
    const btn = reactionButton('❤️');
    expect(btn.classList.contains('mine')).toBe(true);
    expect(btn.querySelector('.reaction-count')?.textContent).toBe('5');
  });
});
