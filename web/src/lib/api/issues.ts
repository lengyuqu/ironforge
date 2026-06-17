import { request, qs, type PaginatedResponse } from './_base';

export const issues = {
  list: (owner: string, repo: string, state?: string, page?: number, perPage?: number, labels?: string) =>
    request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/issues${qs({ state, page, per_page: perPage, labels })}`),
  get: (owner: string, repo: string, number: number) =>
    request<any>(`/repos/${owner}/${repo}/issues/${number}`),
  create: (owner: string, repo: string, title: string, body?: string, labels?: string[]) =>
    request<any>(`/repos/${owner}/${repo}/issues`, {
      method: 'POST',
      body: JSON.stringify({ title, body, labels }),
    }),
  update: (owner: string, repo: string, number: number, data: Record<string, any>) =>
    request<any>(`/repos/${owner}/${repo}/issues/${number}`, {
      method: 'PATCH',
      body: JSON.stringify(data),
    }),
  comments: (owner: string, repo: string, number: number) =>
    request<any[]>(`/repos/${owner}/${repo}/issues/${number}/comments`),
  labels: (owner: string, repo: string, number: number) =>
    request<any[]>(`/repos/${owner}/${repo}/issues/${number}/labels`),
  addComment: (owner: string, repo: string, number: number, body: string) =>
    request<any>(`/repos/${owner}/${repo}/issues/${number}/comments`, {
      method: 'POST',
      body: JSON.stringify({ body }),
    }),
};
