import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { ApiError } from '$lib/api/_base.svelte';

const confirmCondition = vi.fn();
const listSnapshots = vi.fn();
const getSnapshot = vi.fn();
const createSnapshot = vi.fn();

vi.mock('$lib/api/collaboration', () => ({
  collaboration: {
    confirmCondition: (...a: unknown[]) => confirmCondition(...a),
    listSnapshots: (...a: unknown[]) => listSnapshots(...a),
    getSnapshot: (...a: unknown[]) => getSnapshot(...a),
    createSnapshot: (...a: unknown[]) => createSnapshot(...a),
  },
}));
vi.mock('$lib/components/toast.svelte', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock('$lib/i18n', () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === 'object' ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

import ConditionsCard from './ConditionsCard.svelte';
import type { DefinedCondition, DefinedStatus, SnapshotSummary } from '$lib/api/collaboration';

function cond(over: Partial<DefinedCondition> = {}): DefinedCondition {
  return {
    id: 'c1',
    type: 'manual_confirmation',
    label: 'Reviewer looked at it',
    params: {},
    introduced_in_version: 1,
    created_by_kind: 'session',
    created_by_id: 's1',
    status: 'unsatisfied',
    detail: 'No confirmation yet.',
    evidence: [],
    confirmation: null,
    ...over,
  };
}

function defined(conditions: DefinedCondition[], overall = 'unsatisfied'): DefinedStatus {
  return { condition_set_version: 2, conditions, overall };
}

function snap(over: Partial<SnapshotSummary> = {}): SnapshotSummary {
  return {
    id: 'snap-1',
    task_id: 't1',
    condition_set_version: 2,
    task_version: 4,
    overall: 'unsatisfied',
    recorded_by_kind: 'user',
    recorded_by_id: '7',
    created_at: '2026-11-05T10:00:00Z',
    ...over,
  };
}

function mount(d: DefinedStatus | null, onChanged = vi.fn()) {
  render(ConditionsCard, { props: { owner: 'alice', repo: 'demo', taskId: 't1', defined: d, onChanged } });
  return onChanged;
}

describe('ConditionsCard.svelte', () => {
  beforeEach(() => {
    confirmCondition.mockReset();
    listSnapshots.mockReset().mockResolvedValue({ items: [] });
    getSnapshot.mockReset();
    createSnapshot.mockReset();
  });

  it('says so when nothing is defined and offers no confirmation', async () => {
    mount(null);
    expect(await screen.findByTestId('conditions-none')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'collab.conditions.confirm' })).toBeNull();
  });

  it('renders a client-written label as inert text, never as markup', async () => {
    const hostile = '<img src=x onerror="window.__pwned=1"><script>window.__pwned=1</script>';
    mount(defined([cond({ label: hostile })]));
    await screen.findByTestId('defined-condition');
    expect(screen.getByText(hostile)).toBeInTheDocument();
    expect(document.querySelector('[data-testid="conditions-card"] img')).toBeNull();
    expect(document.querySelector('[data-testid="conditions-card"] script')).toBeNull();
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });

  it('confirms a manual condition with a stable key and trimmed note, then refreshes', async () => {
    confirmCondition.mockResolvedValue({ confirmation: {}, replayed: false });
    const onChanged = mount(defined([cond()]));
    await fireEvent.click(await screen.findByRole('button', { name: 'collab.conditions.confirm' }));
    await fireEvent.input(screen.getByRole('textbox'), { target: { value: '  looks good  ' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab.conditions.confirm' }));
    await waitFor(() => expect(confirmCondition).toHaveBeenCalledTimes(1));
    const [owner, repo, task, cid, input] = confirmCondition.mock.calls[0];
    expect([owner, repo, task, cid]).toEqual(['alice', 'demo', 't1', 'c1']);
    expect(input.note).toBe('looks good');
    expect(typeof input.client_request_id).toBe('string');
    expect(input.client_request_id.length).toBeGreaterThan(0);
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it('only manual_confirmation conditions get a confirm button', async () => {
    mount(
      defined([
        cond({ id: 'c1', type: 'manual_confirmation' }),
        cond({ id: 'c2', type: 'ci_passed_for_commit', label: 'CI', params: { commit_sha: 'a'.repeat(40) } }),
        cond({ id: 'c3', type: 'question_answered', label: 'Q', params: { question_id: 'q1' } }),
      ]),
    );
    await screen.findAllByTestId('defined-condition');
    expect(screen.getAllByRole('button', { name: 'collab.conditions.confirm' })).toHaveLength(1);
  });

  it('shows who confirmed, when and at which head; a stale one offers to confirm again', async () => {
    mount(
      defined([
        cond({
          status: 'stale',
          detail: 'The branch moved after the confirmation.',
          confirmation: {
            id: 'k1',
            task_id: 't1',
            condition_id: 'c1',
            condition_set_version: 2,
            task_version: 3,
            head_sha: 'b'.repeat(40),
            note: 'checked on my machine',
            confirmed_by_user_id: 7,
            created_at: '2026-11-05T09:00:00Z',
            origin: 'user_content',
          },
        }),
      ]),
    );
    const info = await screen.findByTestId('confirmation-info');
    expect(info.textContent).toContain('"user":7');
    expect(info.textContent).toContain('bbbbbbbb');
    expect(screen.getByText('checked on my machine')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'collab.conditions.confirm_again' })).toBeInTheDocument();
  });

  it('offers no confirm button once a current confirmation satisfies the condition', async () => {
    mount(
      defined(
        [
          cond({
            status: 'satisfied',
            detail: 'Confirmed at the current branch head.',
            confirmation: {
              id: 'k1',
              task_id: 't1',
              condition_id: 'c1',
              condition_set_version: 2,
              task_version: 4,
              head_sha: null,
              note: '',
              confirmed_by_user_id: 7,
              created_at: '2026-11-05T09:00:00Z',
              origin: 'user_content',
            },
          }),
        ],
        'all_satisfied',
      ),
    );
    await screen.findByTestId('confirmation-info');
    expect(screen.queryByRole('button', { name: /collab\.conditions\.confirm/ })).toBeNull();
  });

  it('keeps the form open and shows the server message when confirming fails, with a fresh key next time', async () => {
    confirmCondition.mockRejectedValue(new ApiError('write access required', 403, 'FORBIDDEN'));
    mount(defined([cond()]));
    await fireEvent.click(await screen.findByRole('button', { name: 'collab.conditions.confirm' }));
    await fireEvent.click(screen.getByRole('button', { name: 'collab.conditions.confirm' }));
    expect(await screen.findByText(/write access required/)).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'collab.conditions.confirm' }));
    await waitFor(() => expect(confirmCondition).toHaveBeenCalledTimes(2));
    expect(confirmCondition.mock.calls[0][4].client_request_id).not.toBe(
      confirmCondition.mock.calls[1][4].client_request_id,
    );
  });

  it('lists snapshots and shows the stored evaluation, not a recomputed one', async () => {
    listSnapshots.mockResolvedValue({ items: [snap()] });
    getSnapshot.mockResolvedValue({
      ...snap(),
      immutable: true,
      replayed: false,
      result: {
        task_id: 't1',
        task_version: 4,
        declared_status: 'done',
        conditions: [],
        overall: 'unsatisfied',
        declared_done_without_evidence: true,
        observed_branch_head: 'c'.repeat(40),
        evaluated_at: '2026-11-05T10:00:00Z',
        defined: defined([cond({ label: 'As it was then' })]),
      },
    });
    mount(null);
    await screen.findByTestId('snapshot-row');
    await fireEvent.click(screen.getByRole('button', { name: 'collab.show' }));
    const stored = await screen.findByTestId('snapshot-stored');
    expect(stored.textContent).toContain('As it was then');
    expect(stored.textContent).toContain('collab.completion.done_without_evidence');
    expect(stored.textContent).toContain('cccccccc');
    expect(getSnapshot).toHaveBeenCalledWith('alice', 'demo', 't1', 'snap-1');
  });

  it('records a snapshot with an idempotency key and reloads the list', async () => {
    createSnapshot.mockResolvedValue({ ...snap({ id: 'snap-2' }), immutable: true, replayed: false, result: {} });
    mount(null);
    await waitFor(() => expect(listSnapshots).toHaveBeenCalledTimes(1));
    await fireEvent.click(screen.getByRole('button', { name: 'collab.conditions.record' }));
    await waitFor(() => expect(createSnapshot).toHaveBeenCalledTimes(1));
    expect(createSnapshot.mock.calls[0].slice(0, 3)).toEqual(['alice', 'demo', 't1']);
    expect(typeof createSnapshot.mock.calls[0][3]).toBe('string');
    await waitFor(() => expect(listSnapshots).toHaveBeenCalledTimes(2));
  });
});
