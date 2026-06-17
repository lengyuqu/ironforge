import { request } from './_base';

export const wiki = {
  list: (owner: string, repo: string) =>
    request<any[]>(`/repos/${owner}/${repo}/wiki`),
  get: (owner: string, repo: string, title: string) =>
    request<any>(`/repos/${owner}/${repo}/wiki/${title}`),
  create: (owner: string, repo: string, title: string, content: string) =>
    request<any>(`/repos/${owner}/${repo}/wiki`, {
      method: 'POST',
      body: JSON.stringify({ title, content }),
    }),
  update: (owner: string, repo: string, title: string, content: string) =>
    request<any>(`/repos/${owner}/${repo}/wiki/${title}`, {
      method: 'PATCH',
      body: JSON.stringify({ content }),
    }),
  remove: (owner: string, repo: string, title: string) =>
    request<any>(`/repos/${owner}/${repo}/wiki/${title}`, { method: 'DELETE' }),
  listRevisions: (owner: string, repo: string, title: string) =>
    request<any[]>(`/repos/${owner}/${repo}/wiki/${title}/history`),
  getRevision: (owner: string, repo: string, title: string, revId: number) =>
    request<any>(`/repos/${owner}/${repo}/wiki/${title}/revisions/${revId}`),
};
