import { describe, expect, it } from 'vitest';
import { createLatest } from './latest';

describe('createLatest', () => {
  it('marks an earlier load as stale once a newer load begins', () => {
    const latest = createLatest();
    const first = latest.begin();
    expect(first()).toBe(true);

    const second = latest.begin();
    expect(first()).toBe(false);
    expect(second()).toBe(true);
  });

  it('marks the active load as stale when invalidate is called', () => {
    const latest = createLatest();
    const current = latest.begin();
    expect(current()).toBe(true);

    latest.invalidate();
    expect(current()).toBe(false);
  });
});
