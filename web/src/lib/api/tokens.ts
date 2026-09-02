import { request } from './_base.svelte';

/** 个人访问令牌（列表项） */
export interface AccessToken {
  id: number;
  name: string;
  scopes: string;
  expires_at?: string | null;
  last_used_at?: string | null;
  created_at: string;
}

/** 创建令牌的响应（token 明文仅此一次返回） */
export interface CreatedToken {
  id: number;
  name: string;
  token: string;
  scopes: string;
  expires_at?: string;
  created_at: string;
}

export const tokens = {
  list: () =>
    request<AccessToken[]>('/users/tokens'),
  create: (name: string, scopes?: string, expires_at?: string) =>
    request<CreatedToken>('/users/tokens', {
      method: 'POST',
      body: JSON.stringify({ name, scopes, expires_at }),
    }),
  delete: (id: number) =>
    request<void>(`/users/tokens/${id}`, { method: 'DELETE' }),
};
