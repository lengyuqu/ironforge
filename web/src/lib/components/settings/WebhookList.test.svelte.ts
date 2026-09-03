import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const webhookUpdate = vi.fn();
const webhookRemove = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  webhooks: {
    update: (...a: unknown[]) => webhookUpdate(...a),
    remove: (...a: unknown[]) => webhookRemove(...a),
  },
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

import WebhookList from './WebhookList.svelte';
import type { RepositoryWebhook } from '$lib/api/webhooks';

const confirmMock = vi.fn(() => true);

function makeHook(overrides: Partial<RepositoryWebhook> = {}): RepositoryWebhook {
  return {
    id: 1,
    repo_id: 10,
    url: 'https://hooks.example/cb',
    content_type: 'json',
    secret: null,
    active: true,
    events: 'push, issue ,',
    created_at: '2026-09-01T00:00:00Z',
    updated_at: '2026-09-01T00:00:00Z',
    ...overrides,
  };
}

function renderList(overrides: Record<string, unknown> = {}) {
  const onChanged = vi.fn().mockResolvedValue(undefined);
  const props = {
    owner: 'alice',
    repo: 'demo',
    hooks: [makeHook()],
    onChanged,
    ...overrides,
  };
  render(WebhookList, props);
  return { onChanged };
}

describe('WebhookList.svelte', () => {
  beforeEach(() => {
    webhookUpdate.mockReset().mockResolvedValue(undefined);
    webhookRemove.mockReset().mockResolvedValue(undefined);
    toastSuccess.mockClear();
    toastError.mockClear();
    confirmMock.mockClear().mockReturnValue(true);
    vi.stubGlobal('confirm', confirmMock);
    return () => vi.unstubAllGlobals();
  });

  it('renders the empty state when no webhooks exist', () => {
    renderList({ hooks: [] });

    expect(screen.getByText('No webhooks configured yet.')).toBeInTheDocument();
    expect(document.querySelector('.hook-list')).not.toBeInTheDocument();
  });

  it('renders hook url, content type and a cleaned-up event list', () => {
    renderList();

    expect(screen.getByText('https://hooks.example/cb')).toBeInTheDocument();
    expect(screen.getByText('json')).toBeInTheDocument();
    // "push, issue ," → trimmed, empty parts dropped, rejoined with ", ".
    expect(screen.getByText('push, issue')).toBeInTheDocument();
  });

  it('toggles a webhook through the API, toasting success and refreshing', async () => {
    const { onChanged } = renderList();

    const checkbox = document.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    expect(checkbox.checked).toBe(true);

    await fireEvent.change(checkbox, { target: { checked: false } });
    await waitFor(() =>
      expect(webhookUpdate).toHaveBeenCalledWith('alice', 'demo', 1, { active: false })
    );
    expect(toastSuccess).toHaveBeenCalledWith('Webhook updated.');
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
    expect(toastError).not.toHaveBeenCalled();
  });

  it('deletes a webhook after confirmation and refreshes', async () => {
    const { onChanged } = renderList();

    await fireEvent.click(screen.getByText('common.delete'));
    expect(confirmMock).toHaveBeenCalled();
    await waitFor(() => expect(webhookRemove).toHaveBeenCalledWith('alice', 'demo', 1));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
    expect(toastSuccess).toHaveBeenCalledWith('Webhook deleted.');
  });

  it('skips deletion when the user cancels the confirmation', async () => {
    confirmMock.mockReturnValue(false);
    const { onChanged } = renderList();

    await fireEvent.click(screen.getByText('common.delete'));
    expect(webhookRemove).not.toHaveBeenCalled();
    expect(onChanged).not.toHaveBeenCalled();
  });
});
