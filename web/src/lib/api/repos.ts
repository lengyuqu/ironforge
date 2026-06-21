import { request, qs, type PaginatedResponse } from './_base';

export const repos = {
  list: (owner: string, page?: number, perPage?: number) =>
    request<PaginatedResponse<{ id: number; name: string; description: string | null; is_private: boolean; created_at: string }>>(
      `/repos/${owner}${qs({ page, per_page: perPage })}`
    ),
  get: (owner: string, name: string) =>
    request<{ id: number; name: string; description: string | null; is_private: boolean; default_branch: string; created_at: string }>(`/repos/${owner}/${name}`),
  create: (name: string, description?: string, is_private?: boolean, org?: string) =>
    request<{ id: number; name: string }>('/repos', {
      method: 'POST',
      body: JSON.stringify({ name, description, is_private, org }),
    }),
  tree: (owner: string, repo: string, ref?: string, path?: string) =>
    request<{ entries: { name: string; kind: string; size?: number }[] }>(`/repos/${owner}/${repo}/tree${qs({ ref, path })}`),
  blob: (owner: string, repo: string, path: string, ref?: string) =>
    request<{ content: string; size: number; name: string }>(`/repos/${owner}/${repo}/blob/${path}${qs({ ref })}`),
  log: (owner: string, repo: string, ref?: string, path?: string) =>
    request<{ commits: { sha: string; message: string; author: string; date: string }[] }>(`/repos/${owner}/${repo}/log${qs({ ref, path })}`),
  branches: (owner: string, repo: string) =>
    request<{ name: string; is_default: boolean }[]>(`/repos/${owner}/${repo}/branches`),
  tags: (owner: string, repo: string) =>
    request<{ name: string }[]>(`/repos/${owner}/${repo}/tags`),
  commitSignature: (owner: string, repo: string, sha: string) =>
    request<{ verified: boolean; signer_key: string | null; signer_name: string | null; signer_email: string | null; status: string }>(`/repos/${owner}/${repo}/commits/${sha}/signature`),
  star: (owner: string, repo: string) =>
    request<{ starred: boolean }>(`/repos/${owner}/${repo}/star`, { method: 'PUT' }),
  unstar: (owner: string, repo: string) =>
    request<{ starred: boolean }>(`/repos/${owner}/${repo}/star`, { method: 'PUT' }),
  stargazers: (owner: string, repo: string, page?: number, perPage?: number) =>
    request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/stargazers${qs({ page, per_page: perPage })}`),
  watch: (owner: string, repo: string, state: string) =>
    request<{ watch_state: string }>(`/repos/${owner}/${repo}/watch`, { method: 'PUT', body: JSON.stringify({ state }) }),
  unwatch: (owner: string, repo: string) =>
    request<{ watch_state: string }>(`/repos/${owner}/${repo}/watch`, { method: 'DELETE' }),
  delete: (owner: string, repo: string) =>
    request<{ deleted: boolean }>(`/repos/${owner}/${repo}`, { method: 'DELETE' }),
  fork: (owner: string, repo: string) =>
    request<any>(`/repos/${owner}/${repo}/fork`, { method: 'POST' }),
  forks: (owner: string, repo: string, page?: number, perPage?: number) => {
    const params = new URLSearchParams();
    if (page) params.set('page', String(page));
    if (perPage) params.set('per_page', String(perPage));
    const qs = params.toString() ? `?${params.toString()}` : '';
    return request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/forks${qs}`);
  },
  transfer: (owner: string, repo: string, newOwner: string) =>
    request<any>(`/repos/${owner}/${repo}/transfer`, { method: 'POST', body: JSON.stringify({ new_owner: newOwner }) }),
  createCommitStatus: (owner: string, repo: string, sha: string, data: { state: string; context: string; description?: string; target_url?: string }) =>
    request<any>(`/repos/${owner}/${repo}/statuses/${sha}`, { method: 'POST', body: JSON.stringify(data) }),
  listCommitStatuses: (owner: string, repo: string, sha: string) =>
    request<any[]>(`/repos/${owner}/${repo}/commits/${sha}/statuses`),
  getCombinedStatus: (owner: string, repo: string, sha: string) =>
    request<any>(`/repos/${owner}/${repo}/commits/${sha}/status`),
};
