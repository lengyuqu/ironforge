// IronForge API Client — shared internals

const configuredApiBase =
  typeof import.meta !== 'undefined'
    ? (import.meta as { env?: { VITE_API_BASE?: string } }).env?.VITE_API_BASE
    : undefined;

function normalizeApiBase(value?: string): string {
  if (!value) return '/api/v1';
  const trimmed = value.trim();
  if (!trimmed || trimmed === '/') return '/api/v1';
  return trimmed.endsWith('/') ? trimmed.slice(0, -1) : trimmed;
}

export const API_BASE = normalizeApiBase(configuredApiBase);

function withApiBase(path: string): string {
  return `${API_BASE}${path.startsWith('/') ? path : `/${path}`}`;
}

let authToken = $state<string | null>(null);

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

export async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const token = getToken();
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string> || {}),
  };
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }

  const res = await fetch(withApiBase(path), { ...options, headers });

  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    // Backend error envelope is { error: { code, message, request_id } }.
    // Reading body.error directly yields "[object Object]" in the UI, so pull
    // out the human-readable message (falling back for older/plain shapes).
    const msg =
      (body?.error && typeof body.error === 'object' ? body.error.message : body?.error) ||
      body?.message ||
      `HTTP ${res.status}`;
    throw new Error(msg);
  }

  return res.json();
}

export function qs(params: Record<string, string | number | boolean | undefined | null>): string {
  const parts = Object.entries(params)
    .filter(([, v]) => v !== undefined && v !== null && v !== '')
    .map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(String(v))}`);
  return parts.length > 0 ? '?' + parts.join('&') : '';
}

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
