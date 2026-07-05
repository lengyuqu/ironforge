import { request } from './_base.svelte';

export const collaborators = {
  list: (owner: string, repo: string) =>
    request<any[]>(`/repos/${owner}/${repo}/collaborators`),
  add: (owner: string, repo: string, userId: number, permission: string) =>
    request<any>(`/repos/${owner}/${repo}/collaborators`, {
      method: 'POST',
      body: JSON.stringify({ user_id: userId, permission }),
    }),
  updatePermission: (owner: string, repo: string, id: number, permission: string) =>
    request<any>(`/repos/${owner}/${repo}/collaborators/${id}`, {
      method: 'PATCH',
      body: JSON.stringify({ permission }),
    }),
  remove: (owner: string, repo: string, userId: number) =>
    request<void>(`/repos/${owner}/${repo}/collaborators/${userId}`, { method: 'DELETE' }),
};
