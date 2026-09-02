import { describe, it, expect } from 'vitest';
import { statusIcon, statusText, statusColor } from './commitStatus';
import { highlightText } from './search';

describe('commitStatus', () => {
  it('maps states to glyph, text and colour', () => {
    expect(statusIcon('success')).toBe('✅');
    expect(statusText('success')).toBe('All checks passed');
    expect(statusColor('success')).toBe('var(--green)');

    expect(statusIcon('failure')).toBe('❌');
    expect(statusText('failure')).toBe('Some checks failed');

    expect(statusIcon('error')).toBe('❌');
    expect(statusColor('error')).toBe('var(--orange)');

    expect(statusIcon('pending')).toBe('⏳');
    expect(statusColor('pending')).toBe('var(--yellow)');
  });

  it('falls back to unknown for unrecognised states', () => {
    expect(statusIcon('whatever')).toBe('❓');
    expect(statusText('whatever')).toBe('Unknown status');
    expect(statusColor('whatever')).toBe('var(--text-muted)');
  });
});

describe('highlightText', () => {
  it('escapes HTML when no query is given', () => {
    expect(highlightText('<b>a&b</b>', '')).toBe('&lt;b&gt;a&amp;b&lt;/b&gt;');
  });

  it('wraps every occurrence of the query in mark tags', () => {
    // substring matches: "rusty" highlights only its "rust" prefix
    expect(highlightText('rust is rusty', 'rust')).toBe(
      '<mark class="search-highlight">rust</mark> is <mark class="search-highlight">rust</mark>y',
    );
  });

  it('highlights multiple words case-insensitively', () => {
    expect(highlightText('Fast Brown Fox', 'brown fast')).toBe(
      '<mark class="search-highlight">Fast</mark> <mark class="search-highlight">Brown</mark> Fox',
    );
  });

  it('escapes regex metacharacters in the query', () => {
    // 'a.b' is escaped so the dot matches literally, not as a wildcard
    expect(highlightText('a.b axb', 'a.b')).toBe(
      '<mark class="search-highlight">a.b</mark> axb',
    );
  });

  it('escapes HTML before highlighting so tags cannot inject', () => {
    expect(highlightText('<div>div</div>', 'div')).toBe(
      '&lt;<mark class="search-highlight">div</mark>&gt;<mark class="search-highlight">div</mark>&lt;/<mark class="search-highlight">div</mark>&gt;',
    );
  });
});
