import { describe, it, expect } from 'vitest';
import { buildLineDiff } from './diff';

// buildLineDiff is an LCS line diff feeding the in-browser file editor —
// these tests pin the core diff semantics: same/add/del classification,
// line numbering on both sides, and edge cases.
describe('buildLineDiff', () => {
  it('marks identical texts as all-same with both line numbers', () => {
    const diff = buildLineDiff('a\nb\nc', 'a\nb\nc');
    expect(diff).toEqual([
      { type: 'same', oldNumber: 1, newNumber: 1, text: 'a' },
      { type: 'same', oldNumber: 2, newNumber: 2, text: 'b' },
      { type: 'same', oldNumber: 3, newNumber: 3, text: 'c' },
    ]);
  });

  it('classifies pure additions and deletions with their side numbers', () => {
    expect(buildLineDiff('', 'x\ny')).toEqual([
      { type: 'add', newNumber: 1, text: 'x' },
      { type: 'add', newNumber: 2, text: 'y' },
    ]);
    expect(buildLineDiff('x\ny', '')).toEqual([
      { type: 'del', oldNumber: 1, text: 'x' },
      { type: 'del', oldNumber: 2, text: 'y' },
    ]);
  });

  it('detects the minimal edit around unchanged context', () => {
    const diff = buildLineDiff('a\nb\nc', 'a\nB\nc');
    expect(diff).toEqual([
      { type: 'same', oldNumber: 1, newNumber: 1, text: 'a' },
      { type: 'del', oldNumber: 2, text: 'b' },
      { type: 'add', newNumber: 2, text: 'B' },
      { type: 'same', oldNumber: 3, newNumber: 3, text: 'c' },
    ]);
  });

  it('keeps new-numbering contiguous across mixed edits', () => {
    const diff = buildLineDiff('1\n2', '1\n2\n3');
    const addRows = diff.filter((l) => l.type === 'add');
    expect(addRows).toEqual([{ type: 'add', newNumber: 3, text: '3' }]);
    expect(diff[0].newNumber).toBe(1);
    expect(diff[1].newNumber).toBe(2);
  });

  it('handles empty vs empty', () => {
    expect(buildLineDiff('', '')).toEqual([]);
  });
});
