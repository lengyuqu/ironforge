import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { ApiError } from '$lib/api/_base.svelte';

const listSessions = vi.fn();
const reassignTask = vi.fn();
const toastSuccess = vi.fn();

vi.mock('$lib/api/collaboration', () => ({
  collaboration: { listSessions: (...a: unknown[]) => listSessions(...a) },
}));
vi.mock('$lib/api/collabRelations', async (importOriginal) => {
  const orig = await importOriginal<typeof import('$lib/api/collabRelations')>();
  return { ...orig, collabRelations: { reassignTask: (...a: unknown[]) => reassignTask(...a) } };
});
vi.mock('$lib/components/toast.svelte', () => ({ toast: { success: (...a: unknown[]) => toastSuccess(...a), error: vi.fn() } }));
vi.mock('$lib/i18n', () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === 'object' ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

import ReassignForm from './ReassignForm.svelte';
import type { CollabSession } from '$lib/api/collaboration';

const U1 = '11111111-1111-4111-8111-111111111111';
const U2 = '22222222-2222-4222-8222-222222222222';
const U3 = '33333333-3333-4333-8333-333333333333';
const U4 = '44444444-4444-4444-8444-444444444444';

function session(id: string, name: string, state = 'online'): CollabSession {
  return {
    id, display_name: name, client_kind: 'agent', state, created_at: '', last_seen_at: '', expires_at: '', ended_at: null, origin: 'user_content',
  };
}

function mount(owner: string | null = U1, version = 4, onChanged = vi.fn()) {
  render(ReassignForm, {
    props: { owner: 'alice', repo: 'demo', task: { id: 't1', version, owner_session_id: owner }, onChanged },
  });
  return onChanged;
}

async function open() {
  await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.reassign.open' }));
}

describe('ReassignForm.svelte', () => {
  beforeEach(() => {
    listSessions.mockReset().mockResolvedValue({
      items: [session(U1, 'Current owner'), session(U2, 'Worker B'), session(U3, 'Gone', 'ended'), session(U4, 'Sleeper', 'offline')],
      next_cursor: null,
    });
    reassignTask.mockReset();
    toastSuccess.mockReset();
    delete (window as unknown as { __pwned?: number }).__pwned;
  });

  it('says what a reassign does and does not do before anything is chosen', () => {
    mount();
    expect(screen.getByText('collab_relations.reassign.explain_does')).toBeInTheDocument();
    expect(screen.getByText('collab_relations.reassign.explain_not')).toBeInTheDocument();
    expect(screen.getByText('collab_relations.reassign.who')).toBeInTheDocument();
  });

  it('offers only live sessions other than the current owner', async () => {
    mount();
    await open();
    await waitFor(() => expect(screen.getAllByRole('option').length).toBe(3));
    const text = screen.getAllByRole('option').map((o) => o.textContent ?? '').join('|');
    expect(text).toContain('Worker B');
    expect(text).toContain('Sleeper');
    expect(text).not.toContain('Gone');
    expect(text).not.toContain('Current owner'); // the owner is not a target
  });

  it('explains when listing sessions is forbidden and offers no target', async () => {
    listSessions.mockRejectedValue(new ApiError('write access denied', 403));
    mount();
    await open();
    expect(await screen.findByTestId('sessions-forbidden')).toBeInTheDocument();
    expect(screen.queryByRole('combobox')).toBeNull();
    expect(screen.getByRole('button', { name: 'collab_relations.reassign.submit' })).toBeDisabled();
  });

  it('needs a target and a non-empty reason, then sends the version and owner it saw', async () => {
    reassignTask.mockResolvedValue({ task: {}, takeover: {}, replayed: false });
    const onChanged = mount(U1, 4);
    await open();
    await waitFor(() => expect(screen.getAllByRole('option').length).toBe(3));
    const submit = screen.getByRole('button', { name: 'collab_relations.reassign.submit' });
    expect(submit).toBeDisabled();
    await fireEvent.change(screen.getByRole('combobox'), { target: { value: U2 } });
    expect(submit).toBeDisabled();
    await fireEvent.input(screen.getByRole('textbox'), { target: { value: '  ' } });
    expect(submit).toBeDisabled();
    await fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Owner is stuck' } });
    await fireEvent.click(submit);
    await waitFor(() => expect(reassignTask).toHaveBeenCalledTimes(1));
    const [, , taskId, input] = reassignTask.mock.calls[0];
    expect(taskId).toBe('t1');
    expect(input).toMatchObject({
      expected_task_version: 4,
      expected_owner_session_id: U1,
      to_session_id: U2,
      reason: 'Owner is stuck',
    });
    expect(input.client_takeover_id).toMatch(/\S+/);
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(toastSuccess).toHaveBeenCalledWith('collab_relations.reassign.done');
  });

  it('sends null as the expected owner for an unowned task', async () => {
    reassignTask.mockResolvedValue({ task: {}, takeover: {}, replayed: false });
    mount(null, 1);
    await open();
    await waitFor(() => expect(screen.getAllByRole('option').length).toBe(4));
    await fireEvent.change(screen.getByRole('combobox'), { target: { value: U2 } });
    await fireEvent.input(screen.getByRole('textbox'), { target: { value: 'assign' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.reassign.submit' }));
    await waitFor(() => expect(reassignTask).toHaveBeenCalled());
    expect(reassignTask.mock.calls[0][3].expected_owner_session_id).toBeNull();
  });

  it('uses a NEW key after a failure and shows the server message', async () => {
    reassignTask
      .mockRejectedValueOnce(new ApiError('to_session_id is not a live session of this repository', 400))
      .mockResolvedValueOnce({ task: {}, takeover: {}, replayed: false });
    mount();
    await open();
    await waitFor(() => expect(screen.getAllByRole('option').length).toBe(3));
    await fireEvent.change(screen.getByRole('combobox'), { target: { value: U2 } });
    await fireEvent.input(screen.getByRole('textbox'), { target: { value: 'why' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.reassign.submit' }));
    expect(await screen.findByText(/not a live session/)).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.reassign.submit' }));
    await waitFor(() => expect(reassignTask).toHaveBeenCalledTimes(2));
    expect(reassignTask.mock.calls[1][3].client_takeover_id).not.toBe(reassignTask.mock.calls[0][3].client_takeover_id);
  });

  it('reloads the task on VERSION_CONFLICT', async () => {
    reassignTask.mockRejectedValue(new ApiError('stale', 409, 'VERSION_CONFLICT'));
    const onChanged = mount();
    await open();
    await waitFor(() => expect(screen.getAllByRole('option').length).toBe(3));
    await fireEvent.change(screen.getByRole('combobox'), { target: { value: U2 } });
    await fireEvent.input(screen.getByRole('textbox'), { target: { value: 'why' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.reassign.submit' }));
    expect(await screen.findByText(/collab_relations\.err_reassign\.VERSION_CONFLICT/)).toBeInTheDocument();
    expect(onChanged).toHaveBeenCalled();
  });

  it('keeps the submit disabled for a reason over 2000 bytes', async () => {
    mount();
    await open();
    await waitFor(() => expect(screen.getAllByRole('option').length).toBe(3));
    await fireEvent.change(screen.getByRole('combobox'), { target: { value: U2 } });
    await fireEvent.input(screen.getByRole('textbox'), { target: { value: 'é'.repeat(1001) } });
    expect(screen.getByRole('button', { name: 'collab_relations.reassign.submit' })).toBeDisabled();
    expect(screen.getByText(/reason_too_long/)).toBeInTheDocument();
  });

  it('renders session names as inert text', async () => {
    const hostile = '<img src=x onerror=window.__pwned=1>';
    listSessions.mockResolvedValue({ items: [session(U2, hostile)], next_cursor: null });
    mount();
    await open();
    await waitFor(() => expect(screen.getAllByRole('option').length).toBe(2));
    expect(screen.getAllByRole('option')[1].textContent).toContain(hostile);
    expect(document.querySelector('[data-testid="reassign-form"] img')).toBeNull();
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });
});
