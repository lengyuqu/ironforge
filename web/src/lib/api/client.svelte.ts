// IronForge API Client
// Shared internals (API_BASE, getToken, setToken, request, etc.) live in _base.svelte.ts.
// This file imports them and adds API-specific methods on top.

import {
  API_BASE,
  withApiBase,
  getToken,
  setToken,
  request,
  downloadApiFile,
  qs,
  type PaginationMeta,
  type PaginatedResponse,
} from './_base.svelte';

// Re-export for backward compatibility — many route files import these from client.
export { API_BASE, getToken, setToken, type PaginationMeta, type PaginatedResponse };

function withWebSocketApiBase(path: string): string {
  const apiUrl = new URL(API_BASE, window.location.origin);
  const protocol = apiUrl.protocol === 'https:' ? 'wss:' : 'ws:';
  const basePath = apiUrl.pathname.replace(/\/+$/g, '');
  return `${protocol}//${apiUrl.host}${basePath}${path.startsWith('/') ? path : `/${path}`}`;
}

function encodeRepoPath(path: string): string {
  return path.split('/').map(encodeURIComponent).join('/');
}

interface PackageSummaryResponse {
  id: number;
  name: string;
  description: string | null;
  homepage: string | null;
  version_count: number;
  latest_version: string | null;
  download_count: number;
  keywords: string | null;
  format?: string;
}

interface PackageRegistry {
  package_type: string;
  enabled: boolean;
}

interface PackageListByTypeResponse {
  packages: PackageSummaryResponse[];
}

interface PackageVersionResponse {
  id: number;
  version: string;
  semver: string | null;
  metadata: string | null;
  size: number;
  sha256: string | null;
  is_yanked: boolean;
  download_count: number;
  files: PackageFileResponse[];
  created_at: string;
}

interface PackageFileResponse {
  id: number;
  filename: string;
  size: number;
  sha256: string | null;
}

interface VersionListByTypeResponse {
  versions: PackageVersionResponse[];
}

interface PublishResponse {
  package_id: number;
  version_id: number;
  existing: boolean;
}

interface RegistryListResponse {
  registries: PackageRegistry[];
}

interface FileOperationResponse {
  success: boolean;
  file_path: string;
  commit_sha: string;
}

type BranchRefResponse = string | { name: string; is_default?: boolean };
type TagRefResponse = string | { name: string };

function normalizeBranchRef(branch: BranchRefResponse): { name: string; is_default: boolean } {
  if (typeof branch === 'string') return { name: branch, is_default: false };
  return { name: branch.name, is_default: Boolean(branch.is_default) };
}

function normalizeTagRef(tag: TagRefResponse): { name: string } {
  if (typeof tag === 'string') return { name: tag };
  return { name: tag.name };
}

interface RunnerAdminResponse {
  id: number;
  name: string;
  status: string;
  labels: string | string[];
  last_seen_at: string;
  version: string | null;
  os: string | null;
  arch: string | null;
}

interface RunnerListItem {
  id: number;
  name: string;
  status: string;
  labels: string[];
  last_seen: string;
  last_seen_at: string;
  version: string | null;
  os: string | null;
  arch: string | null;
}

export interface RegisterRunnerResponse {
  id: number;
  token: string;
  message: string;
}

export interface ReleaseAsset {
  id: number;
  release_id: number;
  filename: string;
  size: number;
  content_type: string;
  download_count: number;
  uploader_id: number;
  created_at: string;
}

export interface AuthLoginResponse {
  token: string;
  user_id: number;
  username: string;
  mfa_required?: boolean;
}

function toPagination(total: number, page?: number, perPage?: number): PaginationMeta {
  const safePage = Math.max(1, Number(page ?? 1));
  const safePerPage = Number(perPage ?? 20);
  const effectivePerPage = safePerPage > 0 ? safePerPage : 20;
  const totalPages = total === 0 ? 1 : Math.max(1, Math.ceil(total / effectivePerPage));
  return {
    page: safePage,
    per_page: effectivePerPage,
    total,
    total_pages: totalPages,
    has_next: safePage < totalPages,
    has_prev: safePage > 1,
  };
}

