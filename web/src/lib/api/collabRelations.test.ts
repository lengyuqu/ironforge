import { describe, it, expect, vi, afterEach } from 'vitest';
import { ApiError } from './_base.svelte';
import { collabRelations } from './collabRelations';

// Contract: user-door calls only (login cookie, never session headers), exact
// verbs and paths, encoded path segments, and the server's machine error code.
describe('collabRelations client', () => {
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
  const noSessionHeaders = (init: RequestInit) => {
    const keys = Object.keys(init.headers as Record<string, string>).map((k) => k.toLowerCase());
    expect(keys).not.toContain('x-ironforge-session');
    expect(keys).not.toContain('x-ironforge-session-secret');
    expect(init.credentials).toBe('include');
  };

  it('adds a dependency with POST and the exact body', async () => {
    const fn = mockFetch(201, { dependency: {}, graph_version: 2, replayed: false });
    const input = { client_request_id: 'k1', depends_on_task_id: 'd', expected_graph_version: 1 };
    await collabRelations.addDependency('alice', 'demo', 'task/1', input);
    const { url, init } = call(fn);
    expect(url).toBe('/api/v1/repos/alice/demo/collaboration/tasks/task%2F1/dependencies');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string)).toEqual(input);
    noSessionHeaders(init);
  });

  it('removes a dependency with DELETE and a JSON body (the server requires one)', async () => {
    const fn = mockFetch(200, { removed: true, graph_version: 3 });
    await collabRelations.removeDependency('alice', 'demo', 't1', 'd 2', { client_request_id: 'k', expected_graph_version: 2 });
    const { url, init } = call(fn);
    expect(url).toBe('/api/v1/repos/alice/demo/collaboration/tasks/t1/dependencies/d%202');
    expect(init.method).toBe('DELETE');
    expect(JSON.parse(init.body as string)).toEqual({ client_request_id: 'k', expected_graph_version: 2 });
    noSessionHeaders(init);
  });

  it('reassigns with POST /reassign and a null expected owner for an unowned task', async () => {
    const fn = mockFetch(201, { task: {}, takeover: {}, replayed: false });
    await collabRelations.reassignTask('alice', 'demo', 't1', {
      client_takeover_id: 'k', expected_task_version: 2, expected_owner_session_id: null, to_session_id: 's', reason: 'r',
    });
    const { url, init } = call(fn);
    expect(url).toBe('/api/v1/repos/alice/demo/collaboration/tasks/t1/reassign');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string).expected_owner_session_id).toBeNull();
    noSessionHeaders(init);
  });

  it('pages takeovers and usage with cursor and filters, omitting empty params', async () => {
    let fn = mockFetch(200, { items: [], next_cursor: null });
    await collabRelations.listTakeovers('alice', 'demo', 't1', { limit: 25, cursor: 'a+b' });
    expect(call(fn).url).toBe('/api/v1/repos/alice/demo/collaboration/tasks/t1/takeovers?limit=25&cursor=a%2Bb');
    fn = mockFetch(200, { items: [], next_cursor: null });
    await collabRelations.listUsage('alice', 'demo', 't1', { unit: 'USD', source: '' });
    expect(call(fn).url).toBe('/api/v1/repos/alice/demo/collaboration/tasks/t1/usage?unit=USD');
  });

  it('reads the budget and writes it with PUT /budgets, usage with POST /usage', async () => {
    let fn = mockFetch(200, { budgets: [] });
    await collabRelations.getBudget('alice', 'demo', 't1');
    expect(call(fn).url).toBe('/api/v1/repos/alice/demo/collaboration/tasks/t1/budget');
    fn = mockFetch(200, {});
    await collabRelations.setBudget('alice', 'demo', 't1', { expected_version: 0, unit: 'USD', source: 's', amount: '5', sampling_period: 'p' });
    expect(call(fn).url).toBe('/api/v1/repos/alice/demo/collaboration/tasks/t1/budgets');
    expect(call(fn).init.method).toBe('PUT');
    fn = mockFetch(201, {});
    await collabRelations.reportUsage('alice', 'demo', 't1', {
      client_usage_id: 'k', unit: 'USD', source: 's', amount: '1', period_start: '2026-11-05T09:00:00.000Z', period_end: '2026-11-05T10:00:00.000Z',
    });
    expect(call(fn).url).toBe('/api/v1/repos/alice/demo/collaboration/tasks/t1/usage');
    expect(call(fn).init.method).toBe('POST');
  });

  it('surfaces the server error code', async () => {
    mockFetch(409, { error: { code: 'DEPENDENCY_CYCLE', message: 'cycle' } });
    const err = await collabRelations
      .addDependency('a', 'b', 't', { client_request_id: 'k', depends_on_task_id: 'd', expected_graph_version: 0 })
      .catch((e) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err.status).toBe(409);
    expect(err.code).toBe('DEPENDENCY_CYCLE');
  });
});
