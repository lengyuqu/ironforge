import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const webhookCreate = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  webhooks: { create: (...a: unknown[]) => webhookCreate(...a) },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: (...a: unknown[]) => toastSuccess(...a), error: (...a: unknown[]) => toastError(...a) },
}));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import WebhookForm from './WebhookForm.svelte';

function renderForm() {
  const onCreated = vi.fn().mockResolvedValue(undefined);
  render(WebhookForm, { owner: 'alice', repo: 'demo', onCreated });
  return { onCreated };
}

describe('WebhookForm.svelte', () => {
  beforeEach(() => {
    webhookCreate.mockReset().mockResolvedValue(undefined);
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  it('renders with push pre-selected and the submit button disabled', () => {
    renderForm();

    const push = screen.getByText('push').closest('label')?.querySelector('input');
    expect(push?.checked).toBe(true);
    const issueOpened = screen.getByText('issue.opened').closest('label')?.querySelector('input');
    expect(issueOpened?.checked).toBe(false);

    // No URL yet → disabled.
    expect(screen.getByText('Add webhook')).toBeDisabled();
  });

  it('toggles events on and off through the checkboxes', async () => {
    renderForm();

    const issueOpened = screen.getByText('issue.opened').closest('label')?.querySelector('input')!;
    await fireEvent.change(issueOpened, { target: { checked: true } });
    expect(issueOpened.checked).toBe(true);

    const push = screen.getByText('push').closest('label')?.querySelector('input')!;
    await fireEvent.change(push, { target: { checked: false } });
    expect(push.checked).toBe(false);
  });

  it('creates a webhook with the entered fields, resets the form and refreshes', async () => {
    const { onCreated } = renderForm();

    fireEvent.input(screen.getByLabelText('Payload URL'), {
      target: { value: '  https://hooks.example/cb  ' },
    });
    fireEvent.input(screen.getByLabelText('Secret'), { target: { value: 's3cret' } });

    await fireEvent.submit(
      screen.getByRole('button', { name: 'Add webhook' }).closest('form')!
    );

    await waitFor(() =>
      expect(webhookCreate).toHaveBeenCalledWith('alice', 'demo', {
        url: 'https://hooks.example/cb',
        content_type: 'json',
        secret: 's3cret',
        active: true,
        events: ['push'],
      })
    );
    expect(toastSuccess).toHaveBeenCalledWith('Webhook created.');
    await waitFor(() => expect(onCreated).toHaveBeenCalledTimes(1));

    // Form resets after a successful create.
    expect(screen.getByLabelText('Payload URL')).toHaveValue('');
    expect(screen.getByLabelText('Secret')).toHaveValue('');
  });

  it('blocks empty URLs with an error toast and no API call', async () => {
    const { onCreated } = renderForm();

    fireEvent.input(screen.getByLabelText('Payload URL'), { target: { value: '' } });
    await fireEvent.submit(
      screen.getByRole('button', { name: 'Add webhook' }).closest('form')!
    );

    expect(toastError).toHaveBeenCalledWith('Enter a payload URL.');
    expect(webhookCreate).not.toHaveBeenCalled();
    expect(onCreated).not.toHaveBeenCalled();
  });

  it('surfaces create failures via an error toast', async () => {
    webhookCreate.mockRejectedValueOnce(new Error('bad host'));
    renderForm();

    fireEvent.input(screen.getByLabelText('Payload URL'), {
      target: { value: 'https://hooks.example/cb' },
    });
    await fireEvent.submit(
      screen.getByRole('button', { name: 'Add webhook' }).closest('form')!
    );

    await waitFor(() => expect(toastError).toHaveBeenCalledWith('bad host'));
  });
});
