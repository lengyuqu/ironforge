import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const listSsoProviders = vi.fn();
const createSsoProvider = vi.fn();
const updateSsoProvider = vi.fn();
const deleteSsoProvider = vi.fn();
const testSsoProvider = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  admin: {
    listSsoProviders: (...a: unknown[]) => listSsoProviders(...a),
    createSsoProvider: (...a: unknown[]) => createSsoProvider(...a),
    updateSsoProvider: (...a: unknown[]) => updateSsoProvider(...a),
    deleteSsoProvider: (...a: unknown[]) => deleteSsoProvider(...a),
    testSsoProvider: (...a: unknown[]) => testSsoProvider(...a),
  },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

import SsoProviderSection from './SsoProviderSection.svelte';
import type { AdminSsoProvider } from '$lib/api/client.svelte';

const oauth: AdminSsoProvider = {
  id: 1,
  name: 'Google',
  slug: 'google',
  provider_type: 'oauth2',
  client_id: 'cid',
  discovery_url: 'https://accounts.google.com',
  scopes: 'openid',
  enabled: true,
} as unknown as AdminSsoProvider;

const ldap: AdminSsoProvider = {
  id: 2,
  name: 'Corp LDAP',
  slug: 'corp',
  provider_type: 'ldap',
  ldap_host: 'ldap.corp',
  ldap_port: 636,
  enabled: false,
} as unknown as AdminSsoProvider;

describe('SsoProviderSection.svelte', () => {
  let confirmMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    confirmMock = vi.fn(() => true);
    vi.stubGlobal('confirm', confirmMock);
    [
      listSsoProviders,
      createSsoProvider,
      updateSsoProvider,
      deleteSsoProvider,
      testSsoProvider,
    ].forEach((m) => m.mockReset());
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders provider rows with Enable/Edit/Delete and Test only for ldap', () => {
    render(SsoProviderSection, { initialProviders: [oauth, ldap] });

    expect(screen.getByText('Google')).toBeInTheDocument();
    expect(screen.getByText('Corp LDAP')).toBeInTheDocument();
    expect(screen.getByText('Enabled')).toBeInTheDocument();
    expect(screen.getByText('Disabled')).toBeInTheDocument();
    // Test connection only for the ldap provider.
    expect(screen.getAllByText('Test connection')).toHaveLength(1);
    expect(screen.getAllByText('Disable')).toHaveLength(1);
    expect(screen.getAllByText('Enable')).toHaveLength(1);
    expect(screen.getByText('Add SSO Provider')).toBeInTheDocument();
  });

  it('prefills the form when editing an existing provider', async () => {
    render(SsoProviderSection, { initialProviders: [oauth] });

    await fireEvent.click(screen.getByText('Edit'));
    expect(screen.getByText('Edit SSO Provider')).toBeInTheDocument();
    expect((screen.getByLabelText('Name') as HTMLInputElement).value).toBe('Google');
    expect((screen.getByLabelText('Slug') as HTMLInputElement).value).toBe('google');
    // Secrets never round-trip from the server.
    expect((screen.getByLabelText('Client Secret') as HTMLInputElement).value).toBe('');
  });

  it('creates a provider with a trimmed payload and reloads the list', async () => {
    createSsoProvider.mockResolvedValue({ id: 3 });
    listSsoProviders.mockResolvedValue([oauth]);
    render(SsoProviderSection, { initialProviders: [] });

    expect(screen.getByText('No SSO providers configured.')).toBeInTheDocument();
    await fireEvent.input(screen.getByLabelText('Name'), { target: { value: '  Okta ' } });
    await fireEvent.input(screen.getByLabelText('Slug'), { target: { value: ' okta ' } });
    await fireEvent.input(screen.getByLabelText('Client ID'), {
      target: { value: ' client-1 ' },
    });
    await fireEvent.click(screen.getByText('Create Provider'));

    await waitFor(() =>
      expect(createSsoProvider).toHaveBeenCalledWith(
        expect.objectContaining({
          name: 'Okta',
          slug: 'okta',
          client_id: 'client-1',
          provider_type: 'oauth2',
          enabled: true,
        })
      )
    );
    await waitFor(() => expect(listSsoProviders).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith('SSO provider created'));
  });

  it('skips delete when the confirm dialog is declined', async () => {
    render(SsoProviderSection, { initialProviders: [oauth] });
    confirmMock.mockReturnValue(false);

    await fireEvent.click(screen.getByText('Delete'));
    expect(deleteSsoProvider).not.toHaveBeenCalled();
  });

  it('runs the ldap connection test and renders the result', async () => {
    testSsoProvider.mockResolvedValue({ ok: true, message: 'LDAP bind OK' });
    render(SsoProviderSection, { initialProviders: [ldap] });

    await fireEvent.click(screen.getByText('Test connection'));
    await waitFor(() => expect(testSsoProvider).toHaveBeenCalledWith(2));
    expect(await screen.findByText('LDAP bind OK')).toBeInTheDocument();
  });
});
