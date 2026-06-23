import { request, qs, type PaginatedResponse } from './_base';

export const pulls = {
  list: (owner: string, repo: string, state?: string, page?: number, perPage?: number) =>
    request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/pulls${qs({ state, page, per_page: perPage })}`),
  get: (owner: string, repo: string, number: number) =>
    request<any>(`/repos/${owner}/${repo}/pulls/${number}`),
  create: (owner: string, repo: string, data: { title: string; body?: string; head_branch: string; base_branch: string }) =>
    request<any>(`/repos/${owner}/${repo}/pulls`, {
      method: 'POST',
      body: JSON.stringify({
        title: data.title,
        body: data.body,
        head: data.head_branch,
        base: data.base_branch,
      }),
    }),
  diff: (owner: string, repo: string, number: number) =>
    request<{ diff: string }>(`/repos/${owner}/${repo}/pulls/${number}/diff`),
  merge: (owner: string, repo: string, number: number, strategy: string) =>
    request<any>(`/repos/${owner}/${repo}/pulls/${number}/merge`, { method: 'POST', body: JSON.stringify({ strategy }) }),
};

export const reviews = {
  list: (owner: string, repo: string, number: number) =>
    request<any[]>(`/repos/${owner}/${repo}/pulls/${number}/reviews`),
  submit: (owner: string, repo: string, number: number, body: string, verdict: string) =>
    request<any>(`/repos/${owner}/${repo}/pulls/${number}/reviews`, {
      method: 'POST',
      body: JSON.stringify({ body, action: verdict }),
    }),
  comments: (owner: string, repo: string, number: number) =>
    request<any[]>(`/repos/${owner}/${repo}/pulls/${number}/comments`),
  addComment: (owner: string, repo: string, number: number, data: { body: string; path?: string; line?: number }) =>
    request<any>(`/repos/${owner}/${repo}/pulls/${number}/comments`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),
};
