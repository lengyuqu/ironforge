// Pure helpers for the SSO provider form: blank state, prefill from an
// existing provider, and conversion to the API payload.
import type { AdminSsoProvider, SsoProviderPayload } from '$lib/api/client.svelte';

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