function parseRunnerLabels(labels: string | string[] | undefined | null): string[] {
  if (Array.isArray(labels)) {
    return labels;
  }

  if (!labels || typeof labels !== 'string') {
    return [];
  }

  try {
    const parsed = JSON.parse(labels);
    if (Array.isArray(parsed)) {
      return parsed
        .filter((v) => typeof v === 'string')
        .map((v) => v as string)
        .map((v) => v.trim())
        .filter(Boolean);
    }
  } catch {
    // Keep backward compatibility with older plain string storage.
  }

  return labels
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean);
}

function parseIssueLabels(labels: string | string[] | undefined | null): string[] {
  if (Array.isArray(labels)) {
    return labels;
  }

  if (!labels || typeof labels !== 'string') {
    return [];
  }

  try {
    const parsed = JSON.parse(labels);
    if (Array.isArray(parsed)) {
      return parsed.filter((value) => typeof value === 'string');
    }
  } catch {
    // Older rows may contain comma-separated label names.
  }

  return labels
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean);
}

function normalizeIssue<T extends { labels?: string | string[] | null }>(issue: T): Omit<T, 'labels'> & { labels: string[] } {
  return {
    ...issue,
    labels: parseIssueLabels(issue.labels),
  };
}

function normalizeRunner(row: RunnerAdminResponse): RunnerListItem {
  const parsedLabels = parseRunnerLabels(row.labels);
  return {
    id: row.id,
    name: row.name,
    status: row.status,
    labels: parsedLabels,
    last_seen: row.last_seen_at,
    last_seen_at: row.last_seen_at,
    version: row.version,
    os: row.os,
    arch: row.arch,
  };
}

function contentDispositionAttachment(filename: string): string {
  return `attachment; filename*=UTF-8''${encodeURIComponent(filename || 'package')}`;
}

