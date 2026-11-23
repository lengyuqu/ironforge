import { request } from './_base.svelte';

/**
 * A push mirror forwards this repository's branches (and optionally tags) to an
 * external Git host after every push. The target's token is not stored here —
 * it comes from the owner's remote credential row for that host.
 */
export interface PushMirror {
  id: number;
  repo_id: number;
  url: string;
  push_tags: boolean;
  /** `on_push` or `manual`. */
  trigger_mode: string;
  /** `active` | `paused` | `error`. */
  status: string;
  last_push_at: string | null;
  last_push_error: string | null;
  created_at: string;
  updated_at: string;
}

export interface PushMirrorPayload {
  url?: string;
  push_tags?: boolean;
  trigger_mode?: string;
  /** `active` or `paused` — patch only. */
  status?: string;
}

export const pushMirrors = {
  get: (owner: string, repo: string) =>
    request<PushMirror>(`/repos/${owner}/${repo}/push-mirror`),
  /** Upsert: reconfiguring an existing target replaces and re-activates it. */
  create: (owner: string, repo: string, payload: PushMirrorPayload) =>
    request<PushMirror>(`/repos/${owner}/${repo}/push-mirror`, {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  update: (owner: string, repo: string, payload: PushMirrorPayload) =>
    request<PushMirror>(`/repos/${owner}/${repo}/push-mirror`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    }),
  remove: (owner: string, repo: string) =>
    request<void>(`/repos/${owner}/${repo}/push-mirror`, { method: 'DELETE' }),
  /** Run one push now (also the only trigger for `manual` mode). */
  sync: (owner: string, repo: string) =>
    request<{ status: string }>(`/repos/${owner}/${repo}/push-mirror/sync`, { method: 'POST' }),
};
