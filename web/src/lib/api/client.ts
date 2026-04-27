// IronForge API Client

const API_BASE = '/api/v1';

let authToken = $state<string | null>(null);

// ── Pagination types ─────────────────────────────────
export interface PaginationMeta {
  page: number;
  per_page: number;
  total: number;
  total_pages: number;
  has_next: boolean;
  has_prev: boolean;
}

export interface PaginatedResponse<T> {
  data: T[];
  pagination: PaginationMeta;
}

export function getToken(): string | null {
  if (typeof window === 'undefined') return null;
  return authToken || localStorage.getItem('ironforge_token');
}

export function setToken(token: string | null) {
  authToken = token;
  if (typeof window === 'undefined') return;
  if (token) {
    localStorage.setItem('ironforge_token', token);
  } else {
    localStorage.removeItem('ironforge_token');
  }
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const token = getToken();
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string> || {}),
  };
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }

  const res = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers,
  });

  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error(body.error || `HTTP ${res.status}`);
  }

  return res.json();
}

/// Build query string from key-value pairs, skipping nullish values.
function qs(params: Record<string, string | number | boolean | undefined | null>): string {
  const parts = Object.entries(params)
    .filter(([, v]) => v !== undefined && v !== null && v !== '')
    .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(String(v))}`);
  return parts.length > 0 ? '?' + parts.join('&') : '';
}

// ── Auth ─────────────────────────────────────────────
export const auth = {
  register: (username: string, email: string, password: string) =>
    request<{ id: number; username: string }>('/users/register', {
      method: 'POST',
      body: JSON.stringify({ username, email, password }),
    }),
  login: (username: string, password: string) =>
    request<{ token: string; user: { id: number; username: string; email: string } }>('/users/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    }),
  me: () =>
    request<{ id: number; username: string; email: string }>('/users/me'),
};

// ── Repos ────────────────────────────────────────────
export const repos = {
  list: (owner: string, page?: number, perPage?: number) =>
    request<PaginatedResponse<{ id: number; name: string; description: string | null; is_private: boolean; created_at: string }>>(
      `/repos/${owner}${qs({ page, per_page: perPage })}`
    ),
  get: (owner: string, name: string) =>
    request<{ id: number; name: string; description: string | null; is_private: boolean; default_branch: string; created_at: string }>(`/repos/${owner}/${name}`),
  create: (name: string, description?: string, is_private?: boolean, org?: string) =>
    request<{ id: number; name: string }>('/repos', {
      method: 'POST',
      body: JSON.stringify({ name, description, is_private, org }),
    }),
  // Content browsing
  tree: (owner: string, repo: string, ref?: string, path?: string) => {
    return request<{ entries: { name: string; kind: string; size?: number }[] }>(`/repos/${owner}/${repo}/tree${qs({ ref, path })}`);
  },
  blob: (owner: string, repo: string, path: string, ref?: string) => {
    return request<{ content: string; size: number; name: string }>(`/repos/${owner}/${repo}/blob/${path}${qs({ ref })}`);
  },
  log: (owner: string, repo: string, ref?: string, path?: string) => {
    return request<{ commits: { sha: string; message: string; author: string; date: string }[] }>(`/repos/${owner}/${repo}/log${qs({ ref, path })}`);
  },
  branches: (owner: string, repo: string) =>
    request<{ name: string; is_default: boolean }[]>(`/repos/${owner}/${repo}/branches`),
  tags: (owner: string, repo: string) =>
    request<{ name: string }[]>(`/repos/${owner}/${repo}/tags`),
  // GPG signature
  commitSignature: (owner: string, repo: string, sha: string) =>
    request<{ verified: boolean; signer_key: string | null; signer_name: string | null; signer_email: string | null; status: string }>(`/repos/${owner}/${repo}/commits/${sha}/signature`),
};

// ── Issues ───────────────────────────────────────────
export const issues = {
  list: (owner: string, repo: string, state?: string, page?: number, perPage?: number) => {
    return request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/issues${qs({ state, page, per_page: perPage })}`);
  },
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
  addComment: (owner: string, repo: string, number: number, body: string) =>
    request<any>(`/repos/${owner}/${repo}/issues/${number}/comments`, {
      method: 'POST',
      body: JSON.stringify({ body }),
    }),
};

