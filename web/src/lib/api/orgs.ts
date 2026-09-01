import { request } from './_base.svelte';
import type { Organization, OrgMember, OrgSummary, OrganizationTeam, TeamMember } from '$lib/types/entities';

/** 创建成员 / 添加成员的精简响应（后端只回传关联字段） */
interface MemberCreated {
  id: number;
  user_id: number;
  role: string;
}

export const orgs = {
  list: (userId?: number) =>
    request<Organization[]>(`/orgs${userId ? `?user_id=${userId}` : ''}`),
  get: (name: string) =>
    request<Organization>(`/orgs/${name}`),
  create: (name: string, displayName?: string, description?: string, visibility?: string) =>
    request<OrgSummary>('/orgs', {
      method: 'POST',
      body: JSON.stringify({ name, display_name: displayName, description, visibility }),
    }),
  update: (name: string, data: { display_name?: string; description?: string; visibility?: string }) =>
    request<Organization>(`/orgs/${name}`, {
      method: 'PATCH',
      body: JSON.stringify(data),
    }),
  delete: (name: string) =>
    request<{ deleted: boolean }>(`/orgs/${name}`, { method: 'DELETE' }),
  listMembers: (name: string) =>
    request<OrgMember[]>(`/orgs/${name}/members`),
  addMember: (name: string, userId: number, role?: string) =>
    request<MemberCreated>(`/orgs/${name}/members`, {
      method: 'POST',
      body: JSON.stringify({ user_id: userId, role: role || 'member' }),
    }),
  removeMember: (name: string, userId: number) =>
    request<{ removed: boolean }>(`/orgs/${name}/members/${userId}`, { method: 'DELETE' }),
  listTeams: (name: string) =>
    request<OrganizationTeam[]>(`/orgs/${name}/teams`),
  createTeam: (name: string, teamName: string, description?: string, permission?: string) =>
    request<{ id: number; org_id: number; name: string; permission: string }>(`/orgs/${name}/teams`, {
      method: 'POST',
      body: JSON.stringify({ name: teamName, description, permission: permission || 'read' }),
    }),
  deleteTeam: (name: string, teamId: number) =>
    request<{ deleted: boolean }>(`/orgs/${name}/teams/${teamId}`, { method: 'DELETE' }),
  listTeamMembers: (name: string, teamId: number) =>
    request<TeamMember[]>(`/orgs/${name}/teams/${teamId}/members`),
  addTeamMember: (name: string, teamId: number, userId: number, role?: string) =>
    request<MemberCreated>(`/orgs/${name}/teams/${teamId}/members`, {
      method: 'POST',
      body: JSON.stringify({ user_id: userId, role: role || 'member' }),
    }),
  removeTeamMember: (name: string, teamId: number, userId: number) =>
    request<{ removed: boolean }>(`/orgs/${name}/teams/${teamId}/members/${userId}`, { method: 'DELETE' }),
};
