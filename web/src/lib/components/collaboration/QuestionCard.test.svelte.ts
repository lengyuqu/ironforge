import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { ApiError } from '$lib/api/_base.svelte';

const getQuestion = vi.fn();
const answerQuestion = vi.fn();
const cancelQuestion = vi.fn();
const confirmDialog = vi.fn();

vi.mock('$lib/api/collaboration', () => ({
  collaboration: {
    getQuestion: (...a: unknown[]) => getQuestion(...a),
    answerQuestion: (...a: unknown[]) => answerQuestion(...a),
    cancelQuestion: (...a: unknown[]) => cancelQuestion(...a),
  },
}));
vi.mock('$lib/components/confirm.svelte', () => ({ confirmDialog: (...a: unknown[]) => confirmDialog(...a) }));
vi.mock('$lib/components/toast.svelte', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock('$lib/i18n', () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === 'object' ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

import QuestionCard from './QuestionCard.svelte';
import type { CollabQuestion } from '$lib/api/collaboration';

function question(over: Partial<CollabQuestion> = {}): CollabQuestion {
  return {
    id: 'q1',
    task_id: 't1',
    creator_kind: 'session',
    creator_id: 'sess-1',
    recipient: { kind: 'repo_writers', id: null },
    title: 'Ship the migration?',
    body: 'Please decide.',
    options: [],
    context: { task_version: 3, base_sha: null, head_sha: null },
    context_hash: 'hash-1',
    status: 'open',
    version: 2,
    client_question_id: 'cq',
    created_at: '2026-11-04T10:00:00Z',
    updated_at: '2026-11-04T10:00:00Z',
    answer: null,
    context_freshness: 'fresh',
    origin: 'user_content',
    ...over,
  };
}

function mount(userId: number | null = 7) {
  render(QuestionCard, { props: { owner: 'alice', repo: 'demo', questionId: 'q1', userId } });
}

describe('QuestionCard.svelte', () => {
  beforeEach(() => {
    getQuestion.mockReset().mockResolvedValue(question());
    answerQuestion.mockReset();
    cancelQuestion.mockReset();
    confirmDialog.mockReset().mockResolvedValue(true);
  });

  it('renders client-written title and body as inert text, never as markup', async () => {
    const hostile = '<img src=x onerror="window.__pwned=1"><script>window.__pwned=1</script>**bold**';
    getQuestion.mockResolvedValue(question({ title: hostile, body: hostile }));
    mount();
    await waitFor(() => expect(screen.getByText(hostile, { selector: 'strong' })).toBeInTheDocument());
    expect(document.querySelector('[data-testid="question-card"] img')).toBeNull();
    expect(document.querySelector('[data-testid="question-card"] script')).toBeNull();
    expect(document.querySelector('[data-testid="question-card"] b, [data-testid="question-card"] strong strong')).toBeNull();
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });

  it('answers a free-text question with the question version and context hash, no session headers', async () => {
    answerQuestion.mockResolvedValue(question({ status: 'answered', version: 3 }));
    mount();
    const send = await screen.findByRole('button', { name: 'collab.question.send_answer' });
    expect(send).toBeDisabled();
    await fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Yes, ship it' } });
    expect(send).not.toBeDisabled();
    await fireEvent.click(send);
    await waitFor(() => expect(answerQuestion).toHaveBeenCalledTimes(1));
    const [owner, repo, id, input] = answerQuestion.mock.calls[0];
    expect([owner, repo, id]).toEqual(['alice', 'demo', 'q1']);
    expect(input).toMatchObject({
      expected_version: 2,
      body: 'Yes, ship it',
      option_id: null,
      acknowledged_context_hash: 'hash-1',
    });
    expect(input.client_answer_id).toBeTruthy();
  });

  it('requires choosing an option when the question has options', async () => {
    getQuestion.mockResolvedValue(question({ options: [{ id: 'o1', label: 'Ship' }, { id: 'o2', label: 'Wait' }] }));
    answerQuestion.mockResolvedValue(question({ status: 'answered' }));
    mount();
    const send = await screen.findByRole('button', { name: 'collab.question.send_answer' });
    expect(send).toBeDisabled();
    await fireEvent.click(screen.getByLabelText('Wait'));
    expect(send).not.toBeDisabled();
    await fireEvent.click(send);
    await waitFor(() => expect(answerQuestion).toHaveBeenCalled());
    expect(answerQuestion.mock.calls[0][3]).toMatchObject({ option_id: 'o2', body: '' });
  });

  it('offers cancel only for a question this user created', async () => {
    getQuestion.mockResolvedValue(question({ creator_kind: 'user', creator_id: '7' }));
    mount(7);
    await screen.findByRole('button', { name: 'collab.question.cancel' });
  });

  it('does not offer cancel for an agent-created question or another user', async () => {
    mount(7);
    await screen.findByRole('button', { name: 'collab.question.send_answer' });
    expect(screen.queryByRole('button', { name: 'collab.question.cancel' })).toBeNull();
  });

  it('cancels after confirmation using the question version', async () => {
    getQuestion.mockResolvedValue(question({ creator_kind: 'user', creator_id: '7' }));
    cancelQuestion.mockResolvedValue(question({ status: 'cancelled' }));
    mount(7);
    await fireEvent.click(await screen.findByRole('button', { name: 'collab.question.cancel' }));
    await waitFor(() => expect(cancelQuestion).toHaveBeenCalledWith('alice', 'demo', 'q1', 2));
    expect(confirmDialog).toHaveBeenCalled();
  });

  it('does not cancel when the confirmation is declined', async () => {
    getQuestion.mockResolvedValue(question({ creator_kind: 'user', creator_id: '7' }));
    confirmDialog.mockResolvedValue(false);
    mount(7);
    await fireEvent.click(await screen.findByRole('button', { name: 'collab.question.cancel' }));
    await new Promise((r) => setTimeout(r, 20));
    expect(cancelQuestion).not.toHaveBeenCalled();
  });

  it('reloads the question and shows the error when the context went stale', async () => {
    answerQuestion.mockRejectedValue(new ApiError('context changed', 409, 'CONTEXT_STALE'));
    mount();
    await fireEvent.input(await screen.findByRole('textbox'), { target: { value: 'ok' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab.question.send_answer' }));
    await waitFor(() => expect(getQuestion).toHaveBeenCalledTimes(2));
    expect(await screen.findByRole('alert')).toHaveTextContent('context changed');
  });

  it('offers no answer form for a stale question but still lets its creator cancel', async () => {
    getQuestion.mockResolvedValue(question({ context_freshness: 'stale', creator_kind: 'user', creator_id: '7' }));
    mount(7);
    expect(await screen.findByTestId('stale-warning')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'collab.question.send_answer' })).toBeNull();
    expect(screen.getByRole('button', { name: 'collab.question.cancel' })).toBeInTheDocument();
  });

  it('shows a closed question with its answer and no form', async () => {
    getQuestion.mockResolvedValue(
      question({
        status: 'answered',
        answer: {
          id: 'a1', actor_kind: 'user', actor_id: '7', body: 'Go.', option_id: null,
          context_hash: 'hash-1', created_at: '2026-11-04T11:00:00Z', origin: 'user_content',
        },
      }),
    );
    mount();
    expect(await screen.findByTestId('question-answer')).toHaveTextContent('Go.');
    expect(screen.queryByRole('button', { name: 'collab.question.send_answer' })).toBeNull();
  });
});
