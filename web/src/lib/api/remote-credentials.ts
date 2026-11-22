import { request } from './_base.svelte';

/**
 * A token for another Git host, keyed by host. The token itself never leaves
 * the server: `token_hint` is at most `****abcd`.
 */
export interface RemoteCredential {
  id: number;
  owner_kind: string;
  owner_id: number;
  host: string;
  username: string | null;
  auth_scheme: string;
  label: string | null;
  token_hint: string | null;
  last_used_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface RemoteCredentialPayload {
  /** Host or full URL; the server normalises it to a lookup key. */
  host: string;
  username?: string;
  token: string;
  /** `basic` (default) or `bearer`. */
  auth_scheme?: string;
  label?: string;
}

/** Changing a credential: the host is the key, so it is not editable. */
export type RemoteCredentialPatch = Partial<Omit<RemoteCredentialPayload, 'host'>>;

export const remoteCredentials = {
  list: () => request<RemoteCredential[]>('/user/remote-credentials'),
  /** Upsert by host — re-submitting a host rotates its token. */
  create: (payload: RemoteCredentialPayload) =>
    request<RemoteCredential>('/user/remote-credentials', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  update: (id: number, payload: RemoteCredentialPatch) =>
    request<RemoteCredential>(`/user/remote-credentials/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    }),
  remove: (id: number) =>
    request<void>(`/user/remote-credentials/${id}`, { method: 'DELETE' }),

  listOrg: (org: string) => request<RemoteCredential[]>(`/orgs/${org}/remote-credentials`),
  createOrg: (org: string, payload: RemoteCredentialPayload) =>
    request<RemoteCredential>(`/orgs/${org}/remote-credentials`, {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  updateOrg: (org: string, id: number, payload: RemoteCredentialPatch) =>
    request<RemoteCredential>(`/orgs/${org}/remote-credentials/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    }),
  removeOrg: (org: string, id: number) =>
    request<void>(`/orgs/${org}/remote-credentials/${id}`, { method: 'DELETE' }),
};
