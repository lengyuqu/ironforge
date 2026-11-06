import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { ApiError } from '$lib/api/_base.svelte';

const createQuestion = vi.fn();
const getTask = vi.fn();

vi.mock('$lib/api/collaboration', () => ({
  collaboration: { getTask: (...a: unknown[]) => getTask(...a) },
}));
vi.mock('$lib/api/collabDecisions', () => ({
  collabDecisions: { createQuestion: (...a: unknown[]) => createQuestion(...a) },
}));
vi.mock('$lib/components/toast.svelte', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock('$lib/i18n', () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === 'object' ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

import NewQuestionForm from './NewQuestionForm.svelte';
import type { CollabSession } from '$lib/api/collaboration';

const session = (over: Partial<CollabSession> = {}): CollabSession => ({
  id: 'sess-1', display_name: 'Codex', client_kind: 'codex', state: 'online',
  created_at: '', last_seen_at: '', expires_at: '', ended_at: null, origin: 'user_content', ...over,
});

const onCreated = vi.fn();
function mount(sessions: CollabSession[] = []) {
  render(NewQuestionForm, { props: { owner: 'alice', repo: 'demo', taskId: 't1', taskVersion: 4, sessions, onCreated } });
}
async function open() {
  await fireEvent.click(screen.getByRole('button', { name: 'collab_decisions.question.ask' }));
}
async function fill(title = 'Which DB?', body = 'Pick one.') {
  const [titleBox, bodyBox] = screen.getAllByRole('textbox');
  await fireEvent.input(titleBox, { target: { value: title } });
  await fireEvent.input(bodyBox, { target: { value: body } });
}
const send = () => screen.getByRole('button', { name: 'collab_decisions.question.send' });

describe('NewQuestionForm.svelte', () => {
  beforeEach(() => {
    createQuestion.mockReset().mockResolvedValue({ id: 'q9' });
    getTask.mockReset().mockResolvedValue({ version: 8 });
    onCreated.mockReset();
  });

  it('asks a free-text question of repository writers, bound to the task version', async () => {
    mount();
    await open();
    expect(send()).toBeDisabled();
    await fill();
    expect(send()).not.toBeDisabled();
    await fireEvent.click(send());
    await waitFor(() => expect(createQuestion).toHaveBeenCalledTimes(1));
    const [owner, repo, task, input] = createQuestion.mock.calls[0];
    expect([owner, repo, task]).toEqual(['alice', 'demo', 't1']);
    expect(input).toMatchObject({
      recipient: { kind: 'repo_writers' },
      title: 'Which DB?',
      body: 'Pick one.',
      options: [],
      context: { task_version: 4 },
    });
    expect(input.recipient.id).toBeUndefined();
    expect(input.client_question_id).toBeTruthy();
    await waitFor(() => expect(onCreated).toHaveBeenCalled());
  });

  it('sends non-empty options in order and drops blank ones', async () => {
    mount();
    await open();
    await fill();
    await fireEvent.click(screen.getByRole('button', { name: 'collab_decisions.question.add_option' }));
    await fireEvent.click(screen.getByRole('button', { name: 'collab_decisions.question.add_option' }));
    await fireEvent.click(screen.getByRole('button', { name: 'collab_decisions.question.add_option' }));
    const boxes = screen.getAllByLabelText(/collab_decisions.question.option_n/);
    await fireEvent.input(boxes[0], { target: { value: ' Postgres ' } });
    await fireEvent.input(boxes[2], { target: { value: 'SQLite' } });
    await fireEvent.click(send());
    await waitFor(() => expect(createQuestion).toHaveBeenCalled());
    expect(createQuestion.mock.calls[0][3].options).toEqual(['Postgres', 'SQLite']);
  });

  it('requires a numeric id for a user recipient and a session for a session recipient', async () => {
    mount([session()]);
    await open();
    await fill();
    await fireEvent.change(screen.getByRole('combobox'), { target: { value: 'user' } });
    expect(send()).toBeDisabled();
    const idBox = screen.getAllByRole('textbox')[2];
    await fireEvent.input(idBox, { target: { value: 'abc' } });
    expect(send()).toBeDisabled();
    await fireEvent.input(idBox, { target: { value: '12' } });
    expect(send()).not.toBeDisabled();
    await fireEvent.change(screen.getAllByRole('combobox')[0], { target: { value: 'session' } });
    expect(send()).toBeDisabled();
    await fireEvent.change(screen.getAllByRole('combobox')[1], { target: { value: 'sess-1' } });
    await fireEvent.click(send());
    await waitFor(() => expect(createQuestion).toHaveBeenCalled());
    expect(createQuestion.mock.calls[0][3].recipient).toEqual({ kind: 'session', id: 'sess-1' });
  });

  it('shows the server error, re-reads the task on VERSION_CONFLICT and retries with a new key and version', async () => {
    createQuestion.mockRejectedValueOnce(new ApiError('task moved on', 409, 'VERSION_CONFLICT')).mockResolvedValue({ id: 'q9' });
    mount();
    await open();
    await fill();
    await fireEvent.click(send());
    expect(await screen.findByRole('alert')).toHaveTextContent('task moved on');
    await waitFor(() => expect(getTask).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByText(/"version":8/)).toBeInTheDocument());
    await fireEvent.click(send());
    await waitFor(() => expect(createQuestion).toHaveBeenCalledTimes(2));
    const [a, b] = createQuestion.mock.calls.map((c) => c[3]);
    expect(b.client_question_id).not.toBe(a.client_question_id);
    expect(a.context.task_version).toBe(4);
    expect(b.context.task_version).toBe(8);
  });

  it('shows a permission error as given and keeps the draft', async () => {
    createQuestion.mockRejectedValue(new ApiError('write access required', 403));
    mount();
    await open();
    await fill('Keep me', 'draft');
    await fireEvent.click(send());
    expect(await screen.findByRole('alert')).toHaveTextContent('write access required');
    expect((screen.getAllByRole('textbox')[0] as HTMLInputElement).value).toBe('Keep me');
    expect(onCreated).not.toHaveBeenCalled();
  });

  it('keeps hostile session names and option text inert', async () => {
    const hostile = '<img src=x onerror="window.__pwnedQ=1"><script>window.__pwnedQ=1</script>';
    mount([session({ display_name: hostile })]);
    await open();
    await fireEvent.change(screen.getByRole('combobox'), { target: { value: 'session' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_decisions.question.add_option' }));
    await fireEvent.input(screen.getAllByLabelText(/option_n/)[0], { target: { value: hostile } });
    const root = document.querySelector('[data-testid="new-question"]')!;
    expect(root.querySelector('img')).toBeNull();
    expect(root.querySelector('script')).toBeNull();
    expect(screen.getByRole('option', { name: new RegExp('<img') })).toBeInTheDocument();
    expect((window as unknown as { __pwnedQ?: number }).__pwnedQ).toBeUndefined();
  });

  it('does not offer ended sessions', async () => {
    mount([session({ state: 'ended' })]);
    await open();
    expect(screen.queryByRole('option', { name: 'collab_decisions.question.recipient_session' })).toBeNull();
  });
});
