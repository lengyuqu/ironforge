import { describe, it, expect, vi, afterEach } from 'vitest';
import { ApiError } from './_base.svelte';
import { collabGroups } from './collabGroups';
import { collabHealth } from './collabHealth';

// Contract: only user-side endpoints, login cookie (no session headers), path
// segments encoded, the server's machine code surfaced.
describe('collabGroups / collabHealth clients', () => {
  const originalFetch = globalThis.fetch;
  afterEach(() => {
    globalThis.fetch = originalFetch;
    vi.restoreAllMocks();
  });
  function mockFetch(status: number, body: unknown) {
    const fn = vi.fn().mockResolvedValue(
      new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } }),
    );
    globalThis.fetch = fn as unknown as typeof fetch;
    return fn;
  }
  const call = (fn: ReturnType<typeof vi.fn>) => ({
    url: fn.mock.calls[0][0] as string,
    init: fn.mock.calls[0][1] as RequestInit,
  });

  it('creates a group with a POST body and the login cookie, no session header', async () => {
    const fn = mockFetch(201, { id: 'g1' });
    await collabGroups.create('alice', 'demo', { client_request_id: 'k1', title: 'T', description: '' });
    const { url, init } = call(fn);
    expect(url).toBe('/api/v1/repos/alice/demo/collaboration/groups');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string)).toEqual({ client_request_id: 'k1', title: 'T', description: '' });
    expect(init.credentials).toBe('include');
    const names = Object.keys(init.headers as Record<string, string>).map((k) => k.toLowerCase());
    expect(names).not.toContain('x-ironforge-session');
  });

  it('encodes ids and the cursor, and removes a link with DELETE + a body', async () => {
    const fn = mockFetch(200, {});
    await collabGroups.listLinks('a b', 'r', 'g/1', { limit: 10, cursor: 'a+b=' });
    expect(call(fn).url).toBe('/api/v1/repos/a%20b/r/collaboration/groups/g%2F1/links?limit=10&cursor=a%2Bb%3D');

    const fn2 = mockFetch(200, {});
    await collabGroups.removeLink('alice', 'demo', 'g1', 'l1', { client_request_id: 'k', expected_group_version: 3 });
    const { url, init } = call(fn2);
    expect(url).toBe('/api/v1/repos/alice/demo/collaboration/groups/g1/links/l1');
    expect(init.method).toBe('DELETE');
    expect(JSON.parse(init.body as string)).toEqual({ client_request_id: 'k', expected_group_version: 3 });
  });

  it('surfaces the coded conflict', async () => {
    mockFetch(409, { error: { code: 'VERSION_CONFLICT', message: 'moved' } });
    await expect(
      collabGroups.addLink('alice', 'demo', 'g1', { client_request_id: 'k', target_repo_id: 2, target_task_id: 't', expected_group_version: 1 }),
    ).rejects.toMatchObject({ status: 409, code: 'VERSION_CONFLICT' });
    expect(new ApiError('x', 409, 'VERSION_CONFLICT').code).toBe('VERSION_CONFLICT');
  });

  it('reads health with a window, drills with a cursor and posts the baseline without a body', async () => {
    const fn = mockFetch(200, {});
    await collabHealth.get('alice', 'demo', { from: '2026-11-01T00:00:00.000Z' });
    expect(call(fn).url).toBe('/api/v1/repos/alice/demo/collaboration/health?from=2026-11-01T00%3A00%3A00.000Z');

    const fn2 = mockFetch(200, {});
    await collabHealth.drill('alice', 'demo', { metric: 'blocked_at_to', limit: 25, cursor: 'c' });
    expect(call(fn2).url).toBe('/api/v1/repos/alice/demo/collaboration/health/tasks?metric=blocked_at_to&limit=25&cursor=c');

    const fn3 = mockFetch(200, {});
    await collabHealth.takeBaseline('alice', 'demo');
    const { url, init } = call(fn3);
    expect(url).toBe('/api/v1/repos/alice/demo/collaboration/health/baseline');
    expect(init.method).toBe('POST');
    expect(init.body).toBeUndefined();
  });
});
