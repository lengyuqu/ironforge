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
    request<{ id: number; username: string; email: string; is_admin: boolean; display_name: string | null }>('/users/me'),
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
  // Star
  star: (owner: string, repo: string) =>
    request<{ starred: boolean }>(`/repos/${owner}/${repo}/star`, { method: 'PUT' }),
  unstar: (owner: string, repo: string) =>
    request<{ starred: boolean }>(`/repos/${owner}/${repo}/star`, { method: 'DELETE' }),
  stargazers: (owner: string, repo: string, page?: number, perPage?: number) =>
    request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/stargazers${qs({ page, per_page: perPage })}`),
  // Watch
  watch: (owner: string, repo: string, state: string) =>
    request<{ watch_state: string }>(`/repos/${owner}/${repo}/watch`, { method: 'PUT', body: JSON.stringify({ state }) }),
  unwatch: (owner: string, repo: string) =>
    request<{ watch_state: string }>(`/repos/${owner}/${repo}/watch`, { method: 'DELETE' }),
  // Delete
  delete: (owner: string, repo: string) =>
    request<{ deleted: boolean }>(`/repos/${owner}/${repo}`, { method: 'DELETE' }),
  // Fork
  fork: (owner: string, repo: string) =>
    request<any>(`/repos/${owner}/${repo}/fork`, { method: 'POST' }),
  forks: (owner: string, repo: string, page?: number, perPage?: number) => {
    const params = new URLSearchParams();
    if (page) params.set('page', String(page));
    if (perPage) params.set('per_page', String(perPage));
    const qs = params.toString() ? `?${params.toString()}` : '';
    return request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/forks${qs}`);
  },
  // Transfer
  transfer: (owner: string, repo: string, newOwner: string) =>
    request<any>(`/repos/${owner}/${repo}/transfer`, { method: 'POST', body: JSON.stringify({ new_owner: newOwner }) }),
  // Commit Statuses
  createCommitStatus: (owner: string, repo: string, sha: string, data: { state: string; context: string; description?: string; target_url?: string }) =>
    request<any>(`/repos/${owner}/${repo}/statuses/${sha}`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  listCommitStatuses: (owner: string, repo: string, sha: string) =>
    request<any[]>(`/repos/${owner}/${repo}/commits/${sha}/statuses`),
  getCombinedStatus: (owner: string, repo: string, sha: string) =>
    request<any>(`/repos/${owner}/${repo}/commits/${sha}/status`),
};

// ── Issues ───────────────────────────────────────────
export const issues = {
  list: (owner: string, repo: string, state?: string, page?: number, perPage?: number, labels?: string) => {
    return request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/issues${qs({ state, page, per_page: perPage, labels })}`);
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
  // Issue labels
  labels: (owner: string, repo: string, number: number) =>
    request<any[]>(`/repos/${owner}/${repo}/issues/${number}/labels`),
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

// ── Releases ──────────────────────────────────────
export const releases = {
  list: (owner: string, repo: string, page?: number, perPage?: number) =>
    request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/releases${qs({ page, per_page: perPage })}`),
  get: (owner: string, repo: string, id: number) =>
    request<any>(`/repos/${owner}/${repo}/releases/${id}`),
  create: (owner: string, repo: string, data: { tag_name: string; title: string; body?: string; target_commitish?: string; is_draft?: boolean; is_prerelease?: boolean }) =>
    request<any>(`/repos/${owner}/${repo}/releases`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  update: (owner: string, repo: string, id: number, data: { title?: string; body?: string; is_draft?: boolean; is_prerelease?: boolean }) =>
    request<any>(`/repos/${owner}/${repo}/releases/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(data),
    }),
  delete: (owner: string, repo: string, id: number) =>
    request<void>(`/repos/${owner}/${repo}/releases/${id}`, { method: 'DELETE' }),
};

// Labels
export const labels = {
  list: (owner: string, repo: string) =>
    request<any[]>(`/repos/${owner}/${repo}/labels`),
  get: (owner: string, repo: string, id: number) =>
    request<any>(`/repos/${owner}/${repo}/labels/${id}`),
  create: (owner: string, repo: string, name: string, color: string, description?: string) =>
    request<any>(`/repos/${owner}/${repo}/labels`, {
      method: 'POST',
      body: JSON.stringify({ name, color, description }),
    }),
  update: (owner: string, repo: string, id: number, data: { name?: string; color?: string; description?: string }) =>
    request<any>(`/repos/${owner}/${repo}/labels/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(data),
    }),
  delete: (owner: string, repo: string, id: number) =>
    request<void>(`/repos/${owner}/${repo}/labels/${id}`, { method: 'DELETE' }),
  // Issue labels
  forIssue: (owner: string, repo: string, issueNumber: number) =>
    request<any[]>(`/repos/${owner}/${repo}/issues/${issueNumber}/labels`),
};

