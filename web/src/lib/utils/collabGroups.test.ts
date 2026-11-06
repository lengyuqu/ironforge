import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  forget,
  groupProblem,
  isFullSha,
  linkProblem,
  loadRemembered,
  parseRepoRef,
  remember,
} from './collabGroups';
import { formatDuration, valueRows, windowFrom } from './collabHealth';

describe('group input rules', () => {
  it('parses owner/name only in that exact shape', () => {
    expect(parseRepoRef(' alice/demo ')).toEqual({ owner: 'alice', name: 'demo' });
    for (const bad of ['', 'alice', 'a/b/c', '/x', 'x/', 'a b/c']) expect(parseRepoRef(bad)).toBeNull();
  });

  it('accepts only full 40/64-digit commit ids', () => {
    expect(isFullSha('a'.repeat(40))).toBe(true);
    expect(isFullSha('A'.repeat(64))).toBe(true);
    expect(isFullSha('abc123')).toBe(false);
    expect(isFullSha('g'.repeat(40))).toBe(false);
  });

  it('needs a valid target and a task or a commit', () => {
    expect(linkProblem('nope', 't', '')).toBe('target_invalid');
    expect(linkProblem('a/b', '', '')).toBe('need_one');
    expect(linkProblem('a/b', '', 'abc')).toBe('sha_invalid');
    expect(linkProblem('a/b', 'task', '')).toBeNull();
    expect(linkProblem('a/b', '', 'a'.repeat(40))).toBeNull();
  });

  it('checks title and description limits', () => {
    expect(groupProblem('  ', '')).toBe('title_required');
    expect(groupProblem('x'.repeat(201), '')).toBe('title_too_long');
    expect(groupProblem('ok', 'x'.repeat(16 * 1024 + 1))).toBe('description_too_long');
    expect(groupProblem('ok', '')).toBeNull();
  });
});

describe('remembered groups (browser-local)', () => {
  beforeEach(() => localStorage.clear());

  it('remembers most recent first, de-duplicates and forgets', () => {
    remember('a', 'r', { id: '1', title: 'one' });
    remember('a', 'r', { id: '2', title: 'two' });
    remember('a', 'r', { id: '1', title: 'one again' });
    expect(loadRemembered('a', 'r').map((g) => g.id)).toEqual(['1', '2']);
    expect(loadRemembered('a', 'other')).toEqual([]);
    forget('a', 'r', '1');
    expect(loadRemembered('a', 'r').map((g) => g.id)).toEqual(['2']);
  });

  it('ignores corrupt storage and survives blocked storage', () => {
    localStorage.setItem('ironforge:collab-groups:a/r', '{"not":"a list"}');
    expect(loadRemembered('a', 'r')).toEqual([]);
    localStorage.setItem('ironforge:collab-groups:a/r', JSON.stringify([{ id: 1 }, { id: 'ok', title: 't' }]));
    expect(loadRemembered('a', 'r')).toEqual([{ id: 'ok', title: 't' }]);
    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    });
    expect(loadRemembered('a', 'r')).toEqual([]);
    expect(() => remember('a', 'r', { id: 'x', title: 'y' })).not.toThrow();
    vi.unstubAllGlobals();
  });
});

describe('health helpers', () => {
  it('formats durations', () => {
    expect(formatDuration(0)).toBe('0s');
    expect(formatDuration(3725)).toBe('1h 2m 5s');
    expect(formatDuration(90_000)).toBe('1d 1h');
  });

  it('computes a window start in the past', () => {
    expect(windowFrom(7, new Date('2026-11-08T00:00:00Z'))).toBe('2026-11-01T00:00:00.000Z');
  });

  it('gives an unknown metric no rows, so a zero can never be invented', () => {
    expect(valueRows(null)).toEqual([]);
    const rows = valueRows({ blocked_seconds: 60, tasks: 0, covered_from: '2026-11-01T00:00:00Z', unit: 'seconds' });
    expect(rows.map((r) => [r.key, r.kind])).toEqual([
      ['blocked_seconds', 'seconds'],
      ['tasks', 'number'],
      ['covered_from', 'time'],
    ]);
  });
});

describe('seconds keys', () => {
  it('recognises every seconds field the server sends', async () => {
    const { isSecondsKey } = await import('./collabHealth');
    for (const k of ['blocked_seconds', 'waiting_seconds', 'open_wait_seconds_at_to', 'wait_seconds_in_window', 'blocked_seconds_in_window']) {
      expect(isSecondsKey(k), k).toBe(true);
    }
    for (const k of ['tasks', 'intervals', 'entered_blocked_events']) expect(isSecondsKey(k), k).toBe(false);
  });
});
