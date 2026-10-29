import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// The app's confirm/prompt dialogs are promise-based UI; route them through the
// stubbed window.confirm / window.prompt so tests keep driving the answer.
vi.mock('$lib/components/confirm.svelte', () => ({
  confirmDialog: async (o: { message: string }) => globalThis.confirm(o.message),
  promptDialog: async (o: { message: string }) => globalThis.prompt(o.message),
}));
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

  describe('validation and save errors', () => {
    it('requires a name and slug before calling the API', async () => {
      render(SsoProviderSection, { initialProviders: [] });

      await fireEvent.click(screen.getByText('Create Provider'));
      expect(toastError).toHaveBeenCalledWith('SSO provider name and slug are required');

      await fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'Okta' } });
      await fireEvent.click(screen.getByText('Create Provider'));
      expect(toastError).toHaveBeenCalledTimes(2);
      expect(createSsoProvider).not.toHaveBeenCalled();
    });

    it('reports a failed save and keeps what was typed', async () => {
      createSsoProvider.mockRejectedValue(new Error('slug taken'));
      render(SsoProviderSection, { initialProviders: [] });
      await fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'Okta' } });
      await fireEvent.input(screen.getByLabelText('Slug'), { target: { value: 'okta' } });
      await fireEvent.click(screen.getByText('Create Provider'));

      await waitFor(() => expect(toastError).toHaveBeenCalledWith('slug taken'));
      expect((screen.getByLabelText('Name') as HTMLInputElement).value).toBe('Okta');
      expect(screen.getByText('Create Provider')).toBeEnabled();
    });

    it('clears the form after a successful create', async () => {
      createSsoProvider.mockResolvedValue({ id: 9 });
      listSsoProviders.mockResolvedValue([]);
      render(SsoProviderSection, { initialProviders: [] });
      await fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'Okta' } });
      await fireEvent.input(screen.getByLabelText('Slug'), { target: { value: 'okta' } });
      await fireEvent.click(screen.getByText('Create Provider'));

      await waitFor(() => expect(toastSuccess).toHaveBeenCalled());
      expect((screen.getByLabelText('Name') as HTMLInputElement).value).toBe('');
      expect((screen.getByLabelText('Scopes') as HTMLInputElement).value).toBe('openid profile email');
    });
  });

  describe('payload shaping', () => {
    async function fillAndCreate(fields: Record<string, string>) {
      createSsoProvider.mockResolvedValue({ id: 9 });
      listSsoProviders.mockResolvedValue([]);
      render(SsoProviderSection, { initialProviders: [] });
      await fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'P' } });
      await fireEvent.input(screen.getByLabelText('Slug'), { target: { value: 'p' } });
      for (const [label, value] of Object.entries(fields)) {
        await fireEvent.input(screen.getByLabelText(label), { target: { value } });
      }
      await fireEvent.click(screen.getByText('Create Provider'));
      await waitFor(() => expect(createSsoProvider).toHaveBeenCalledTimes(1));
      return createSsoProvider.mock.calls[0][0];
    }

    it('turns blank optional fields into undefined', async () => {
      const payload = await fillAndCreate({});
      expect(payload).toMatchObject({
        name: 'P',
        slug: 'p',
        provider_type: 'oauth2',
        scopes: 'openid profile email',
        enabled: true,
      });
      for (const key of [
        'client_id',
        'client_secret',
        'discovery_url',
        'ldap_host',
        'ldap_port',
        'ldap_bind_dn',
        'ldap_bind_password',
        'ldap_base_dn',
        'ldap_user_filter',
        'icon_url',
      ]) {
        expect(payload[key]).toBeUndefined();
      }
    });

    it('keeps secrets verbatim and trims everything else', async () => {
      const payload = await fillAndCreate({
        'Client Secret': ' s3cret ',
        'LDAP Host': ' ldap.corp ',
        'LDAP Port': '636',
        'LDAP Bind Password': ' pw ',
        'Discovery URL': ' https://idp/.well-known ',
      });
      expect(payload.client_secret).toBe(' s3cret ');
      expect(payload.ldap_bind_password).toBe(' pw ');
      expect(payload.ldap_host).toBe('ldap.corp');
      expect(payload.ldap_port).toBe(636);
      expect(payload.discovery_url).toBe('https://idp/.well-known');
    });

    it('lets the type switch to LDAP', async () => {
      createSsoProvider.mockResolvedValue({ id: 9 });
      listSsoProviders.mockResolvedValue([]);
      render(SsoProviderSection, { initialProviders: [] });
      await fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'P' } });
      await fireEvent.input(screen.getByLabelText('Slug'), { target: { value: 'p' } });
      // Select bindings read `:checked`, which happy-dom lacks; answer it here.
      const original = HTMLSelectElement.prototype.querySelector;
      const spy = vi
        .spyOn(HTMLSelectElement.prototype, 'querySelector')
        .mockImplementation(function (this: HTMLSelectElement, selector: string) {
          return selector === ':checked'
            ? (this.options[this.selectedIndex] ?? null)
            : original.call(this, selector);
        });
      try {
        await fireEvent.change(screen.getByLabelText('Type'), { target: { value: 'ldap' } });
      } finally {
        spy.mockRestore();
      }
      await fireEvent.click(screen.getByText('Create Provider'));

      await waitFor(() => expect(createSsoProvider).toHaveBeenCalledTimes(1));
      expect(createSsoProvider.mock.calls[0][0].provider_type).toBe('ldap');
    });
  });

  describe('editing', () => {
    it('saves an edit through update and returns to add mode', async () => {
      updateSsoProvider.mockResolvedValue({});
      listSsoProviders.mockResolvedValue([oauth]);
      render(SsoProviderSection, { initialProviders: [oauth] });

      await fireEvent.click(screen.getByText('Edit'));
      await fireEvent.input(screen.getByLabelText('Name'), { target: { value: 'Google 2' } });
      await fireEvent.click(screen.getByText('Update Provider'));

      await waitFor(() =>
        expect(updateSsoProvider).toHaveBeenCalledWith(
          1,
          expect.objectContaining({ name: 'Google 2', slug: 'google', client_secret: undefined })
        )
      );
      expect(createSsoProvider).not.toHaveBeenCalled();
      await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith('SSO provider updated'));
      expect(screen.getByText('Add SSO Provider')).toBeInTheDocument();
    });

    it('hints that blank secrets are kept only while editing', async () => {
      render(SsoProviderSection, { initialProviders: [ldap] });
      const secret = () => screen.getByLabelText('Client Secret') as HTMLInputElement;
      const bind = () => screen.getByLabelText('LDAP Bind Password') as HTMLInputElement;
      expect(secret().placeholder).toBe('');

      await fireEvent.click(screen.getByText('Edit'));
      expect(secret().placeholder).toBe('Leave blank to keep existing secret');
      expect(bind().placeholder).toBe('Leave blank to keep existing password');
      expect((screen.getByLabelText('LDAP Host') as HTMLInputElement).value).toBe('ldap.corp');
      expect((screen.getByLabelText('LDAP Port') as HTMLInputElement).value).toBe('636');
    });

    it('Cancel discards the edit and restores the empty form', async () => {
      render(SsoProviderSection, { initialProviders: [oauth] });
      await fireEvent.click(screen.getByText('Edit'));
      expect(screen.getByText('Cancel')).toBeInTheDocument();

      await fireEvent.click(screen.getByText('Cancel'));
      expect(screen.getByText('Add SSO Provider')).toBeInTheDocument();
      expect((screen.getByLabelText('Name') as HTMLInputElement).value).toBe('');
      expect(screen.queryByText('Cancel')).toBeNull();
    });
  });

  describe('row actions', () => {
    it('disables an enabled provider via update without sending secrets', async () => {
      updateSsoProvider.mockResolvedValue({});
      listSsoProviders.mockResolvedValue([{ ...oauth, enabled: false }]);
      render(SsoProviderSection, { initialProviders: [oauth] });

      await fireEvent.click(screen.getByText('Disable'));

      await waitFor(() => expect(updateSsoProvider).toHaveBeenCalledTimes(1));
      const [id, payload] = updateSsoProvider.mock.calls[0];
      expect(id).toBe(1);
      expect(payload).toMatchObject({ name: 'Google', slug: 'google', enabled: false, client_id: 'cid' });
      expect(payload).not.toHaveProperty('client_secret');
      expect(payload).not.toHaveProperty('ldap_bind_password');
      expect(await screen.findByText('Enable')).toBeInTheDocument();
    });

    it('enables a disabled ldap provider and keeps its port', async () => {
      updateSsoProvider.mockResolvedValue({});
      listSsoProviders.mockResolvedValue([{ ...ldap, enabled: true }]);
      render(SsoProviderSection, { initialProviders: [ldap] });

      await fireEvent.click(screen.getByText('Enable'));

      await waitFor(() => expect(updateSsoProvider).toHaveBeenCalledTimes(1));
      expect(updateSsoProvider.mock.calls[0][1]).toMatchObject({
        ldap_host: 'ldap.corp',
        ldap_port: 636,
        enabled: true,
      });
    });

    it('toasts when toggling fails', async () => {
      updateSsoProvider.mockRejectedValue(new Error('denied'));
      render(SsoProviderSection, { initialProviders: [oauth] });
      await fireEvent.click(screen.getByText('Disable'));
      await waitFor(() => expect(toastError).toHaveBeenCalledWith('denied'));
    });

    it('deletes after confirmation and reloads', async () => {
      deleteSsoProvider.mockResolvedValue(undefined);
      listSsoProviders.mockResolvedValue([]);
      render(SsoProviderSection, { initialProviders: [oauth] });

      await fireEvent.click(screen.getByText('Delete'));

      await waitFor(() => expect(deleteSsoProvider).toHaveBeenCalledWith(1));
      expect(confirmMock).toHaveBeenCalledTimes(1);
      expect(await screen.findByText('No SSO providers configured.')).toBeInTheDocument();
    });

    it('resets the form when the provider being edited is deleted', async () => {
      deleteSsoProvider.mockResolvedValue(undefined);
      listSsoProviders.mockResolvedValue([]);
      render(SsoProviderSection, { initialProviders: [oauth] });
      await fireEvent.click(screen.getByText('Edit'));
      expect(screen.getByText('Edit SSO Provider')).toBeInTheDocument();

      await fireEvent.click(screen.getByText('Delete'));
      await waitFor(() => expect(screen.getByText('Add SSO Provider')).toBeInTheDocument());
    });

    it('toasts when deleting fails', async () => {
      deleteSsoProvider.mockRejectedValue(new Error('in use'));
      render(SsoProviderSection, { initialProviders: [oauth] });
      await fireEvent.click(screen.getByText('Delete'));
      await waitFor(() => expect(toastError).toHaveBeenCalledWith('in use'));
    });

    it('shows a failed connection test with the API error', async () => {
      testSsoProvider.mockRejectedValue(new Error('timeout'));
      render(SsoProviderSection, { initialProviders: [ldap] });
      await fireEvent.click(screen.getByText('Test connection'));
      expect(await screen.findByText('timeout')).toBeInTheDocument();
      expect(screen.getByText('Test connection')).toBeEnabled();
    });

    it('shows the testing label while a connection test runs', async () => {
      let resolve!: (v: { ok: boolean; message: string }) => void;
      testSsoProvider.mockReturnValue(new Promise((r) => (resolve = r)));
      render(SsoProviderSection, { initialProviders: [ldap] });
      await fireEvent.click(screen.getByText('Test connection'));

      expect(await screen.findByText('Testing...')).toBeDisabled();
      resolve({ ok: true, message: 'done' });
      expect(await screen.findByText('done')).toBeInTheDocument();
    });
  });
});
