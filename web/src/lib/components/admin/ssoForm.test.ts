import { describe, it, expect } from 'vitest';
import {
  emptySsoProviderForm,
  formFromProvider,
  ssoPayloadFromForm,
  togglePayload,
} from './ssoForm';
import type { AdminSsoProvider } from '$lib/api/client.svelte';

const provider = {
  id: 7,
  name: 'Corp',
  slug: 'corp',
  provider_type: 'ldap',
  client_id: null,
  discovery_url: null,
  scopes: null,
  ldap_host: 'ldap.corp',
  ldap_port: 636,
  ldap_bind_dn: 'cn=svc',
  ldap_base_dn: 'dc=corp',
  ldap_user_filter: '(uid={username})',
  icon_url: null,
  enabled: false,
} as unknown as AdminSsoProvider;

describe('emptySsoProviderForm', () => {
  it('defaults to an enabled OAuth2 provider with the standard scopes', () => {
    expect(emptySsoProviderForm()).toMatchObject({
      name: '',
      slug: '',
      provider_type: 'oauth2',
      scopes: 'openid profile email',
      enabled: true,
      ldap_port: undefined,
    });
  });

  it('returns a fresh object every time', () => {
    const a = emptySsoProviderForm();
    a.name = 'changed';
    expect(emptySsoProviderForm().name).toBe('');
  });
});

describe('formFromProvider', () => {
  it('copies stored fields, blanking nulls and never carrying secrets', () => {
    const form = formFromProvider(provider);
    expect(form).toMatchObject({
      name: 'Corp',
      slug: 'corp',
      provider_type: 'ldap',
      client_id: '',
      client_secret: '',
      discovery_url: '',
      scopes: '',
      ldap_host: 'ldap.corp',
      ldap_port: 636,
      ldap_bind_password: '',
      icon_url: '',
      enabled: false,
    });
  });

  it('falls back to oauth2 when the type is missing and drops a null port', () => {
    const form = formFromProvider({ ...provider, provider_type: '', ldap_port: null } as unknown as AdminSsoProvider);
    expect(form.provider_type).toBe('oauth2');
    expect(form.ldap_port).toBeUndefined();
  });
});

describe('ssoPayloadFromForm', () => {
  it('trims text fields but keeps secrets as typed', () => {
    const payload = ssoPayloadFromForm({
      ...emptySsoProviderForm(),
      name: '  A ',
      slug: ' a ',
      client_id: ' id ',
      client_secret: ' sec ',
      ldap_bind_password: ' pw ',
      ldap_host: ' h ',
    });
    expect(payload).toMatchObject({
      name: 'A',
      slug: 'a',
      client_id: 'id',
      client_secret: ' sec ',
      ldap_bind_password: ' pw ',
      ldap_host: 'h',
    });
  });

  it('drops blank optionals and coerces the port to a number', () => {
    const payload = ssoPayloadFromForm({
      ...emptySsoProviderForm(),
      name: 'A',
      slug: 'a',
      ldap_port: '389' as unknown as number,
      client_id: '   ',
    });
    expect(payload.client_id).toBeUndefined();
    expect(payload.client_secret).toBeUndefined();
    expect(payload.ldap_port).toBe(389);
  });

  it('treats a zero or missing port as unset', () => {
    expect(ssoPayloadFromForm({ ...emptySsoProviderForm(), ldap_port: 0 }).ldap_port).toBeUndefined();
  });
});

describe('togglePayload', () => {
  it('flips enabled, omits secrets and blanks', () => {
    const payload = togglePayload(provider);
    expect(payload).toEqual({
      name: 'Corp',
      slug: 'corp',
      provider_type: 'ldap',
      client_id: undefined,
      discovery_url: undefined,
      scopes: undefined,
      ldap_host: 'ldap.corp',
      ldap_port: 636,
      ldap_bind_dn: 'cn=svc',
      ldap_base_dn: 'dc=corp',
      ldap_user_filter: '(uid={username})',
      icon_url: undefined,
      enabled: true,
    });
    expect(payload).not.toHaveProperty('client_secret');
  });

  it('disables an enabled provider', () => {
    expect(togglePayload({ ...provider, enabled: true } as unknown as AdminSsoProvider).enabled).toBe(false);
  });
});
