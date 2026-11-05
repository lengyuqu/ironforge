import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { ApiError } from '$lib/api/_base.svelte';

const listTakeovers = vi.fn();

vi.mock('$lib/api/collabRelations', async (importOriginal) => {
  const orig = await importOriginal<typeof import('$lib/api/collabRelations')>();
  return { ...orig, collabRelations: { listTakeovers: (...a: unknown[]) => listTakeovers(...a) } };
});
vi.mock('$lib/i18n', () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === 'object' ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

import TakeoversCard from './TakeoversCard.svelte';
import type { TakeoverRecord } from '$lib/api/collabRelations';

function rec(over: Partial<TakeoverRecord> = {}): TakeoverRecord {
  return {
    id: 'r1', task_id: 't1', from_session_id: 'aaaaaaaa-1', to_session_id: 'bbbbbbbb-2', actor_kind: 'user', actor_id: '7',
    previous_task_version: 3, new_task_version: 4, outcome: 'reassigned', reason: 'Owner went away',
    created_at: '2026-11-05T10:00:00Z', origin: 'user_content', ...over,
  };
}

function mount(props: Record<string, unknown> = {}) {
  return render(TakeoversCard, { props: { owner: 'alice', repo: 'demo', taskId: 't1', ...props } });
}

describe('TakeoversCard.svelte', () => {
  beforeEach(() => {
    listTakeovers.mockReset().mockResolvedValue({ items: [], next_cursor: null });
    delete (window as unknown as { __pwned?: number }).__pwned;
  });

  it('says so when there is no history', async () => {
    mount();
    expect(await screen.findByTestId('takeovers-none')).toBeInTheDocument();
  });

  it('shows who moved the task, through which door, with the reason and versions', async () => {
    listTakeovers.mockResolvedValue({ items: [rec(), rec({ id: 'r2', from_session_id: null, outcome: 'claimed', actor_kind: 'session', reason: 'Took the abandoned task' })], next_cursor: null });
    mount({ sessionLabel: (id: string | null) => (id ? `label:${id}` : 'unowned') });
    const rows = await screen.findAllByTestId('takeover-row');
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain('label:aaaaaaaa-1');
    expect(rows[0].textContent).toContain('label:bbbbbbbb-2');
    expect(rows[0].textContent).toContain('Owner went away');
    expect(rows[0].textContent).toContain('"kind":"user","id":"7"');
    expect(rows[0].textContent).toContain('"from":3,"to":4');
    expect(rows[1].textContent).toContain('collab_relations.takeovers.no_previous');
    expect(rows[1].textContent).toContain('"kind":"session"');
  });

  it('renders a hostile reason as text, never as markup', async () => {
    const hostile = '<img src=x onerror=window.__pwned=1><script>window.__pwned=1</script>';
    listTakeovers.mockResolvedValue({ items: [rec({ reason: hostile })], next_cursor: null });
    mount();
    expect(await screen.findByText(hostile)).toBeInTheDocument();
    expect(document.querySelector('[data-testid="takeovers-card"] img')).toBeNull();
    expect(document.querySelector('[data-testid="takeovers-card"] script')).toBeNull();
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });

  it('pages with the cursor and appends', async () => {
    listTakeovers
      .mockResolvedValueOnce({ items: [rec()], next_cursor: 'c1' })
      .mockResolvedValueOnce({ items: [rec({ id: 'r2', reason: 'second' })], next_cursor: null });
    mount();
    await fireEvent.click(await screen.findByRole('button', { name: 'collab.load_more' }));
    await waitFor(() => expect(screen.getAllByTestId('takeover-row')).toHaveLength(2));
    expect(listTakeovers.mock.calls[1][3].cursor).toBe('c1');
    expect(screen.queryByRole('button', { name: 'collab.load_more' })).toBeNull();
  });

  it('re-reads when the refresh key changes', async () => {
    const { rerender } = mount({ refreshKey: 1 });
    await waitFor(() => expect(listTakeovers).toHaveBeenCalledTimes(1));
    await rerender({ owner: 'alice', repo: 'demo', taskId: 't1', refreshKey: 2 });
    await waitFor(() => expect(listTakeovers).toHaveBeenCalledTimes(2));
  });

  it('shows the server message when reading fails', async () => {
    listTakeovers.mockRejectedValue(new ApiError('read access denied', 403));
    mount();
    expect(await screen.findByText(/read access denied/)).toBeInTheDocument();
  });
});
