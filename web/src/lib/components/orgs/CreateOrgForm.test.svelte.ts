import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const orgsCreate = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  orgs: { create: (...a: unknown[]) => orgsCreate(...a) },
}));

import CreateOrgForm from './CreateOrgForm.svelte';

describe('CreateOrgForm.svelte', () => {
  beforeEach(() => {
    orgsCreate.mockReset();
  });

  it('creates an org passing only filled optional fields', async () => {
    const created = { id: 1, name: 'acme' };
    orgsCreate.mockResolvedValue(created);
    const onCreated = vi.fn();
    render(CreateOrgForm, { onCreated });

    await fireEvent.input(screen.getByLabelText(/orgs\.name/), { target: { value: 'acme' } });
    await fireEvent.input(screen.getByLabelText(/orgs\.display_name/), {
      target: { value: 'Acme Inc' },
    });
    await fireEvent.submit(document.querySelector('form')!);

    await waitFor(() =>
      expect(orgsCreate).toHaveBeenCalledWith('acme', 'Acme Inc', undefined, 'public')
    );
    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(created));
    // The form resets after success.
    expect((screen.getByLabelText(/orgs\.name/) as HTMLInputElement).value).toBe('');
  });

  it('shows an inline error when the API rejects', async () => {
    orgsCreate.mockRejectedValue(new Error('name taken'));
    render(CreateOrgForm, { onCreated: () => {} });

    await fireEvent.input(screen.getByLabelText(/orgs\.name/), { target: { value: 'acme' } });
    await fireEvent.submit(document.querySelector('form')!);

    expect(await screen.findByText('name taken')).toBeInTheDocument();
  });
});
