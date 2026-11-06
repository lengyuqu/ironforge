import { describe, it, expect, vi, afterEach } from 'vitest';
import { ApiError } from './_base.svelte';
import { collabDecisions } from './collabDecisions';

// Contract: user-door endpoints only (login cookie, never session headers),
// path segments encoded, machine error code surfaced.
describe('collabDecisions client', () => {
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

  it('creates a decision with a JSON POST', async () => {
    const fn = mockFetch(201, {});
    await collabDecisions.createDecision('a b', 'r', 't/1', {
      client_decision_id: 'k', title: 'T', rationale: 'R', context: { task_version: 3 },
    });
    const { url, init } = call(fn);
    expect(url).toBe('/api/v1/repos/a%20b/r/collaboration/tasks/t%2F1/decisions');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string)).toMatchObject({ client_decision_id: 'k', context: { task_version: 3 } });
    noSessionHeaders(init);
  });

  it('changes a decision at its changes endpoint', async () => {
    const fn = mockFetch(201, {});
    await collabDecisions.changeDecision('o', 'r', 'd1', { client_change_id: 'k', kind: 'revoked', reason: 'x' });
    const { url, init } = call(fn);
    expect(url).toBe('/api/v1/repos/o/r/collaboration/decisions/d1/changes');
    expect(init.method).toBe('POST');
    noSessionHeaders(init);
  });

  it('creates a question as the user', async () => {
    const fn = mockFetch(201, {});
    await collabDecisions.createQuestion('o', 'r', 't1', {
      client_question_id: 'k', recipient: { kind: 'repo_writers' }, title: 'T', body: 'B', options: [],
    });
    const { url, init } = call(fn);
    expect(url).toBe('/api/v1/repos/o/r/collaboration/tasks/t1/questions');
    noSessionHeaders(init);
  });

  it('reads work packages: list with cursor and single', async () => {
    const fn = mockFetch(200, { items: [], next_cursor: null });
    await collabDecisions.listWorkPackages('o', 'r', 't1', { limit: 25, cursor: 'a+b' });
    expect(call(fn).url).toBe('/api/v1/repos/o/r/collaboration/tasks/t1/work-packages?limit=25&cursor=a%2Bb');
    const fn2 = mockFetch(200, {});
    await collabDecisions.getWorkPackage('o', 'r', 'p1');
    expect(call(fn2).url).toBe('/api/v1/repos/o/r/collaboration/work-packages/p1');
  });

  it('surfaces the machine error code', async () => {
    mockFetch(409, { error: { code: 'DECISION_ALREADY_CHANGED', message: 'already' } });
    const err = await collabDecisions.changeDecision('o', 'r', 'd', { client_change_id: 'k', kind: 'revoked', reason: 'x' }).catch((e) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err.code).toBe('DECISION_ALREADY_CHANGED');
  });
});
