import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const addCommentMock = vi.fn();
const updateMock = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  issues: {
    addComment: (...a: unknown[]) => addCommentMock(...a),
    update: (...a: unknown[]) => updateMock(...a),
  },
}));

import IssueCommentForm from './IssueCommentForm.svelte';

describe('IssueCommentForm.svelte', () => {
  beforeEach(() => {
    addCommentMock.mockReset();
    updateMock.mockReset();
    addCommentMock.mockResolvedValue(undefined);
    updateMock.mockResolvedValue(undefined);
  });

  it('submits a comment and clears the textarea', async () => {
    const onChanged = vi.fn().mockResolvedValue(undefined);
    render(IssueCommentForm, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      issueState: 'open',
      onError: vi.fn(),
      onChanged,
    });

    const textarea = screen.getByPlaceholderText('issues.comment_placeholder') as HTMLTextAreaElement;
    await fireEvent.input(textarea, { target: { value: 'My comment' } });
    await fireEvent.click(screen.getByText('issues.comment'));

    await waitFor(() =>
      expect(addCommentMock).toHaveBeenCalledWith('acme', 'web', 7, 'My comment')
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
    expect(textarea.value).toBe('');
  });

  it('does nothing when the comment is blank (guards on submit)', async () => {
    const onChanged = vi.fn().mockResolvedValue(undefined);
    const { container } = render(IssueCommentForm, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      issueState: 'open',
      onError: vi.fn(),
      onChanged,
    });

    const form = container.querySelector('form') as HTMLFormElement;
    await fireEvent.submit(form);

    expect(addCommentMock).not.toHaveBeenCalled();
    expect(onChanged).not.toHaveBeenCalled();
  });

  it('reports an error and keeps the text when addComment fails', async () => {
    addCommentMock.mockRejectedValue(new Error('boom'));
    const onError = vi.fn();
    const onChanged = vi.fn().mockResolvedValue(undefined);
    render(IssueCommentForm, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      issueState: 'open',
      onError,
      onChanged,
    });

    await fireEvent.input(screen.getByPlaceholderText('issues.comment_placeholder'), {
      target: { value: 'My comment' },
    });
    await fireEvent.click(screen.getByText('issues.comment'));

    await waitFor(() => expect(onError).toHaveBeenCalledWith('boom'));
    expect(onChanged).not.toHaveBeenCalled();
    expect((screen.getByPlaceholderText('issues.comment_placeholder') as HTMLTextAreaElement).value).toBe(
      'My comment'
    );
  });

  it('toggles an open issue to closed', async () => {
    const onChanged = vi.fn().mockResolvedValue(undefined);
    render(IssueCommentForm, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      issueState: 'open',
      onError: vi.fn(),
      onChanged,
    });

    await fireEvent.click(screen.getByText('issues.close_issue'));

    await waitFor(() =>
      expect(updateMock).toHaveBeenCalledWith('acme', 'web', 7, { state: 'closed' })
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('reports an error when the state toggle fails', async () => {
    updateMock.mockRejectedValue(new Error('locked'));
    const onError = vi.fn();
    render(IssueCommentForm, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      issueState: 'open',
      onError,
      onChanged: vi.fn().mockResolvedValue(undefined),
    });

    await fireEvent.click(screen.getByText('issues.close_issue'));

    await waitFor(() => expect(onError).toHaveBeenCalledWith('locked'));
  });
});
