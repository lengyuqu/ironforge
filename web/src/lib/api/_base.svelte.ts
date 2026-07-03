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

export function withApiBase(path: string): string {
  return `${API_BASE}${path.startsWith('/') ? path : `/${path}`}`;
}

export function withBackendBase(path: string): string {
  const backendBase = API_BASE.replace(/\/api\/v1$/, '');
  return `${backendBase}${path.startsWith('/') ? path : `/${path}`}`;
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

  if (res.status === 204) {
    return undefined as T;
  }

  const text = await res.text();
  if (!text.trim()) {
    return undefined as T;
  }

  return JSON.parse(text) as T;
}

function filenameFromContentDisposition(value: string | null): string | null {
  if (!value) return null;

  const encoded = value.match(/filename\*=UTF-8''([^;]+)/i);
  if (encoded?.[1]) {
    try {
      return decodeURIComponent(encoded[1]);
    } catch {
      return encoded[1];
    }
  }

  const quoted = value.match(/filename="([^"]+)"/i);
  if (quoted?.[1]) return quoted[1];

  const plain = value.match(/filename=([^;]+)/i);
  return plain?.[1]?.trim() || null;
}

export async function downloadApiFile(path: string, fallbackFilename: string): Promise<void> {
  const token = getToken();
  const headers: Record<string, string> = {};
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }

  const res = await fetch(withApiBase(path), { headers });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    const msg =
      (body?.error && typeof body.error === 'object' ? body.error.message : body?.error) ||
      body?.message ||
      `HTTP ${res.status}`;
    throw new Error(msg);
  }

  const blob = await res.blob();
  const filename = filenameFromContentDisposition(res.headers.get('content-disposition')) || fallbackFilename;
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.style.display = 'none';
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
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
