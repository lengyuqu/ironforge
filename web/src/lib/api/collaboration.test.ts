import { describe, it, expect, vi, afterEach } from 'vitest';
import { ApiError } from './_base.svelte';
import { collaboration, loadSessionMap } from './collaboration';

// Contract: the client talks only to the user-side collaboration endpoints,
// encodes path segments, sends the login cookie (never session headers) and
// surfaces the server's machine error code.
describe('collaboration client', () => {
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

  it('lists tasks with status filter and cursor, omitting empty params', async () => {
    const fn = mockFetch(200, { items: [], next_cursor: null });
    await collaboration.listTasks('alice', 'demo', { status: 'blocked', cursor: 'a+b/c=' });
    const { url, init } = call(fn);
    expect(url).toBe('/api/v1/repos/alice/demo/collaboration/tasks?status=blocked&cursor=a%2Bb%2Fc%3D');
    expect(init.credentials).toBe('include');
    const headers = init.headers as Record<string, string>;
    expect(Object.keys(headers).map((k) => k.toLowerCase())).not.toContain('x-ironforge-session');
    expect(Object.keys(headers).map((k) => k.toLowerCase())).not.toContain('x-ironforge-session-secret');
  });

  it('encodes owner, repo and ids in the path', async () => {
    const fn = mockFetch(200, {});
    await collaboration.getTask('a b', 'r/x', 'id?1');
    expect(call(fn).url).toBe('/api/v1/repos/a%20b/r%2Fx/collaboration/tasks/id%3F1');
  });

  it('pages messages by after_seq', async () => {
    const fn = mockFetch(200, { items: [], next_after_seq: 0, has_more: false });
    await collaboration.listMessages('o', 'r', 't1', { afterSeq: 12, limit: 50 });
    expect(call(fn).url).toBe('/api/v1/repos/o/r/collaboration/tasks/t1/messages?after_seq=12&limit=50');
  });

  it('sends an answer as a JSON POST to the answer endpoint', async () => {
    const fn = mockFetch(200, {});
    const input = {
      client_answer_id: 'k1',
      expected_version: 2,
      body: 'ship it',
      option_id: null,
      acknowledged_context_hash: 'h',
    };
    await collaboration.answerQuestion('o', 'r', 'q1', input);
    const { url, init } = call(fn);
    expect(url).toBe('/api/v1/repos/o/r/collaboration/questions/q1/answer');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string)).toEqual(input);
  });

  it('cancels with the expected version', async () => {
    const fn = mockFetch(200, {});
    await collaboration.cancelQuestion('o', 'r', 'q1', 5);
    const { url, init } = call(fn);
    expect(url).toBe('/api/v1/repos/o/r/collaboration/questions/q1/cancel');
    expect(JSON.parse(init.body as string)).toEqual({ expected_version: 5 });
  });

  it('asks diagnostics for exactly one task id', async () => {
    const fn = mockFetch(200, {});
    await collaboration.diagnoseTask('o', 'r', 't1');
    expect(call(fn).url).toBe('/api/v1/repos/o/r/collaboration/diagnostics?task_id=t1');
  });

  it('requests the overview sections as a comma list', async () => {
    const fn = mockFetch(200, {});
    await collaboration.overview('o', 'r', { sections: ['blocked_waiting', 'in_progress'], limit: 20 });
    expect(call(fn).url).toBe(
      '/api/v1/repos/o/r/collaboration/overview?sections=blocked_waiting%2Cin_progress&limit=20',
    );
  });

  it('surfaces the machine error code on ApiError', async () => {
    mockFetch(409, { error: { code: 'CONTEXT_STALE', message: 'context changed' } });
    const err = await collaboration.answerQuestion('o', 'r', 'q', {
      client_answer_id: 'k',
      expected_version: 1,
      body: 'x',
      acknowledged_context_hash: 'h',
    }).catch((e) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err.status).toBe(409);
    expect(err.code).toBe('CONTEXT_STALE');
    expect(err.message).toBe('context changed');
  });

  it('leaves code undefined for plain error bodies', async () => {
    mockFetch(500, { error: 'boom' });
    const err = await collaboration.getTask('o', 'r', 't').catch((e) => e);
    expect(err.code).toBeUndefined();
    expect(err.message).toBe('boom');
  });

  it('loadSessionMap indexes sessions across pages and stops at the last page', async () => {
    const page = (items: { id: string }[], next: string | null) =>
      new Response(JSON.stringify({ items, next_cursor: next }), { status: 200 });
    const fn = vi
      .fn()
      .mockResolvedValueOnce(page([{ id: 's1' }], 'c2'))
      .mockResolvedValueOnce(page([{ id: 's2' }], null));
    globalThis.fetch = fn as unknown as typeof fetch;
    const map = await loadSessionMap('o', 'r');
    expect([...map!.keys()]).toEqual(['s1', 's2']);
    expect(fn).toHaveBeenCalledTimes(2);
    expect(fn.mock.calls[1][0]).toContain('cursor=c2');
  });

  it('loadSessionMap returns null when listing sessions is forbidden', async () => {
    mockFetch(403, { error: { message: 'write access denied' } });
    expect(await loadSessionMap('o', 'r')).toBeNull();
  });

  it('loadSessionMap rethrows other failures', async () => {
    mockFetch(500, { error: { message: 'boom' } });
    await expect(loadSessionMap('o', 'r')).rejects.toMatchObject({ status: 500 });
  });
});
