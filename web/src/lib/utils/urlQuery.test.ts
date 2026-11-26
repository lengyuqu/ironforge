import { describe, expect, it } from 'vitest';
import { buildQueryHref, readEnumParam, readPageParam } from './urlQuery';

describe('urlQuery', () => {
  describe('readPageParam', () => {
    it('returns positive integer page numbers', () => {
      expect(readPageParam(new URL('http://localhost/issues?page=1'))).toBe(1);
      expect(readPageParam(new URL('http://localhost/issues?page=3'))).toBe(3);
      expect(readPageParam(new URL('http://localhost/issues?p=7'), 'p')).toBe(7);
    });

    it.each(['', '0', '-1', 'abc', '2.5', '1e2', ' 2 '])(
      'falls back to 1 for invalid value %j',
      (val) => {
        expect(readPageParam(new URL(`http://localhost/issues?page=${encodeURIComponent(val)}`))).toBe(1);
      },
    );

    it('falls back to 1 when the parameter is missing', () => {
      expect(readPageParam(new URL('http://localhost/issues'))).toBe(1);
    });
  });

  describe('readEnumParam', () => {
    const allowed = ['open', 'closed', 'all'] as const;

    it('returns the matching allowed value', () => {
      expect(readEnumParam(new URL('http://localhost/issues?state=closed'), 'state', 'open', allowed)).toBe('closed');
      expect(readEnumParam(new URL('http://localhost/issues?state=all'), 'state', 'open', allowed)).toBe('all');
    });

    it('falls back for missing or unrecognized values', () => {
      expect(readEnumParam(new URL('http://localhost/issues'), 'state', 'open', allowed)).toBe('open');
      expect(readEnumParam(new URL('http://localhost/issues?state=bogus'), 'state', 'open', allowed)).toBe('open');
    });
  });

  describe('buildQueryHref', () => {
    it('sets and deletes parameters while preserving unrelated parameters', () => {
      const url = new URL('http://localhost/o/r/issues?milestone=3&keep=yes');
      expect(buildQueryHref(url, { state: 'closed', page: 2 })).toBe(
        '/o/r/issues?milestone=3&keep=yes&state=closed&page=2',
      );
      expect(buildQueryHref(url, { milestone: null, keep: 'yes', page: undefined, flag: false, empty: '' })).toBe(
        '/o/r/issues?keep=yes',
      );
    });

    it('omits the trailing question mark when all parameters are cleared', () => {
      const url = new URL('http://localhost/o/r/issues?state=closed&page=2');
      expect(buildQueryHref(url, { state: null, page: null })).toBe('/o/r/issues');
    });
  });
});
