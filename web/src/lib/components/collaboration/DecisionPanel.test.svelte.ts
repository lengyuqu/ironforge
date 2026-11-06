import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@testing-library/svelte';
import { ApiError } from '$lib/api/_base.svelte';

const getQuestion = vi.fn();
const getTask = vi.fn();
const createDecision = vi.fn();
const changeDecision = vi.fn();
const confirmDialog = vi.fn();

vi.mock('$lib/api/collaboration', () => ({
  collaboration: {
    getQuestion: (...a: unknown[]) => getQuestion(...a),
    getTask: (...a: unknown[]) => getTask(...a),
  },
}));
vi.mock('$lib/api/collabDecisions', () => ({
  collabDecisions: {
    createDecision: (...a: unknown[]) => createDecision(...a),
    changeDecision: (...a: unknown[]) => changeDecision(...a),
  },
}));
vi.mock('$lib/components/confirm.svelte', () => ({ confirmDialog: (...a: unknown[]) => confirmDialog(...a) }));
vi.mock('$lib/components/toast.svelte', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock('$lib/i18n', () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === 'object' ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

import DecisionPanel from './DecisionPanel.svelte';
import type { CollabDecision, CollabQuestion } from '$lib/api/collaboration';

function decision(over: Partial<CollabDecision> = {}): CollabDecision {
  return {
    id: 'd1',
    task_id: 't1',
    sequence: 1,
    question_id: null,
    answer_id: null,
    title: 'Use SQLite',
    rationale: 'Simple.',
    context: { task_version: 3, base_sha: null, head_sha: null },
    actor_kind: 'user',
    actor_id: '7',
    created_at: '2026-11-04T10:00:00Z',
    status: 'active',
    change: null,
    context_freshness: 'fresh',
    origin: 'user_content',
    ...over,
  };
}

function question(over: Partial<CollabQuestion> = {}): CollabQuestion {
  return {
    id: 'q1', task_id: 't1', creator_kind: 'session', creator_id: 's1',
    recipient: { kind: 'repo_writers', id: null }, title: 'Ship?', body: 'b', options: [],
    context: { task_version: 3, base_sha: null, head_sha: null }, context_hash: 'h', status: 'open',
    version: 1, client_question_id: 'c', created_at: '2026-11-04T10:00:00Z', updated_at: '2026-11-04T10:00:00Z',
    answer: null, context_freshness: null, origin: 'user_content', ...over,
  };
}

const onChanged = vi.fn();
function mount(over: { decisions?: CollabDecision[]; questions?: CollabQuestion[]; taskVersion?: number } = {}) {
  return render(DecisionPanel, {
    props: {
      owner: 'alice', repo: 'demo', taskId: 't1',
      taskVersion: over.taskVersion ?? 3,
      decisions: over.decisions ?? [decision()],
      questions: over.questions ?? [],
      onChanged,
    },
  });
}

async function openForm() {
  await fireEvent.click(screen.getByRole('button', { name: 'collab_decisions.panel.record' }));
}

describe('DecisionPanel.svelte', () => {
  beforeEach(() => {
    for (const m of [getQuestion, getTask, createDecision, changeDecision, onChanged]) m.mockReset();
    confirmDialog.mockReset().mockResolvedValue(true);
    createDecision.mockResolvedValue(decision());
    changeDecision.mockResolvedValue(decision());
    getTask.mockResolvedValue({ version: 9 });
  });

  it('says a decision is a record, not an approval', () => {
    mount();
    expect(screen.getByText('collab_decisions.panel.not_approval')).toBeInTheDocument();
  });

  it('records a decision bound to the task version, without session headers or links', async () => {
    mount();
    await openForm();
    const save = screen.getByRole('button', { name: 'collab_decisions.panel.save' });
    expect(save).toBeDisabled();
    const [titleBox, rationaleBox] = screen.getAllByRole('textbox');
    await fireEvent.input(titleBox, { target: { value: '  Use Postgres  ' } });
    await fireEvent.input(rationaleBox, { target: { value: 'Scale.' } });
    await fireEvent.click(save);
    await waitFor(() => expect(createDecision).toHaveBeenCalledTimes(1));
    const [owner, repo, task, input] = createDecision.mock.calls[0];
    expect([owner, repo, task]).toEqual(['alice', 'demo', 't1']);
    expect(input).toMatchObject({
      title: 'Use Postgres',
      rationale: 'Scale.',
      question_id: null,
      answer_id: null,
      context: { task_version: 3 },
    });
    expect(input.client_decision_id).toBeTruthy();
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it('links an answered question and cites its answer on request', async () => {
    getQuestion.mockResolvedValue(question({ status: 'answered', answer: { id: 'a1', actor_kind: 'user', actor_id: '7', body: 'yes', option_id: null, context_hash: 'h', created_at: '', origin: 'user_content' } }));
    mount({ questions: [question({ status: 'answered' })] });
    await openForm();
    const [titleBox, rationaleBox] = screen.getAllByRole('textbox');
    await fireEvent.input(titleBox, { target: { value: 'T' } });
    await fireEvent.input(rationaleBox, { target: { value: 'R' } });
    await fireEvent.change(screen.getByRole('combobox'), { target: { value: 'q1' } });
    await fireEvent.click(await screen.findByRole('checkbox'));
    await fireEvent.click(screen.getByRole('button', { name: 'collab_decisions.panel.save' }));
    await waitFor(() => expect(createDecision).toHaveBeenCalled());
    expect(createDecision.mock.calls[0][3]).toMatchObject({ question_id: 'q1', answer_id: 'a1' });
  });

  it('shows the server error, uses a NEW key for the retry, and re-reads the version on VERSION_CONFLICT', async () => {
    createDecision.mockRejectedValueOnce(new ApiError('task changed', 409, 'VERSION_CONFLICT')).mockResolvedValue(decision());
    mount();
    await openForm();
    const [titleBox, rationaleBox] = screen.getAllByRole('textbox');
    await fireEvent.input(titleBox, { target: { value: 'T' } });
    await fireEvent.input(rationaleBox, { target: { value: 'R' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_decisions.panel.save' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('task changed');
    await waitFor(() => expect(getTask).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    await fireEvent.click(screen.getByRole('button', { name: 'collab_decisions.panel.save' }));
    await waitFor(() => expect(createDecision).toHaveBeenCalledTimes(2));
    const [first, second] = createDecision.mock.calls.map((c) => c[3]);
    expect(second.client_decision_id).not.toBe(first.client_decision_id);
    expect(first.context.task_version).toBe(3);
    expect(second.context.task_version).toBe(9);
  });

  it('renders hostile client text as inert text', async () => {
    const hostile = '<img src=x onerror="window.__pwnedD=1"><script>window.__pwnedD=1</script>';
    mount({ decisions: [decision({ title: hostile })], questions: [question({ title: hostile })] });
    expect(screen.getByText(hostile, { selector: 'strong' })).toBeInTheDocument();
    await openForm();
    expect(screen.getByRole('option', { name: new RegExp('<img') })).toBeInTheDocument();
    const root = document.querySelector('[data-testid="decision-panel"]')!;
    expect(root.querySelector('img')).toBeNull();
    expect(root.querySelector('script')).toBeNull();
    expect((window as unknown as { __pwnedD?: number }).__pwnedD).toBeUndefined();
  });

  it('revokes only after confirmation, with a reason and no replacement', async () => {
    mount();
    await fireEvent.click(screen.getByRole('button', { name: 'collab_decisions.panel.revoke' }));
    const submit = within(screen.getByTestId('change-form')).getByRole('button', { name: 'collab_decisions.panel.revoke' });
    expect(submit).toBeDisabled();
    await fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Wrong call' } });
    confirmDialog.mockResolvedValueOnce(false);
    await fireEvent.click(submit);
    await new Promise((r) => setTimeout(r, 20));
    expect(changeDecision).not.toHaveBeenCalled();
    await fireEvent.click(submit);
    await waitFor(() => expect(changeDecision).toHaveBeenCalledTimes(1));
    const [, , id, input] = changeDecision.mock.calls[0];
    expect(id).toBe('d1');
    expect(input).toMatchObject({ kind: 'revoked', reason: 'Wrong call', replacement_decision_id: null });
  });

  it('supersedes with a newer active decision only', async () => {
    mount({
      decisions: [
        decision({ id: 'd1', sequence: 1 }),
        decision({ id: 'd2', sequence: 2, title: 'Newer' }),
        decision({ id: 'd3', sequence: 3, title: 'Revoked newer', status: 'revoked' }),
      ],
    });
    await fireEvent.click(screen.getAllByRole('button', { name: 'collab_decisions.panel.supersede' })[0]);
    const select = screen.getByRole('combobox');
    expect(screen.queryByRole('option', { name: /Revoked newer/ })).toBeNull();
    const submit = within(screen.getByTestId('change-form')).getByRole('button', { name: 'collab_decisions.panel.supersede' });
    await fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Replaced' } });
    expect(submit).toBeDisabled();
    await fireEvent.change(select, { target: { value: 'd2' } });
    await fireEvent.click(submit);
    await waitFor(() => expect(changeDecision).toHaveBeenCalled());
    expect(changeDecision.mock.calls[0][3]).toMatchObject({ kind: 'superseded', replacement_decision_id: 'd2', reason: 'Replaced' });
  });

  it('offers no replacement when none is newer, and blocks submitting', async () => {
    mount({ decisions: [decision({ id: 'd1', sequence: 5 }), decision({ id: 'd0', sequence: 1 })] });
    await fireEvent.click(screen.getAllByRole('button', { name: 'collab_decisions.panel.supersede' })[0]);
    expect(screen.getByText('collab_decisions.panel.no_replacement')).toBeInTheDocument();
  });

  it('reloads and closes the form when someone else already changed the decision, and retries with a new key', async () => {
    changeDecision.mockRejectedValueOnce(new ApiError('already changed', 409, 'DECISION_ALREADY_CHANGED'));
    mount();
    await fireEvent.click(screen.getByRole('button', { name: 'collab_decisions.panel.revoke' }));
    await fireEvent.input(screen.getByRole('textbox'), { target: { value: 'x' } });
    await fireEvent.click(within(screen.getByTestId('change-form')).getByRole('button', { name: 'collab_decisions.panel.revoke' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('already changed');
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(screen.queryByTestId('change-form')).toBeNull();
  });

  it('keeps the form after a failed attempt and uses a different key next time', async () => {
    changeDecision.mockRejectedValueOnce(new ApiError('boom', 500)).mockResolvedValue(decision());
    mount();
    await fireEvent.click(screen.getByRole('button', { name: 'collab_decisions.panel.revoke' }));
    await fireEvent.input(screen.getByRole('textbox'), { target: { value: 'x' } });
    const submit = () => within(screen.getByTestId('change-form')).getByRole('button', { name: 'collab_decisions.panel.revoke' });
    await fireEvent.click(submit());
    expect(await screen.findByRole('alert')).toHaveTextContent('boom');
    await fireEvent.click(submit());
    await waitFor(() => expect(changeDecision).toHaveBeenCalledTimes(2));
    expect(changeDecision.mock.calls[1][3].client_change_id).not.toBe(changeDecision.mock.calls[0][3].client_change_id);
  });

  it('marks a stale active decision and does not hide it', () => {
    mount({ decisions: [decision({ context_freshness: 'stale' })] });
    expect(screen.getByText('collab_decisions.panel.stale_note')).toBeInTheDocument();
  });

  it('lists no actions for decisions that are already superseded or revoked', () => {
    mount({ decisions: [decision({ status: 'revoked' })] });
    expect(screen.getByText('collab_decisions.panel.none_active')).toBeInTheDocument();
  });
});
