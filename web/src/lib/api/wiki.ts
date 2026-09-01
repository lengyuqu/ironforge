import { request } from './_base.svelte';
import type { WikiPage, WikiPageSummary, WikiRevision } from '$lib/types/entities';

export const wiki = {
  list: (owner: string, repo: string) =>
    request<WikiPageSummary[]>(`/repos/${owner}/${repo}/wiki`),
  get: (owner: string, repo: string, title: string) =>
    request<WikiPage>(`/repos/${owner}/${repo}/wiki/${encodeURIComponent(title)}`),
  create: (owner: string, repo: string, title: string, content: string, message?: string) =>
    request<WikiPage>(`/repos/${owner}/${repo}/wiki`, {
      method: 'POST',
      body: JSON.stringify({ title, content, message }),
    }),
  update: (owner: string, repo: string, title: string, content: string, message?: string) =>
    request<WikiPage>(`/repos/${owner}/${repo}/wiki/${encodeURIComponent(title)}`, {
      method: 'PATCH',
      body: JSON.stringify({ content, message }),
    }),
  remove: (owner: string, repo: string, title: string) =>
    request<{ message: string }>(`/repos/${owner}/${repo}/wiki/${encodeURIComponent(title)}`, {
      method: 'DELETE',
    }),
  history: (owner: string, repo: string, title: string) =>
    request<WikiRevision[]>(`/repos/${owner}/${repo}/wiki/${encodeURIComponent(title)}/history`),
  revision: (owner: string, repo: string, title: string, revId: number) =>
    request<WikiRevision>(`/repos/${owner}/${repo}/wiki/${encodeURIComponent(title)}/revisions/${revId}`),
  listRevisions: (owner: string, repo: string, title: string) =>
    request<WikiRevision[]>(`/repos/${owner}/${repo}/wiki/${encodeURIComponent(title)}/history`),
  getRevision: (owner: string, repo: string, title: string, revId: number) =>
    request<WikiRevision>(`/repos/${owner}/${repo}/wiki/${encodeURIComponent(title)}/revisions/${revId}`),
};