// ── Pull Requests ────────────────────────────────────
export const pulls = {
  list: (owner: string, repo: string, state?: string, page?: number, perPage?: number) => {
    return request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/pulls${qs({ state, page, per_page: perPage })}`);
  },
  get: (owner: string, repo: string, number: number) =>
    request<any>(`/repos/${owner}/${repo}/pulls/${number}`),
  create: (owner: string, repo: string, data: { title: string; body?: string; head_branch: string; base_branch: string }) =>
    request<any>(`/repos/${owner}/${repo}/pulls`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  diff: (owner: string, repo: string, number: number) =>
    request<{ diff: string }>(`/repos/${owner}/${repo}/pulls/${number}/diff`),
  merge: (owner: string, repo: string, number: number, strategy: string) =>
    request<any>(`/repos/${owner}/${repo}/pulls/${number}/merge`, {
      method: 'POST',
      body: JSON.stringify({ strategy }),
    }),
};

// ── Reviews ──────────────────────────────────────────
export const reviews = {
  list: (owner: string, repo: string, number: number) =>
    request<any[]>(`/repos/${owner}/${repo}/pulls/${number}/reviews`),
  submit: (owner: string, repo: string, number: number, body: string, verdict: string) =>
    request<any>(`/repos/${owner}/${repo}/pulls/${number}/reviews`, {
      method: 'POST',
      body: JSON.stringify({ body, verdict }),
    }),
  comments: (owner: string, repo: string, number: number) =>
    request<any[]>(`/repos/${owner}/${repo}/pulls/${number}/comments`),
  addComment: (owner: string, repo: string, number: number, data: { body: string; path?: string; line?: number }) =>
    request<any>(`/repos/${owner}/${repo}/pulls/${number}/comments`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),
};

// ── CI/CD Pipelines ─────────────────────────────────
export const pipelines = {
  list: (owner: string, repo: string, page?: number, perPage?: number) =>
    request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/pipelines${qs({ page, per_page: perPage })}`),
  get: (owner: string, repo: string, id: number) =>
    request<any>(`/repos/${owner}/${repo}/pipelines/${id}`),
  trigger: (owner: string, repo: string, ref?: string) =>
    request<any>(`/repos/${owner}/${repo}/pipelines`, {
      method: 'POST',
      body: JSON.stringify({ ref }),
    }),
  retry: (owner: string, repo: string, id: number) =>
    request<any>(`/repos/${owner}/${repo}/pipelines/${id}/retry`, { method: 'POST' }),
  cancel: (owner: string, repo: string, id: number) =>
    request<any>(`/repos/${owner}/${repo}/pipelines/${id}/cancel`, { method: 'POST' }),
  job: (owner: string, repo: string, pipelineId: number, jobId: number) =>
    request<any>(`/repos/${owner}/${repo}/pipelines/${pipelineId}/jobs/${jobId}`),
};

// ── Wiki ─────────────────────────────────────────────
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
};

// ── Collaborators ────────────────────────────────────
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

// ── Organizations ────────────────────────────────────
export const orgs = {
  list: (userId?: number) =>
    request<any[]>(`/orgs${userId ? `?user_id=${userId}` : ''}`),
  get: (name: string) =>
    request<any>(`/orgs/${name}`),
  create: (name: string, displayName?: string, description?: string, visibility?: string) =>
    request<any>('/orgs', {
      method: 'POST',
      body: JSON.stringify({ name, display_name: displayName, description, visibility }),
    }),
  update: (name: string, data: { display_name?: string; description?: string; visibility?: string }) =>
    request<any>(`/orgs/${name}`, {
      method: 'PATCH',
      body: JSON.stringify(data),
    }),
  delete: (name: string) =>
    request<any>(`/orgs/${name}`, { method: 'DELETE' }),
  // Members
  listMembers: (name: string) =>
    request<any[]>(`/orgs/${name}/members`),
  addMember: (name: string, userId: number, role?: string) =>
    request<any>(`/orgs/${name}/members`, {
      method: 'POST',
      body: JSON.stringify({ user_id: userId, role: role || 'member' }),
    }),
  removeMember: (name: string, userId: number) =>
    request<any>(`/orgs/${name}/members/${userId}`, { method: 'DELETE' }),
  // Teams
  listTeams: (name: string) =>
    request<any[]>(`/orgs/${name}/teams`),
  createTeam: (name: string, teamName: string, description?: string, permission?: string) =>
    request<any>(`/orgs/${name}/teams`, {
      method: 'POST',
      body: JSON.stringify({ name: teamName, description, permission: permission || 'read' }),
    }),
  deleteTeam: (name: string, teamId: number) =>
    request<any>(`/orgs/${name}/teams/${teamId}`, { method: 'DELETE' }),
  listTeamMembers: (name: string, teamId: number) =>
    request<any[]>(`/orgs/${name}/teams/${teamId}/members`),
  addTeamMember: (name: string, teamId: number, userId: number, role?: string) =>
    request<any>(`/orgs/${name}/teams/${teamId}/members`, {
      method: 'POST',
      body: JSON.stringify({ user_id: userId, role: role || 'member' }),
    }),
  removeTeamMember: (name: string, teamId: number, userId: number) =>
    request<any>(`/orgs/${name}/teams/${teamId}/members/${userId}`, { method: 'DELETE' }),
};

