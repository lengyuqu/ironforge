import { describe, it, expect } from 'vitest';
import { ApiError } from '$lib/api/_base.svelte';
import { byteLength, errorText, explainKey, isDenied, isValidAmount, isValidToken, localToIso, mustReload } from './collabRelations';

describe('collabRelations helpers', () => {
  it('explains documented coded errors per context, then 403/404, otherwise nothing', () => {
    expect(explainKey('deps', new ApiError('x', 409, 'DEPENDENCY_CYCLE'))).toBe('collab_relations.err_deps.DEPENDENCY_CYCLE');
    expect(explainKey('reassign', new ApiError('x', 409, 'DEPENDENCY_CYCLE'))).toBeNull();
    expect(explainKey('budget', new ApiError('x', 409, 'BUDGET_LIMIT_REACHED'))).toBe('collab_relations.err_budget.BUDGET_LIMIT_REACHED');
    expect(explainKey('deps', new ApiError('x', 403))).toBe('collab_relations.err_forbidden');
    expect(explainKey('deps', new ApiError('x', 404))).toBe('collab_relations.err_not_found');
    expect(explainKey('deps', new ApiError('x', 500))).toBeNull();
    expect(explainKey('deps', new Error('x'))).toBeNull();
  });

  it('keeps the server message next to the explanation', () => {
    const t = (k: string) => k;
    expect(errorText('deps', new ApiError('would cycle', 409, 'DEPENDENCY_CYCLE'), t)).toBe(
      'collab_relations.err_deps.DEPENDENCY_CYCLE — would cycle (DEPENDENCY_CYCLE)',
    );
    expect(errorText('deps', new ApiError('boom', 500), t)).toBe('boom');
  });

  it('reloads only on VERSION_CONFLICT; denial covers 401 and 403', () => {
    expect(mustReload(new ApiError('x', 409, 'VERSION_CONFLICT'))).toBe(true);
    expect(mustReload(new ApiError('x', 409, 'DEPENDENCY_CYCLE'))).toBe(false);
    expect(isDenied(new ApiError('x', 401))).toBe(true);
    expect(isDenied(new ApiError('x', 403))).toBe(true);
    expect(isDenied(new ApiError('x', 404))).toBe(false);
  });

  it('mirrors the server amount and token grammar', () => {
    for (const ok of ['0', '12', '0.5', '123456789012345678', '1.123456']) expect(isValidAmount(ok)).toBe(true);
    for (const bad of ['', '-1', '1e3', '01', '1.', '.5', '1,000', '1.1234567', '1234567890123456789']) expect(isValidAmount(bad)).toBe(false);
    expect(isValidToken('USD', 32)).toBe(true);
    expect(isValidToken('a.b_c:d/e@f+g-h', 32)).toBe(true);
    expect(isValidToken('US D', 32)).toBe(false);
    expect(isValidToken('', 32)).toBe(false);
    expect(isValidToken('x'.repeat(33), 32)).toBe(false);
  });

  it('measures bytes, not characters, and converts local times to UTC ISO', () => {
    expect(byteLength('é')).toBe(2);
    expect(localToIso('')).toBeNull();
    expect(localToIso('nonsense')).toBeNull();
    expect(localToIso('2026-11-05T09:00')).toMatch(/^2026-11-05T\d\d:00:00\.000Z$|^2026-11-0[45]T/);
  });
});