// ── Auth ─────────────────────────────────────────────
export const auth = {
  register: (username: string, email: string, password: string) =>
    request<{ id: number; username: string }>('/users/register', {
      method: 'POST',
      body: JSON.stringify({ username, email, password }),
    }),
  login: (username: string, password: string) =>
    request<AuthLoginResponse>('/users/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    }),
  verifyMfa: (username: string, code: string, backup = false) =>
    request<AuthLoginResponse>('/users/mfa/verify', {
      method: 'POST',
      body: JSON.stringify({ username, code, backup }),
    }),
  me: () =>
    request<{ id: number; username: string; email: string; is_admin: boolean; display_name: string | null }>('/users/me'),
  forgotPassword: (email: string) =>
    request<{ message: string }>('/users/forgot-password', {
      method: 'POST',
      body: JSON.stringify({ email }),
    }),
  resetPassword: (token: string, newPassword: string) =>
    request<{ token: string; user_id: number; username: string }>('/users/reset-password', {
      method: 'POST',
      body: JSON.stringify({ token, new_password: newPassword }),
    }),
  // M-4: Backend clears the HttpOnly auth cookie
  logout: () =>
    request<{ logged_out: boolean }>('/users/logout', {
      method: 'POST',
    }),
};

// ── Repos ────────────────────────────────────────────
export const repos = {
  list: (owner: string, page?: number, perPage?: number) =>
    request<PaginatedResponse<{ id: number; name: string; description: string | null; is_private: boolean; created_at: string }>>(
      `/repos/${owner}${qs({ page, per_page: perPage })}`
    ),
  explore: (page?: number, perPage?: number) =>
    request<PaginatedResponse<{ id: number; owner_id: number; name: string; description: string | null; stars_count: number; updated_at: string }>>(
      `/repos/explore${qs({ page, per_page: perPage })}`
    ),
  get: (owner: string, name: string) =>
    request<{
      id: number;
      name: string;
      description: string | null;
      is_private: boolean;
      default_branch: string;
      stars_count: number;
      created_at: string;
    }>(`/repos/${owner}/${name}`),
  create: (opts: {
    name: string;
    description?: string;
    is_private?: boolean;
    org?: string;
    auto_init?: boolean;
    default_branch?: string;
    gitignores?: string;
    license?: string;
    readme?: string;
    issue_labels?: string;
  }) =>
    request<{ id: number; name: string }>('/repos', {
      method: 'POST',
      body: JSON.stringify(opts),
    }),
  // Template listing
  templates: {
    gitignores: () => request<{ data: { key: string; name: string; description: string }[] }>('/repos/templates/gitignores'),
    licenses: () => request<{ data: { key: string; name: string; description: string }[] }>('/repos/templates/licenses'),
    readmes: () => request<{ data: { key: string; name: string; description: string }[] }>('/repos/templates/readmes'),
    labels: () => request<{ data: { key: string; name: string; description: string }[] }>('/repos/templates/labels'),
  },
  // Content browsing
  tree: (owner: string, repo: string, ref?: string, path?: string) => {
    return request<{ entries: { name: string; kind: string; size?: number }[] }>(`/repos/${owner}/${repo}/tree${qs({ ref, path })}`);
  },
  blob: (owner: string, repo: string, path: string, ref?: string) => {
    return request<{ path: string; content: string; size: number; name: string; sha: string; encoding: string; is_binary: boolean }>(`/repos/${owner}/${repo}/blob/${encodeRepoPath(path)}${qs({ ref })}`);
  },
  saveContent: (
    owner: string,
    repo: string,
    path: string,
    data: { branch?: string; content: string; message: string; sha?: string }
  ) =>
    request<FileOperationResponse>(`/repos/${owner}/${repo}/contents/${encodeRepoPath(path)}`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  deleteContent: (
    owner: string,
    repo: string,
    path: string,
    data: { branch?: string; message: string; sha: string }
  ) =>
    request<FileOperationResponse>(`/repos/${owner}/${repo}/contents/${encodeRepoPath(path)}${qs({
      branch: data.branch,
      message: data.message,
      sha: data.sha,
    })}`, {
      method: 'DELETE',
    }),
  log: (owner: string, repo: string, ref?: string, path?: string) => {
    return request<{ commits: { sha: string; message: string; author: string; date: string }[] }>(`/repos/${owner}/${repo}/log${qs({ ref, path })}`);
  },
  branches: (owner: string, repo: string) =>
    request<BranchRefResponse[]>(`/repos/${owner}/${repo}/branches`).then((branches) => branches.map(normalizeBranchRef)),
  tags: (owner: string, repo: string) =>
    request<TagRefResponse[]>(`/repos/${owner}/${repo}/tags`).then((tags) => tags.map(normalizeTagRef)),
  // GPG signature
  commitSignature: (owner: string, repo: string, sha: string) =>
    request<{ verified: boolean; signer_key: string | null; signer_name: string | null; signer_email: string | null; status: string }>(`/repos/${owner}/${repo}/commits/${sha}/signature`),
  // Star
  star: (owner: string, repo: string) =>
    request<{ starred: boolean }>(`/repos/${owner}/${repo}/star`, { method: 'PUT' }),
  starred: (owner: string, repo: string) =>
    request<{ starred: boolean }>(`/repos/${owner}/${repo}/starred`, { method: 'GET' }),
  unstar: async (owner: string, repo: string) => {
    const status = await repos.starred(owner, repo);
    if (!status.starred) return { starred: false };
    return repos.star(owner, repo);
  },
  stargazers: (owner: string, repo: string, page?: number, perPage?: number) =>
    request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/stargazers${qs({ page, per_page: perPage })}`),
  // Watch
  watch: (owner: string, repo: string, state: string) =>
    request<{ watch_state: string }>(`/repos/${owner}/${repo}/watch`, { method: 'PUT', body: JSON.stringify({ state }) }),
  watchStatus: (owner: string, repo: string) =>
    request<{ watch_state: 'not_watching' | 'watching' | 'ignoring' }>(`/repos/${owner}/${repo}/watch`, { method: 'GET' }),
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
    return request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/issues${qs({ state, page, per_page: perPage, labels })}`)
      .then((response) => ({
        ...response,
        data: response.data.map(normalizeIssue),
      }));
  },
  get: (owner: string, repo: string, number: number) =>
    request<any>(`/repos/${owner}/${repo}/issues/${number}`).then(normalizeIssue),
  create: (owner: string, repo: string, title: string, body?: string, labels?: string[]) =>
    request<any>(`/repos/${owner}/${repo}/issues`, {
      method: 'POST',
      body: JSON.stringify({ title, body, labels }),
    }).then(normalizeIssue),
  update: (owner: string, repo: string, number: number, data: Record<string, any>) =>
    request<any>(`/repos/${owner}/${repo}/issues/${number}`, {
      method: 'PATCH',
      body: JSON.stringify(data),
    }).then(normalizeIssue),
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
      body: JSON.stringify({
        title: data.title,
        body: data.body,
        head: data.head_branch,
        base: data.base_branch,
      }),
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
      body: JSON.stringify({ body, action: verdict }),
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
    request<any>(`/repos/${owner}/${repo}/wiki/${encodeURIComponent(title)}`),
  create: (owner: string, repo: string, title: string, content: string) =>
    request<any>(`/repos/${owner}/${repo}/wiki`, {
      method: 'POST',
      body: JSON.stringify({ title, content }),
    }),
  update: (owner: string, repo: string, title: string, content: string) =>
    request<any>(`/repos/${owner}/${repo}/wiki/${encodeURIComponent(title)}`, {
      method: 'PATCH',
      body: JSON.stringify({ content }),
    }),
  remove: (owner: string, repo: string, title: string) =>
    request<any>(`/repos/${owner}/${repo}/wiki/${encodeURIComponent(title)}`, {
      method: 'DELETE',
    }),
  history: (owner: string, repo: string, title: string) =>
    request<any[]>(`/repos/${owner}/${repo}/wiki/${encodeURIComponent(title)}/history`),
  revision: (owner: string, repo: string, title: string, revId: number) =>
    request<any>(`/repos/${owner}/${repo}/wiki/${encodeURIComponent(title)}/revisions/${revId}`),
  listRevisions: (owner: string, repo: string, title: string) =>
    request<any[]>(`/repos/${owner}/${repo}/wiki/${encodeURIComponent(title)}/history`),
  getRevision: (owner: string, repo: string, title: string, revId: number) =>
    request<any>(`/repos/${owner}/${repo}/wiki/${encodeURIComponent(title)}/revisions/${revId}`),
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
  updatePermission: (owner: string, repo: string, id: number, permission: string) =>
    request<any>(`/repos/${owner}/${repo}/collaborators/${id}`, {
      method: 'PATCH',
      body: JSON.stringify({ permission }),
    }),
  remove: (owner: string, repo: string, userId: number) =>
    request<void>(`/repos/${owner}/${repo}/collaborators/${userId}`, { method: 'DELETE' }),
};