// ── Notifications ────────────────────────────────────
export const notifications = {
  list: (userId?: number, unreadOnly?: boolean, page?: number, perPage?: number) =>
    request<PaginatedResponse<any>>(`/notifications${qs({ user_id: userId, unread_only: unreadOnly, page, per_page: perPage })}`),
  unreadCount: (userId?: number) =>
    request<any>(`/notifications/unread-count${userId ? `?user_id=${userId}` : ''}`),
  markRead: (id: number) =>
    request<any>(`/notifications/${id}/read`, { method: 'POST' }),
  markAllRead: (userId?: number) =>
    request<any>(`/notifications/mark-all-read${userId ? `?user_id=${userId}` : ''}`, { method: 'POST' }),
  delete: (id: number) =>
    request<any>(`/notifications/${id}`, { method: 'DELETE' }),
};

// ── Admin ────────────────────────────────────────────
export interface AdminUser {
  id: number;
  username: string;
  email: string;
  display_name: string | null;
  avatar_url: string | null;
  bio: string | null;
  is_admin: boolean;
  is_active: boolean;
  created_at: string;
}

export interface AdminOrg {
  id: number;
  name: string;
  display_name: string | null;
  description: string | null;
  owner_id: number;
  visibility: string;
  created_at: string;
  updated_at: string;
}

export interface UpdateUserData {
  display_name?: string;
  bio?: string;
  is_admin?: boolean;
  is_active?: boolean;
}

export const admin = {
  // Users
  listUsers: (page?: number, perPage?: number) =>
    request<PaginatedResponse<AdminUser>>(`/admin/users${qs({ page, per_page: perPage })}`),
  getUser: (id: number) =>
    request<AdminUser>(`/admin/users/${id}`),
  updateUser: (id: number, data: UpdateUserData) =>
    request<AdminUser>(`/admin/users/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(data),
    }),
  deleteUser: (id: number) =>
    request<{ deleted: boolean }>(`/admin/users/${id}`, { method: 'DELETE' }),

  // Organizations
  listOrgs: (page?: number, perPage?: number) =>
    request<PaginatedResponse<AdminOrg>>(`/admin/orgs${qs({ page, per_page: perPage })}`),
  getOrg: (name: string) =>
    request<AdminOrg>(`/admin/orgs/${name}`),
  deleteOrg: (name: string) =>
    request<{ deleted: boolean }>(`/admin/orgs/${name}`, { method: 'DELETE' }),
};

// ── WebSocket ────────────────────────────────────────
export function connectNotificationWebSocket(
  onMessage: (event: { event_type: string; data: any }) => void,
  onError?: (err: Event) => void,
): WebSocket | null {
  const token = getToken();
  if (!token) return null;

  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const wsUrl = `${protocol}//${window.location.host}/api/v1/ws/notifications?token=${encodeURIComponent(token)}`;

  const ws = new WebSocket(wsUrl);

  ws.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);
      onMessage(data);
    } catch {
      // ignore non-JSON messages
    }
  };

  ws.onerror = (err) => {
    if (onError) onError(err);
  };

  ws.onclose = () => {
    // Auto-reconnect after 5 seconds
    setTimeout(() => {
      connectNotificationWebSocket(onMessage, onError);
    }, 5000);
  };

  return ws;
}
