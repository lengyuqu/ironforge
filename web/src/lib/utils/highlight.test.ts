import { describe, expect, it } from 'vitest';
import { highlightCode, highlightCodeToLines, splitHighlightedHtmlLines } from './highlight';

describe('highlight utilities', () => {
  it('highlights single-line Rust code using core + rust grammar', async () => {
    const html = await highlightCode('fn main() {}', 'rust');
    expect(html).toContain('hljs-keyword');
    expect(html).toContain('fn');
  });

  it('escapes HTML safely when the language is unknown', async () => {
    const html = await highlightCode('<script>alert(1)</script>', '');
    expect(html).toBe('&lt;script&gt;alert(1)&lt;/script&gt;');
  });

  it('balances multi-line comment spans across every line', async () => {
    const code = '/* start\n * middle\n * end */';
    const lines = await highlightCodeToLines(code, 'rust');
    expect(lines).toHaveLength(3);
    for (const line of lines) {
      expect(line).toContain('<span class="hljs-comment">');
      expect(line).toContain('</span>');
    }
  });

  it('preserves nested spans across splitHighlightedHtmlLines', () => {
    const lines = splitHighlightedHtmlLines('<span class="a"><span class="b">one\ntwo</span></span>');
    expect(lines).toEqual([
      '<span class="a"><span class="b">one</span></span>',
      '<span class="a"><span class="b">two</span></span>',
    ]);
  });
});
