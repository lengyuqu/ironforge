import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const tokensCreate = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  tokens: { create: (...a: unknown[]) => tokensCreate(...a) },
}));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import TokenCreateForm from './TokenCreateForm.svelte';

const writeText = vi.fn();

function renderForm() {
  const onCreated = vi.fn().mockResolvedValue(undefined);
  render(TokenCreateForm, { onCreated });
  return { onCreated };
}

describe('TokenCreateForm.svelte', () => {
  beforeEach(() => {
    tokensCreate.mockReset().mockResolvedValue({ id: 1, name: 'ci', token: 'tok_plain_123' });
    writeText.mockReset().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('keeps the create button disabled until a name is entered', () => {
    renderForm();

    expect(screen.getByText('Create')).toBeDisabled();
    fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'CI deploy token' } });
    expect(screen.getByText('Create')).toBeEnabled();
  });

  it('creates a token with trimmed fields and a parsed expiry, then resets the form', async () => {
    const { onCreated } = renderForm();

    fireEvent.input(screen.getByLabelText('Name'), { target: { value: '  ci  ' } });
    fireEvent.input(screen.getByLabelText('Scopes'), { target: { value: '' } });
    fireEvent.input(screen.getByLabelText('Expires'), { target: { value: '2026-12-31' } });
    await fireEvent.submit(screen.getByText('Create').closest('form')!);

    await waitFor(() => expect(tokensCreate).toHaveBeenCalledTimes(1));
    const [name, scopes, expiresAt] = tokensCreate.mock.calls[0];
    expect(name).toBe('ci');
    // Empty scopes fall back to 'repo'.
    expect(scopes).toBe('repo');
    // Date input → local end-of-day → UTC ISO (timezone-agnostic check:
    // compute the expected instant from the same local wall clock).
    const expected = new Date('2026-12-31T23:59:59').toISOString();
    expect(expiresAt).toBe(expected);

    // The one-time token panel appears and the form resets.
    expect(screen.getByText('tok_plain_123')).toBeInTheDocument();
    expect(screen.getByLabelText('Name')).toHaveValue('');
    await waitFor(() => expect(onCreated).toHaveBeenCalledTimes(1));
  });

  it('copies the one-time token to the clipboard and flips the button label', async () => {
    renderForm();

    fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'ci' } });
    await fireEvent.submit(screen.getByText('Create').closest('form')!);
    await waitFor(() => expect(screen.getByText('tok_plain_123')).toBeInTheDocument());

    await fireEvent.click(screen.getByText('Copy'));
    expect(writeText).toHaveBeenCalledWith('tok_plain_123');
    await waitFor(() => expect(screen.getByText('Copied')).toBeInTheDocument());
  });

  it('blocks empty names with an inline error and no API call', async () => {
    const { onCreated } = renderForm();

    fireEvent.input(screen.getByLabelText('Name'), { target: { value: '   ' } });
    await fireEvent.submit(screen.getByText('Create').closest('form')!);

    expect(screen.getByText('Token name is required')).toBeInTheDocument();
    expect(tokensCreate).not.toHaveBeenCalled();
    expect(onCreated).not.toHaveBeenCalled();
  });

  it('surfaces create failures inline without showing a token', async () => {
    tokensCreate.mockRejectedValueOnce(new Error('quota exceeded'));
    renderForm();

    fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'ci' } });
    await fireEvent.submit(screen.getByText('Create').closest('form')!);

    await waitFor(() => expect(screen.getByText('quota exceeded')).toBeInTheDocument());
    expect(screen.queryByText('tok_plain_123')).not.toBeInTheDocument();
  });
});
