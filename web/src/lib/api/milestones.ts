import { request } from './_base.svelte';
import type { Milestone, CreateMilestoneInput, UpdateMilestoneInput } from '../types/entities';
export type { Milestone, CreateMilestoneInput, UpdateMilestoneInput };

export const milestones = {
  list: (owner: string, repo: string, state?: string) => {
    const params = new URLSearchParams();
    if (state) params.set('state', state);
    const qs = params.toString() ? `?${params.toString()}` : '';
    return request<Milestone[]>(`/repos/${owner}/${repo}/milestones${qs}`);
  },
  get: (owner: string, repo: string, id: number) =>
    request<Milestone>(`/repos/${owner}/${repo}/milestones/${id}`),
  create: (owner: string, repo: string, data: CreateMilestoneInput) =>
    request<Milestone>(`/repos/${owner}/${repo}/milestones`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  update: (owner: string, repo: string, id: number, data: UpdateMilestoneInput) =>
    request<Milestone>(`/repos/${owner}/${repo}/milestones/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(data),
    }),
  delete: (owner: string, repo: string, id: number) =>
    request<void>(`/repos/${owner}/${repo}/milestones/${id}`, { method: 'DELETE' }),
};