// Milestones
export const milestones = {
  list: (owner: string, repo: string, state?: string) => {
    const params = new URLSearchParams();
    if (state) params.set('state', state);
    const qs = params.toString() ? `?${params.toString()}` : '';
    return request<any[]>(`/repos/${owner}/${repo}/milestones${qs}`);
  },
  get: (owner: string, repo: string, id: number) =>
    request<any>(`/repos/${owner}/${repo}/milestones/${id}`),
  create: (owner: string, repo: string, data: { title: string; description?: string; due_date?: string; state?: string }) =>
    request<any>(`/repos/${owner}/${repo}/milestones`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  update: (owner: string, repo: string, id: number, data: { title?: string; description?: string; state?: string; due_date?: string }) =>
    request<any>(`/repos/${owner}/${repo}/milestones/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(data),
    }),
  delete: (owner: string, repo: string, id: number) =>
    request<void>(`/repos/${owner}/${repo}/milestones/${id}`, { method: 'DELETE' }),
};

// Tokens (PAT)
export const tokens = {
  list: () =>
    request<any[]>('/users/tokens'),
  create: (name: string, scopes?: string, expires_at?: string) =>
    request<{ id: number; name: string; token: string; scopes: string; expires_at?: string; created_at: string }>('/users/tokens', {
      method: 'POST',
      body: JSON.stringify({ name, scopes, expires_at }),
    }),
  delete: (id: number) =>
    request<void>(`/users/tokens/${id}`, { method: 'DELETE' }),
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

// Audit Log
export interface AuditLogEntry {
  id: number;
  user_id: number | null;
  username: string | null;
  action: string;
  resource_type: string | null;
  resource_id: number | null;
  resource_name: string | null;
  ip_address: string | null;
  details: string | null;
  created_at: string;
}

export interface AuditLogResponse {
  total: number;
  page: number;
  page_size: number;
  logs: AuditLogEntry[];
}

export interface AuditLogQuery {
  page?: number;
  page_size?: number;
  user_id?: number;
  action?: string;
  resource_type?: string;
  start_time?: string;
  end_time?: string;
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

  // Audit Logs
  listAuditLogs: (query?: AuditLogQuery) =>
    request<AuditLogResponse>(`/admin/audit/logs${qs({
      page: query?.page,
      page_size: query?.page_size,
      user_id: query?.user_id,
      action: query?.action,
      resource_type: query?.resource_type,
      start_time: query?.start_time,
      end_time: query?.end_time,
    })}`),
  getAuditLog: (id: number) =>
    request<AuditLogEntry>(`/admin/audit/logs/${id}`),
};

// ── WebSocket ────────────────────────────────────────
export interface SearchResult {
  result_type: string;
  id: number;
  title: string;
  excerpt: string | null;
  repo_owner: string | null;
  repo_name: string | null;
}

export interface SearchResponse {
  results: SearchResult[];
  total: number;
  page: number;
  per_page: number;
}

export const search = {
  search: (q: string, type?: string, page?: number, perPage?: number) =>
    request<SearchResponse>(`/search${qs({ q, type: type || 'all', page, per_page: perPage })}`),
};

// ── Packages ─────────────────────────────────────
export const packages = {
  list: (owner: string, repo: string, format?: string, page?: number, perPage?: number) =>
    request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/packages${qs({ format, page, per_page: perPage })}`),
  getFormat: (owner: string, repo: string, format: string) =>
    request<{ format: string; packages: any[] }>(`/repos/${owner}/${repo}/packages/${format}`),
  get: (owner: string, repo: string, format: string, name: string) =>
    request<any>(`/repos/${owner}/${repo}/packages/${format}/${name}`),
  getVersions: (owner: string, repo: string, format: string, name: string) =>
    request<{ name: string; versions: string[] }>(`/repos/${owner}/${repo}/packages/${format}/${name}/versions`),
  getVersion: (owner: string, repo: string, format: string, name: string, version: string) =>
    request<any>(`/repos/${owner}/${repo}/packages/${format}/${name}/versions/${version}`),
  create: (owner: string, repo: string, format: string, data: { name: string; version: string; description?: string; content_type?: string; file?: File }) =>
    request<any>(`/repos/${owner}/${repo}/packages/${format}`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  delete: (owner: string, repo: string, format: string, name: string, version: string) =>
    request<{ deleted: boolean }>(`/repos/${owner}/${repo}/packages/${format}/${name}/versions/${version}`, { method: 'DELETE' }),
};

// ── Runners ──────────────────────────────────────
export const runners = {
  list: (page?: number, perPage?: number) =>
    request<PaginatedResponse<any>>(`/runners${qs({ page, per_page: perPage })}`),
  get: (id: number) =>
    request<any>(`/runners/${id}`),
  register: (data: { name: string; labels?: string[] }) =>
    request<any>('/runners', {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  delete: (id: number) =>
    request<{ deleted: boolean }>(`/runners/${id}`, { method: 'DELETE' }),
};

// ── Time Tracking ─────────────────────────────────
export const timeTracking = {
  list: (owner: string, repo: string, page?: number, perPage?: number) =>
    request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/time-tracking${qs({ page, per_page: perPage })}`),
  add: (owner: string, repo: string, data: { duration: number; note?: string; date?: string }) =>
    request<any>(`/repos/${owner}/${repo}/time-tracking`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  delete: (owner: string, repo: string, id: number) =>
    request<{ deleted: boolean }>(`/repos/${owner}/${repo}/time-tracking/${id}`, { method: 'DELETE' }),
};

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
