import { request } from './_base';

export const collaborators = {
  list: (owner: string, repo: string) =>
    request<any[]>(`/repos/${owner}/${repo}/collaborators`),
  add: (owner: string, repo: string, userId: number, permission: string) =>
    request<any>(`/repos/${owner}/${repo}/collaborators`, {
      method: 'POST',
      body: JSON.stringify({ user_id: userId, permission }),
    }),
  remove: (owner: string, repo: string, userId: number) =>
    request(`/repos/${owner}/${repo}/collaborators/${userId}/remove`, { method: 'POST' }),
};
