import { describe, it, expect } from 'vitest';
import { parseRunnerLabels } from './runners';

// parseRunnerLabels tolerates three storage shapes the backend has used
// over time: JSON-encoded arrays, comma-separated plain strings and the
// already-normalized array. These tests pin the compatibility contract
// documented in runners.ts (F-015 area).
describe('parseRunnerLabels', () => {
  it('returns arrays unchanged', () => {
    expect(parseRunnerLabels(['linux', 'docker'])).toEqual(['linux', 'docker']);
  });

  it('parses JSON-encoded arrays and trims/filters entries', () => {
    expect(parseRunnerLabels('["linux", " x86_64 ", ""]')).toEqual(['linux', 'x86_64']);
  });

  it('falls back to comma-separated parsing for plain strings', () => {
    expect(parseRunnerLabels('linux, x86_64,, docker')).toEqual(['linux', 'x86_64', 'docker']);
  });

  it('drops non-string JSON entries', () => {
    expect(parseRunnerLabels('[1, "linux", null]')).toEqual(['linux']);
  });

  it('handles null / undefined / empty inputs', () => {
    expect(parseRunnerLabels(null)).toEqual([]);
    expect(parseRunnerLabels(undefined)).toEqual([]);
    expect(parseRunnerLabels('')).toEqual([]);
  });
});
