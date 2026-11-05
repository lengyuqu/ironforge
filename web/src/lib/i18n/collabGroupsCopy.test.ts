import { describe, it, expect } from 'vitest';
import en from './translations/en.json';
import zh from './translations/zh-CN.json';

// The groups UI must never hint at links the reader cannot see. The copy is
// part of that contract: no wording about hidden / filtered / total items on
// the pages that list links, in either language.
function collect(node: unknown, out: string[] = []): string[] {
  if (typeof node === 'string') out.push(node);
  else if (node && typeof node === 'object') Object.values(node).forEach((v) => collect(v, out));
  return out;
}

describe('collab_groups copy', () => {
  it.each([
    ['en', en.collab_groups, /hidden|invisible|filtered|omitted|\btotal\b|\bcount\b|\bN\b/i],
    ['zh-CN', zh.collab_groups, /隐藏|不可见|过滤|省略|总数|共\s*\{/],
  ])('%s has no wording that reveals hidden links', (_lang, tree, forbidden) => {
    // Only the strings shown around the links list; the create-form warning is
    // about who may read title/description, not about link counts.
    const strings = collect({ links: tree.links, detail: tree.detail, add: tree.add, no_list: tree.no_list_note });
    for (const s of strings) expect(s, s).not.toMatch(forbidden);
  });

  it('has no ranking vocabulary in the health copy', () => {
    for (const s of collect(en.collab_health)) {
      expect(s, s).not.toMatch(/leaderboard|top \d|best|worst/i);
    }
  });
});