// ── Branch Protection ───────────────────────────────
export interface BranchProtectionRule {
  id: number;
  repo_id: number;
  branch_name: string;
  require_pr: boolean;
  require_status_check: boolean;
  required_status_checks: string | null;
  require_approval: boolean;
  required_approvals: number | null;
  allow_force_push: boolean;
  allowed_push_user_ids: string | null;
  created_at: string;
  updated_at: string;
}

export interface BranchProtectionPayload {
  branch_name?: string;
  require_pr?: boolean;
  require_status_check?: boolean;
  required_status_checks?: string[];
  require_approval?: boolean;
  required_approvals?: number;
  allow_force_push?: boolean;
  allowed_push_user_ids?: number[];
}

export const branchProtections = {
  list: (owner: string, repo: string) =>
    request<BranchProtectionRule[]>(`/repos/${owner}/${repo}/branches/protection`),
  create: (owner: string, repo: string, payload: BranchProtectionPayload & { branch_name: string }) =>
    request<BranchProtectionRule>(`/repos/${owner}/${repo}/branches/protection`, {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  update: (owner: string, repo: string, id: number, payload: Omit<BranchProtectionPayload, 'branch_name'>) =>
    request<BranchProtectionRule>(`/repos/${owner}/${repo}/branches/protection/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    }),
  remove: (owner: string, repo: string, id: number) =>
    request<void>(`/repos/${owner}/${repo}/branches/protection/${id}`, { method: 'DELETE' }),
};

// ── Repository Mirror ────────────────────────────────
export interface RepositoryMirror {
  id: number;
  repo_id: number;
  url: string;
  username: string | null;
  sync_interval_seconds: number;
  next_sync_at: string | null;
  last_sync_at: string | null;
  last_sync_error: string | null;
  status: string;
  created_at: string;
  updated_at: string;
}

export interface MirrorPayload {
  url: string;
  username?: string;
  password?: string;
  sync_interval_seconds: number;
}

export const mirrors = {
  get: (owner: string, repo: string) =>
    request<RepositoryMirror>(`/repos/${owner}/${repo}/mirror`),
  create: (owner: string, repo: string, payload: MirrorPayload) =>
    request<RepositoryMirror>(`/repos/${owner}/${repo}/mirror`, {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  update: (owner: string, repo: string, payload: Partial<MirrorPayload> & { status?: string }) =>
    request<RepositoryMirror>(`/repos/${owner}/${repo}/mirror`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    }),
  remove: (owner: string, repo: string) =>
    request<void>(`/repos/${owner}/${repo}/mirror`, { method: 'DELETE' }),
  sync: (owner: string, repo: string) =>
    request<{ status: string }>(`/repos/${owner}/${repo}/mirror/sync`, { method: 'POST' }),
};

// ── Repository Imports ───────────────────────────────
export interface StartImportPayload {
  platform: 'github' | 'gitlab';
  source_url: string;
  target_owner: string;
  target_name?: string;
  auth_token?: string;
  import_repo?: boolean;
  import_issues?: boolean;
  import_pull_requests?: boolean;
  import_wiki?: boolean;
  import_releases?: boolean;
  import_labels?: boolean;
  import_milestones?: boolean;
}

export interface ImportTask {
  id: number;
  user_id: number;
  platform: string;
  source_url: string;
  target_owner: string;
  target_name: string;
  status: string;
  progress: number;
  error_message: string | null;
  created_at: string;
  updated_at: string;
}

export const imports = {
  list: () =>
    request<ImportTask[]>('/imports'),
  start: (payload: StartImportPayload) =>
    request<ImportTask>('/imports', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  get: (id: number) =>
    request<ImportTask>(`/imports/${id}`),
  remove: (id: number) =>
    request<void>(`/imports/${id}`, { method: 'DELETE' }),
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
  listAssets: (owner: string, repo: string, releaseId: number) =>
    request<ReleaseAsset[]>(`/repos/${owner}/${repo}/releases/${releaseId}/assets`),
  uploadAsset: (owner: string, repo: string, releaseId: number, file: File) =>
    request<ReleaseAsset>(`/repos/${owner}/${repo}/releases/${releaseId}/assets`, {
      method: 'POST',
      headers: {
        'Content-Type': file.type || 'application/octet-stream',
        'Content-Disposition': contentDispositionAttachment(file.name || 'asset'),
      },
      body: file,
    }),
  getAsset: (owner: string, repo: string, assetId: number) =>
    request<ReleaseAsset>(`/repos/${owner}/${repo}/releases/assets/${assetId}`),
  assetDownloadUrl: (owner: string, repo: string, assetId: number) =>
    `${API_BASE}/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/releases/assets/${assetId}/download`,
  downloadAsset: (owner: string, repo: string, assetId: number, filename: string) =>
    downloadApiFile(
      `/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/releases/assets/${assetId}/download`,
      filename || 'asset'
    ),
  deleteAsset: (owner: string, repo: string, assetId: number) =>
    request<void>(`/repos/${owner}/${repo}/releases/assets/${assetId}`, { method: 'DELETE' }),
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
    request<Array<{
      id: number;
      name: string;
      scopes: string;
      expires_at?: string | null;
      last_used_at?: string | null;
      created_at: string;
    }>>('/users/tokens'),
  create: (name: string, scopes?: string, expires_at?: string) =>
    request<{ id: number; name: string; token: string; scopes: string; expires_at?: string; created_at: string }>('/users/tokens', {
      method: 'POST',
      body: JSON.stringify({ name, scopes, expires_at }),
    }),
  delete: (id: number) =>
    request<void>(`/users/tokens/${id}`, { method: 'DELETE' }),
};

export interface MfaSetupResponse {
  secret: string;
  otpauth_url: string;
  qr_svg: string;
}

export interface MfaEnableResponse {
  enabled: boolean;
  backup_codes: string[];
}

export interface MfaBackupStatus {
  total: number;
  unused: number;
  codes: { used: boolean; used_at?: string | null; created_at: string }[];
}

export const mfa = {
  setup: () =>
    request<MfaSetupResponse>('/users/mfa/setup', { method: 'POST' }),
  enable: (code: string) =>
    request<MfaEnableResponse>('/users/mfa/enable', {
      method: 'POST',
      body: JSON.stringify({ code }),
    }),
  backup: () =>
    request<MfaBackupStatus>('/users/mfa/backup'),
  disable: (password: string) =>
    request<void>('/users/mfa/disable', {
      method: 'POST',
      body: JSON.stringify({ password }),
    }),
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
  per_page: number;
  logs: AuditLogEntry[];
}

export interface AuditLogQuery {
  page?: number;
  per_page?: number;
  user_id?: number;
  action?: string;
  resource_type?: string;
  start_time?: string;
  end_time?: string;
}

export interface AdminSettings {
  maintenance_mode: boolean;
  banner_message: string | null;
  banner_type: 'info' | 'warning' | 'error';
}

export interface AdminSsoProvider {
  id: number;
  name: string;
  slug: string;
  provider_type: string;
  client_id: string | null;
  discovery_url: string | null;
  scopes: string | null;
  ldap_host: string | null;
  ldap_port: number | null;
  ldap_bind_dn: string | null;
  ldap_base_dn: string | null;
  ldap_user_filter: string | null;
  enabled: boolean;
  icon_url: string | null;
  created_at: string;
  updated_at: string;
}

export interface SsoProviderPayload {
  name: string;
  slug: string;
  provider_type?: string;
  client_id?: string;
  client_secret?: string;
  discovery_url?: string;
  scopes?: string;
  ldap_host?: string;
  ldap_port?: number;
  ldap_bind_dn?: string;
  ldap_bind_password?: string;
  ldap_base_dn?: string;
  ldap_user_filter?: string;
  enabled?: boolean;
  icon_url?: string;
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
      per_page: query?.per_page,
      user_id: query?.user_id,
      action: query?.action,
      resource_type: query?.resource_type,
      start_time: query?.start_time,
      end_time: query?.end_time,
    })}`),
  getAuditLog: (id: number) =>
    request<AuditLogEntry>(`/admin/audit/logs/${id}`),
  getSettings: () =>
    request<AdminSettings>('/admin/settings'),
  updateSettings: (payload: Partial<AdminSettings>) =>
    request<AdminSettings>('/admin/settings', {
      method: 'PATCH',
      body: JSON.stringify(payload),
    }),
  listSsoProviders: () =>
    request<AdminSsoProvider[]>('/admin/sso/providers'),
  createSsoProvider: (payload: SsoProviderPayload) =>
    request<AdminSsoProvider>('/admin/sso/providers', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  updateSsoProvider: (id: number, payload: SsoProviderPayload) =>
    request<AdminSsoProvider>(`/admin/sso/providers/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    }),
  deleteSsoProvider: (id: number) =>
    request<{ deleted: boolean }>(`/admin/sso/providers/${id}`, { method: 'DELETE' }),
};

// ── WebSocket ────────────────────────────────────────
export interface SearchResult {
  result_type: string;
  id: number;
  title: string;
  excerpt: string | null;
  repo_owner: string | null;
  repo_name: string | null;
  state?: string | null;
  number?: number | null;
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
function filterPackagesByQuery(packages: PackageSummaryResponse[], query?: string): PackageSummaryResponse[] {
  const needle = (query || '').trim().toLowerCase();
  if (!needle) return packages;

  return packages.filter((pkg) => {
    const haystack = [
      pkg.name,
      pkg.description,
      pkg.latest_version,
      pkg.keywords,
      pkg.format,
    ]
      .filter(Boolean)
      .join(' ')
      .toLowerCase();

    return haystack.includes(needle);
  });
}

export const packages = {
  list: async (owner: string, repo: string, pkg_type?: string, page?: number, perPage?: number, query?: string) => {
    if (pkg_type) {
      const res = await request<PackageListByTypeResponse>(`/repos/${owner}/${repo}/packages/${encodeURIComponent(pkg_type)}/list`);
      const list = filterPackagesByQuery(
        (res.packages || []).map((item) => ({ ...item, format: pkg_type })),
        query,
      );
      const start = ((page ?? 1) - 1) * (perPage ?? 20);
      const size = perPage ?? 20;
      return {
        data: list.slice(start, start + size),
        pagination: toPagination(list.length, page, perPage),
      };
    }

    const reg = await request<RegistryListResponse>(`/repos/${owner}/${repo}/packages`);
    const regTypes = reg.registries.filter((r) => r.enabled).map((r) => r.package_type);

    if (regTypes.length === 0) {
      return {
        data: [] as PackageSummaryResponse[],
        pagination: toPagination(0, page, perPage),
      };
    }

    const packByType = await Promise.all(
      regTypes.map((pkg_type) => request<PackageListByTypeResponse>(`/repos/${owner}/${repo}/packages/${encodeURIComponent(pkg_type)}/list`).catch(() => ({ packages: [] })))
    );
    const list = packByType.flatMap((group, idx) =>
      (group.packages || []).map((pkg) => ({ ...pkg, format: regTypes[idx] }))
    );
    const filteredList = filterPackagesByQuery(list, query);
    const start = ((page ?? 1) - 1) * (perPage ?? 20);
    const size = perPage ?? 20;

    return {
      data: filteredList.slice(start, start + size),
      pagination: toPagination(filteredList.length, page, perPage),
    };
  },
  getFormat: (owner: string, repo: string, pkg_type: string) =>
    request<PackageListByTypeResponse>(`/repos/${owner}/${repo}/packages/${encodeURIComponent(pkg_type)}/list`),
  get: (owner: string, repo: string, pkg_type: string, pkg_name: string) =>
    request<any>(`/repos/${owner}/${repo}/packages/${encodeURIComponent(pkg_type)}/${encodeURIComponent(pkg_name)}`),
  getVersions: (owner: string, repo: string, pkg_type: string, pkg_name: string) =>
    request<VersionListByTypeResponse>(`/repos/${owner}/${repo}/packages/${encodeURIComponent(pkg_type)}/${encodeURIComponent(pkg_name)}/versions`),
  getVersion: (owner: string, repo: string, pkg_type: string, pkg_name: string, version: string) =>
    request<any>(`/repos/${owner}/${repo}/packages/${encodeURIComponent(pkg_type)}/${encodeURIComponent(pkg_name)}/${encodeURIComponent(version)}`),
  downloadUrl: (owner: string, repo: string, pkg_type: string, pkg_name: string, version: string, filename: string) =>
    withApiBase(`/repos/${owner}/${repo}/packages/${encodeURIComponent(pkg_type)}/${encodeURIComponent(pkg_name)}/${encodeURIComponent(version)}/${encodeRepoPath(filename)}`),
  publish: (owner: string, repo: string, pkg_type: string, body: Blob | string, metadata?: { name?: string; version?: string; description?: string; homepage?: string; repository_url?: string; semver?: string }) => {
    const query = qs({
      name: metadata?.name,
      version: metadata?.version,
      description: metadata?.description,
      homepage: metadata?.homepage,
      repository_url: metadata?.repository_url,
      semver: metadata?.semver,
    });
    const headers: Record<string, string> = {
      'Content-Type': 'application/octet-stream',
    };
    if (body instanceof Blob && 'name' in body) {
      const filename = (body as File).name || 'package';
      headers['Content-Disposition'] = contentDispositionAttachment(filename);
    }
    return request<PublishResponse>(`/repos/${owner}/${repo}/packages/${encodeURIComponent(pkg_type)}/publish${query}`, {
      method: 'POST',
      body: body instanceof Blob ? body : (body as string),
      headers,
    } as RequestInit);
  },
  create: (owner: string, repo: string, pkg_type: string, data: { name: string; version: string; description?: string; content_type?: string; file?: File }) =>
    request<any>(`/repos/${owner}/${repo}/packages/${encodeURIComponent(pkg_type)}/publish`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  delete: (owner: string, repo: string, pkg_type: string, pkg_name: string, version: string) =>
    request<void>(`/repos/${owner}/${repo}/packages/${encodeURIComponent(pkg_type)}/${encodeURIComponent(pkg_name)}/${encodeURIComponent(version)}`, { method: 'DELETE' }),
};

// ── Runners ──────────────────────────────────────
export const runners = {
  list: (page?: number, perPage?: number) =>
    request<RunnerAdminResponse[]>(`/admin/runners`).then((rows) => {
      const list = rows.map(normalizeRunner);
      const start = ((page ?? 1) - 1) * (perPage ?? 20);
      const size = perPage ?? 20;
      return {
        data: list.slice(start, start + size),
        pagination: toPagination(list.length, page, perPage),
      } as PaginatedResponse<RunnerListItem>;
    }),
  get: (id: number) =>
    request<RunnerAdminResponse>(`/admin/runners/${id}`).then(normalizeRunner),
  register: (data: { name: string; labels?: string[] }) =>
    request<RegisterRunnerResponse>('/runners/register', {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  delete: (id: number) =>
    request<{ deleted: boolean }>(`/admin/runners/${id}`, { method: 'DELETE' }),
};

// ── Time Tracking (issue-scoped) ──────────────────
export const timeTracking = {
  list: (owner: string, repo: string, issueNumber: number, page?: number, perPage?: number) =>
    request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/issues/${issueNumber}/time${qs({ page, per_page: perPage })}`),
  add: (owner: string, repo: string, issueNumber: number, data: { duration_minutes: number; description?: string }) =>
    request<any>(`/repos/${owner}/${repo}/issues/${issueNumber}/time`, {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  total: (owner: string, repo: string, issueNumber: number) =>
    request<{ total_minutes: number; total_formatted: string }>(`/repos/${owner}/${repo}/issues/${issueNumber}/time/total`),
  delete: (owner: string, repo: string, issueNumber: number, id: number) =>
    request<void>(`/repos/${owner}/${repo}/issues/${issueNumber}/time/${id}`, { method: 'DELETE' }),
};

// ── Project Boards ─────────────────────────────────
export const boards = {
  list: (owner: string, repo: string) =>
    request<any[]>(`/repos/${owner}/${repo}/boards`),
  get: (owner: string, repo: string, id: number) =>
    request<any>(`/repos/${owner}/${repo}/boards/${id}`),
  create: (owner: string, repo: string, data: { name: string; description?: string }) =>
    request<any>(`/repos/${owner}/${repo}/boards`, { method: 'POST', body: JSON.stringify(data) }),
  update: (owner: string, repo: string, id: number, data: { name?: string; description?: string }) =>
    request<any>(`/repos/${owner}/${repo}/boards/${id}`, { method: 'PATCH', body: JSON.stringify(data) }),
  delete: (owner: string, repo: string, id: number) =>
    request<{ deleted: boolean }>(`/repos/${owner}/${repo}/boards/${id}`, { method: 'DELETE' }),
  // Column CRUD
  createColumn: (owner: string, repo: string, boardId: number, data: { name: string }) =>
    request<any>(`/repos/${owner}/${repo}/boards/${boardId}/columns`, { method: 'POST', body: JSON.stringify(data) }),
  updateColumn: (owner: string, repo: string, boardId: number, colId: number, data: { name?: string }) =>
    request<any>(`/repos/${owner}/${repo}/boards/${boardId}/columns/${colId}`, { method: 'PATCH', body: JSON.stringify(data) }),
  deleteColumn: (owner: string, repo: string, boardId: number, colId: number) =>
    request<{ deleted: boolean }>(`/repos/${owner}/${repo}/boards/${boardId}/columns/${colId}`, { method: 'DELETE' }),
  // Card CRUD
  createCard: (owner: string, repo: string, boardId: number, colId: number, data: { note?: string; issue_id?: number }) =>
    request<any>(`/repos/${owner}/${repo}/boards/${boardId}/columns/${colId}/cards`, { method: 'POST', body: JSON.stringify(data) }),
  updateCard: (owner: string, repo: string, boardId: number, cardId: number, data: { note?: string; issue_id?: number | null }) =>
    request<any>(`/repos/${owner}/${repo}/boards/${boardId}/cards/${cardId}`, { method: 'PATCH', body: JSON.stringify(data) }),
  moveCard: (owner: string, repo: string, boardId: number, cardId: number, data: { column_id: number; position: number }) =>
    request<any>(`/repos/${owner}/${repo}/boards/${boardId}/cards/${cardId}/move`, { method: 'POST', body: JSON.stringify(data) }),
  reorderCards: (owner: string, repo: string, boardId: number, data: { positions: [number, number][] }) =>
    request<{ status: string }>(`/repos/${owner}/${repo}/boards/${boardId}/cards/reorder`, { method: 'POST', body: JSON.stringify(data) }),
  deleteCard: (owner: string, repo: string, boardId: number, cardId: number) =>
    request<{ deleted: boolean }>(`/repos/${owner}/${repo}/boards/${boardId}/cards/${cardId}`, { method: 'DELETE' }),
};

export function connectNotificationWebSocket(
  onMessage: (event: { event_type: string; data: any }) => void,
  onError?: (err: Event) => void,
): WebSocket | null {
  // M-4: WebSocket auth via HttpOnly cookie — the browser sends the cookie
  // automatically for same-origin WebSocket connections. No token needed in JS.
  // The backend checks the cookie header before upgrading.
  const wsUrl = `${withWebSocketApiBase('/ws/notifications')}`;
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
