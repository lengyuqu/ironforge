import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const pageStore = vi.hoisted(() => {
  const value = { params: { owner: 'alice', repo: 'demo' } };
  return {
    page: {
      subscribe(fn: (v: typeof value) => void) {
        fn(value);
        return () => {};
      },
    },
  };
});
vi.mock('$app/stores', () => ({ page: pageStore.page }));
const goto = vi.hoisted(() => vi.fn());
vi.mock('$app/navigation', () => ({ goto: (...a: unknown[]) => goto(...a) }));
vi.mock('$lib/components/RepoHeader.svelte', async () => ({ default: (await import('../../../../../test-stubs/EmptyStub.svelte')).default }));
vi.mock('$lib/components/collaboration/CollabNav.svelte', async () => ({ default: (await import('../../../../../test-stubs/EmptyStub.svelte')).default }));
vi.mock('$lib/i18n', () => {
  const t = (key: string) => key;
  return { createT: () => t, t };
});

import Page from './+page.svelte';

describe('coordination groups index', () => {
  beforeEach(() => {
    goto.mockReset();
    localStorage.clear();
  });

  it('says plainly there is no complete list, and shows what this browser remembers as text', async () => {
    const hostile = '<img src=x onerror="window.__pwned=1">';
    localStorage.setItem('ironforge:collab-groups:alice/demo', JSON.stringify([{ id: 'g1', title: hostile }]));
    render(Page);
    expect(await screen.findByText('collab_groups.no_list_note')).toBeInTheDocument();
    expect(screen.getByTestId('remembered-list')).toHaveTextContent(hostile);
    expect(document.querySelector('img')).toBeNull();
    expect(screen.getByRole('link', { name: hostile })).toHaveAttribute('href', '/alice/demo/collaboration/groups/g1');
  });

  it('opens a group by id', async () => {
    render(Page);
    await fireEvent.input(screen.getByLabelText('collab_groups.open.id_label'), { target: { value: ' g 7/x ' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_groups.open.button' }));
    await waitFor(() => expect(goto).toHaveBeenCalledWith('/alice/demo/collaboration/groups/g%207%2Fx'));
  });

  it('forgets a remembered group', async () => {
    localStorage.setItem('ironforge:collab-groups:alice/demo', JSON.stringify([{ id: 'g1', title: 'One' }]));
    render(Page);
    await fireEvent.click(await screen.findByRole('button', { name: 'collab_groups.remembered.forget' }));
    expect(await screen.findByTestId('remembered-empty')).toBeInTheDocument();
  });
});
