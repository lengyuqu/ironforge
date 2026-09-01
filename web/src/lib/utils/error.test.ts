import { describe, it, expect } from 'vitest';
import { toErrorMessage } from '$lib/utils/error';

describe('toErrorMessage', () => {
  it('returns Error.message for standard Error instances', () => {
    expect(toErrorMessage(new Error('boom'))).toBe('boom');
  });

  it('falls back to the provided default on empty message', () => {
    expect(toErrorMessage(new Error(''), 'oops')).toBe('oops');
    expect(toErrorMessage(undefined, 'oops')).toBe('oops');
    expect(toErrorMessage(null, 'oops')).toBe('oops');
  });

  it('appends code and request_id when attached to Error', () => {
    const err = Object.assign(new Error('forbidden'), {
      code: 'ERR_ACCESS_DENIED',
      request_id: 'req_123',
    });
    expect(toErrorMessage(err)).toContain('forbidden');
    expect(toErrorMessage(err)).toContain('(ERR_ACCESS_DENIED)');
    expect(toErrorMessage(err)).toContain('[request req_123]');
  });

  it('passes through primitives directly', () => {
    expect(toErrorMessage('nope')).toBe('nope');
    expect(toErrorMessage(42)).toBe('42');
    expect(toErrorMessage(true)).toBe('true');
  });

  it('reads message/error from duck-typed plain objects', () => {
    expect(toErrorMessage({ message: 'oops' })).toBe('oops');
    expect(toErrorMessage({ error: 'server_err' })).toBe('server_err');
    expect(toErrorMessage({})).toBe('Unknown error');
  });
});
