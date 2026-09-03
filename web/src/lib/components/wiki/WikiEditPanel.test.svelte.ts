import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const wikiUpdate = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  wiki: { update: (...a: unknown[]) => wikiUpdate(...a) },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

import WikiEditPanel from './WikiEditPanel.svelte';

describe('WikiEditPanel.svelte', () => {
  beforeEach(() => {
    wikiUpdate.mockReset();
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  it('saves the edited content through the wiki API', async () => {
    wikiUpdate.mockResolvedValue(undefined);
    const onSaved = vi.fn();
    const onCancel = vi.fn();
    render(WikiEditPanel, {
      owner: 'alice',
      repo: 'demo',
      title: 'Home',
      initialContent: 'old content',
      onSaved,
      onCancel,
    });

    const textarea = screen.getByRole('textbox');
    expect((textarea as HTMLTextAreaElement).value).toBe('old content');
    await fireEvent.input(textarea, { target: { value: 'new content' } });

    await fireEvent.click(screen.getByText('wiki.save'));
    await waitFor(() => expect(wikiUpdate).toHaveBeenCalledWith('alice', 'demo', 'Home', 'new content'));
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    expect(toastSuccess).toHaveBeenCalledTimes(1);
  });

  it('toasts and stays editable when saving fails', async () => {
    wikiUpdate.mockRejectedValue(new Error('locked'));
    const onSaved = vi.fn();
    render(WikiEditPanel, {
      owner: 'alice',
      repo: 'demo',
      title: 'Home',
      initialContent: 'x',
      onSaved,
      onCancel: () => {},
    });

    await fireEvent.click(screen.getByText('wiki.save'));
    await waitFor(() => expect(toastError).toHaveBeenCalledWith('locked'));
    expect(onSaved).not.toHaveBeenCalled();
    expect((screen.getByRole('textbox') as HTMLTextAreaElement).disabled).toBe(false);
  });

  it('cancels without touching the API', async () => {
    const onCancel = vi.fn();
    render(WikiEditPanel, {
      owner: 'alice',
      repo: 'demo',
      title: 'Home',
      initialContent: 'x',
      onSaved: () => {},
      onCancel,
    });

    await fireEvent.click(screen.getByText('wiki.cancel'));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(wikiUpdate).not.toHaveBeenCalled();
  });
});
