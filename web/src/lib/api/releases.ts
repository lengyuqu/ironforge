import { request, qs, type PaginatedResponse } from './_base';

export const releases = {
  list: (owner: string, repo: string, page?: number, perPage?: number) =>
    request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/releases${qs({ page, per_page: perPage })}`),
  get: (owner: string, repo: string, id: number) =>
    request<any>(`/repos/${owner}/${repo}/releases/${id}`),
  create: (owner: string, repo: string, data: { tag_name: string; title: string; body?: string; target_commitish?: string; is_draft?: boolean; is_prerelease?: boolean }) =>
    request<any>(`/repos/${owner}/${repo}/releases`, { method: 'POST', body: JSON.stringify(data) }),
  update: (owner: string, repo: string, id: number, data: { title?: string; body?: string; is_draft?: boolean; is_prerelease?: boolean }) =>
    request<any>(`/repos/${owner}/${repo}/releases/${id}`, { method: 'PATCH', body: JSON.stringify(data) }),
  delete: (owner: string, repo: string, id: number) =>
    request<void>(`/repos/${owner}/${repo}/releases/${id}`, { method: 'DELETE' }),
};
