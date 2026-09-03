import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

import IssueSelector from './IssueSelector.svelte';
import type { Issue } from '$lib/types/entities';

const issues: Issue[] = [
  { id: 1, number: 1, title: 'First', state: 'open', author: 'a' },
  { id: 2, number: 2, title: 'Second', state: 'open', author: 'b' },
] as Issue[];

describe('IssueSelector.svelte', () => {
  it('shows a loading indicator while loading', () => {
    render(IssueSelector, { issues: [], loading: true, selectedId: null, onSelect: vi.fn() });
    expect(screen.getByText('Loading…')).toBeInTheDocument();
  });

  it('shows an empty message when there are no issues', () => {
    render(IssueSelector, { issues: [], loading: false, selectedId: null, onSelect: vi.fn() });
    expect(screen.getByText('No open issues.')).toBeInTheDocument();
  });

  it('emits the selected issue and highlights the active row', async () => {
    const onSelect = vi.fn();
    render(IssueSelector, { issues, loading: false, selectedId: 1, onSelect });

    const firstBtn = screen.getByText('First').closest('button') as HTMLElement;
    expect(firstBtn.classList.contains('active')).toBe(true);
    const secondBtn = screen.getByText('Second').closest('button') as HTMLElement;
    expect(secondBtn.classList.contains('active')).toBe(false);

    await fireEvent.click(secondBtn);
    expect(onSelect).toHaveBeenCalledWith(issues[1]);
  });
});
