// Pure helpers for the SSO provider form: blank state, prefill from an
// existing provider, and conversion to the API payload.
import type {
  AdminSsoProvider,
  LdapGroupSyncPayload,
  SsoProviderPayload,
} from '$lib/api/client.svelte';

export function emptySsoProviderForm(): SsoProviderPayload {
  return {
    name: '',
    slug: '',
    provider_type: 'oauth2',
    client_id: '',
    client_secret: '',
    discovery_url: '',
    scopes: 'openid profile email',
    ldap_host: '',
    ldap_port: undefined,
    ldap_bind_dn: '',
    ldap_bind_password: '',
    ldap_base_dn: '',
    ldap_user_filter: '',
    enabled: true,
    icon_url: '',
  };
}

/** Form state for editing `provider`. Secrets are never sent back by the server. */
export function formFromProvider(provider: AdminSsoProvider): SsoProviderPayload {
  return {
    name: provider.name,
    slug: provider.slug,
    provider_type: provider.provider_type || 'oauth2',
    client_id: provider.client_id || '',
    client_secret: '',
    discovery_url: provider.discovery_url || '',
    scopes: provider.scopes || '',
    ldap_host: provider.ldap_host || '',
    ldap_port: provider.ldap_port ?? undefined,
    ldap_bind_dn: provider.ldap_bind_dn || '',
    ldap_bind_password: '',
    ldap_base_dn: provider.ldap_base_dn || '',
    ldap_user_filter: provider.ldap_user_filter || '',
    enabled: provider.enabled,
    icon_url: provider.icon_url || '',
  };
}

/**
 * API payload for the form: strings are trimmed (secrets are kept verbatim)
 * and blank optionals become undefined so they are omitted.
 */
export function ssoPayloadFromForm(form: SsoProviderPayload): SsoProviderPayload {
  return {
    ...form,
    name: form.name.trim(),
    slug: form.slug.trim(),
    provider_type: form.provider_type || 'oauth2',
    client_id: form.client_id?.trim() || undefined,
    client_secret: form.client_secret || undefined,
    discovery_url: form.discovery_url?.trim() || undefined,
    scopes: form.scopes?.trim() || undefined,
    ldap_host: form.ldap_host?.trim() || undefined,
    ldap_port: form.ldap_port ? Number(form.ldap_port) : undefined,
    ldap_bind_dn: form.ldap_bind_dn?.trim() || undefined,
    ldap_bind_password: form.ldap_bind_password || undefined,
    ldap_base_dn: form.ldap_base_dn?.trim() || undefined,
    ldap_user_filter: form.ldap_user_filter?.trim() || undefined,
    icon_url: form.icon_url?.trim() || undefined,
  };
}

/** Payload that flips `enabled` and leaves everything else (and secrets) as stored. */
export function togglePayload(provider: AdminSsoProvider): SsoProviderPayload {
  return {
    name: provider.name,
    slug: provider.slug,
    provider_type: provider.provider_type,
    client_id: provider.client_id || undefined,
    discovery_url: provider.discovery_url || undefined,
    scopes: provider.scopes || undefined,
    ldap_host: provider.ldap_host || undefined,
    ldap_port: provider.ldap_port || undefined,
    ldap_bind_dn: provider.ldap_bind_dn || undefined,
    ldap_base_dn: provider.ldap_base_dn || undefined,
    ldap_user_filter: provider.ldap_user_filter || undefined,
    icon_url: provider.icon_url || undefined,
    enabled: !provider.enabled,
  };
}

// ── LDAP group -> team sync ──────────────────────────────────────
//
// A second, optional configuration of a provider, written by its own endpoint.
// Kept out of `SsoProviderPayload` so that saving a bind password can never
// clear it.

/** Form state for the group-sync block. `team_org_id` is a text field: empty means unset. */
export interface LdapGroupSyncForm {
  base_dn: string;
  filter: string;
  team_org_id: string;
}

export function emptyLdapGroupSyncForm(): LdapGroupSyncForm {
  return { base_dn: '', filter: '', team_org_id: '' };
}

export function ldapGroupSyncFormFromProvider(provider: AdminSsoProvider): LdapGroupSyncForm {
  return {
    base_dn: provider.ldap_group_base_dn || '',
    filter: provider.ldap_group_filter || '',
    team_org_id: provider.ldap_group_team_org_id ? String(provider.ldap_group_team_org_id) : '',
  };
}

export function isLdapGroupSyncConfigured(form: LdapGroupSyncForm): boolean {
  return Boolean(form.base_dn.trim() || form.filter.trim() || form.team_org_id.trim());
}

/**
 * Validation before the save, mirroring the server's all-or-nothing rule.
 *
 * Returns a message key suffix the caller localises, or `null` when the form is
 * either fully configured or fully empty.
 */
export type LdapGroupSyncError = 'base_dn' | 'filter' | 'org';

export function validateLdapGroupSyncForm(form: LdapGroupSyncForm): LdapGroupSyncError | null {
  if (!isLdapGroupSyncConfigured(form)) return null;
  if (!form.base_dn.trim()) return 'base_dn';
  if (!form.filter.trim()) return 'filter';
  const org = form.team_org_id.trim();
  if (!org) return 'org';
  if (!/^\d+$/.test(org) || Number(org) <= 0) return 'org';
  return null;
}

/** Payload for `setLdapGroupSync`. All three go together; all-empty switches it off. */
export function ldapGroupSyncPayloadFromForm(form: LdapGroupSyncForm): LdapGroupSyncPayload {
  if (!isLdapGroupSyncConfigured(form)) {
    return { base_dn: null, filter: null, team_org_id: null };
  }
  return {
    base_dn: form.base_dn.trim() || null,
    filter: form.filter.trim() || null,
    team_org_id: form.team_org_id.trim() ? Number(form.team_org_id.trim()) : null,
  };
}
