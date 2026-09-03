import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const createMock = vi.fn();
const milestonesListMock = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  issues: { create: (...a: unknown[]) => createMock(...a) },
  milestones: { list: (...a: unknown[]) => milestonesListMock(...a) },
}));

import IssueCreateForm from './IssueCreateForm.svelte';

describe('IssueCreateForm.svelte', () => {
  beforeEach(() => {
    createMock.mockReset();
    milestonesListMock.mockReset();
    createMock.mockResolvedValue(undefined);
    milestonesListMock.mockResolvedValue([]);
  });

  it('blocks submission and shows an error when the title is empty', () => {
    render(IssueCreateForm, {
      owner: 'acme',
      repo: 'web',
      onCreated: vi.fn(),
      onCancel: vi.fn(),
    });

    expect(screen.getByText('issues.create_form.title_required')).toBeInTheDocument();
    const submit = screen.getByText('issues.create_form.submit').closest('button') as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
  });

  it('submits trimmed labels and assignees and clears on success', async () => {
    const onCreated = vi.fn().mockResolvedValue(undefined);
    render(IssueCreateForm, {
      owner: 'acme',
      repo: 'web',
      onCreated,
      onCancel: vi.fn(),
    });

    await fireEvent.input(screen.getByPlaceholderText('issues.create_form.title_placeholder'), {
      target: { value: 'Bug report' },
    });
    await fireEvent.input(screen.getByPlaceholderText('issues.create_form.labels_placeholder'), {
      target: { value: ' x , y ' },
    });
    await fireEvent.input(screen.getByPlaceholderText('issues.create_form_assignees_placeholder'), {
      target: { value: ' u ' },
    });
    await fireEvent.click(screen.getByText('issues.create_form.submit'));

    await waitFor(() =>
      expect(createMock).toHaveBeenCalledWith('acme', 'web', 'Bug report', undefined, ['x', 'y'], ['u'], undefined)
    );
    await waitFor(() => expect(onCreated).toHaveBeenCalledTimes(1));
  });

  it('shows a form error and does not call onCreated when create fails', async () => {
    createMock.mockRejectedValue(new Error('boom'));
    const onCreated = vi.fn().mockResolvedValue(undefined);
    render(IssueCreateForm, {
      owner: 'acme',
      repo: 'web',
      onCreated,
      onCancel: vi.fn(),
    });

    await fireEvent.input(screen.getByPlaceholderText('issues.create_form.title_placeholder'), {
      target: { value: 'Bug report' },
    });
    await fireEvent.click(screen.getByText('issues.create_form.submit'));

    await waitFor(() => expect(screen.getByText('boom')).toBeInTheDocument());
    expect(onCreated).not.toHaveBeenCalled();
  });

  it('invokes onCancel when the cancel button is clicked', async () => {
    const onCancel = vi.fn();
    render(IssueCreateForm, {
      owner: 'acme',
      repo: 'web',
      onCreated: vi.fn(),
      onCancel,
    });

    await fireEvent.click(screen.getByText('issues.create_form.cancel'));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it('still requires a title when it is only whitespace', async () => {
    render(IssueCreateForm, {
      owner: 'acme',
      repo: 'web',
      onCreated: vi.fn(),
      onCancel: vi.fn(),
    });

    await fireEvent.input(screen.getByPlaceholderText('issues.create_form.title_placeholder'), {
      target: { value: '   ' },
    });
    expect(screen.getByText('issues.create_form.title_required')).toBeInTheDocument();
    const submit = screen.getByText('issues.create_form.submit').closest('button') as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
  });
});
