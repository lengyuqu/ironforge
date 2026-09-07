import { describe, it, expect, vi, afterEach } from 'vitest';
import { request, ApiError, UNAUTHORIZED_EVENT } from './_base.svelte';

// Session-expiry contract (M-4 follow-up): when a non-auth API call answers
// 401, `request()` must fire UNAUTHORIZED_EVENT exactly once (the root layout
// listens for it, clears auth state and redirects to /login) and reject with
// an ApiError carrying the status. Auth endpoints answer 401 during normal
// flows (wrong password, expired MFA challenge) and must NOT fire the event.
describe('request() 401 handling', () => {
  const originalFetch = globalThis.fetch;

  afterEach(() => {
    globalThis.fetch = originalFetch;
    vi.restoreAllMocks();
  });

  function mockFetch(status: number, body: unknown) {
    return vi.fn().mockResolvedValue(
      new Response(JSON.stringify(body), {
        status,
        headers: { 'Content-Type': 'application/json' },
      }),
    );
  }

  it('fires UNAUTHORIZED_EVENT and rejects with ApiError(401) for non-auth paths', async () => {
    globalThis.fetch = mockFetch(401, { error: { message: 'authentication required' } });
    const fired = vi.fn();
    window.addEventListener(UNAUTHORIZED_EVENT, fired);
    try {
      await expect(request('/repos/alice/demo')).rejects.toMatchObject({
        name: 'ApiError',
        status: 401,
        message: 'authentication required',
      });
      expect(fired).toHaveBeenCalledTimes(1);
    } finally {
      window.removeEventListener(UNAUTHORIZED_EVENT, fired);
    }
  });

  it('does not fire UNAUTHORIZED_EVENT for auth paths', async () => {
    globalThis.fetch = mockFetch(401, { error: { message: 'bad credentials' } });
    const fired = vi.fn();
    window.addEventListener(UNAUTHORIZED_EVENT, fired);
    try {
      await expect(request('/auth/login')).rejects.toBeInstanceOf(ApiError);
      expect(fired).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener(UNAUTHORIZED_EVENT, fired);
    }
  });

  it('does not fire UNAUTHORIZED_EVENT for non-401 errors', async () => {
    globalThis.fetch = mockFetch(500, { error: { message: 'boom' } });
    const fired = vi.fn();
    window.addEventListener(UNAUTHORIZED_EVENT, fired);
    try {
      await expect(request('/repos/alice/demo')).rejects.toMatchObject({
        status: 500,
        message: 'boom',
      });
      expect(fired).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener(UNAUTHORIZED_EVENT, fired);
    }
  });

  it('extracts the backend error envelope message into ApiError', async () => {
    globalThis.fetch = mockFetch(403, { error: { code: 'forbidden', message: 'no access' } });
    const err = await request('/repos/alice/demo').catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect((err as ApiError).message).toBe('no access');
    expect((err as ApiError).status).toBe(403);
  });
});
