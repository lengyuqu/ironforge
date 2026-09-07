import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { sanitizeHtml, renderMarkdown } from './markdown';

// renderMarkdown powers all user-authored content rendering (issue/PR
// bodies, comments, wiki) — the sanitizer is the XSS last line of defence.
describe('sanitizeHtml', () => {
  it('unwraps script elements so no executable tag remains', () => {
    // whitelist design: disallowed tags are unwrapped (children kept as
    // inert text) — the invariant is that no <script> tag survives.
    const out = sanitizeHtml('<p>ok</p><script>alert(1)</script>');
    expect(out).toContain('<p>ok</p>');
    expect(out.toLowerCase()).not.toContain('<script');
    expect(out.toLowerCase()).not.toContain('</script');
  });

  it('strips inline event handlers', () => {
    const out = sanitizeHtml('<img src="https://x/y.png" onerror="alert(1)">');
    expect(out.toLowerCase()).not.toContain('onerror');
  });

  it('unwraps non-whitelisted tags but keeps whitelisted markup', () => {
    // 'b' is not whitelisted (unwrapped to text); 'strong' and 'p' are
    const out = sanitizeHtml('<iframe src="https://evil"></iframe><b>bold</b><strong>keep</strong>');
    expect(out.toLowerCase()).not.toContain('iframe');
    expect(out).toContain('bold');
    expect(out).toContain('<strong>keep</strong>');
  });

  it('drops unsafe link protocols but keeps http(s)', () => {
    const out = sanitizeHtml('<a href="javascript:alert(1)">x</a><a href="https://ok.dev">y</a>');
    expect(out.toLowerCase()).not.toContain('javascript:');
    expect(out).toContain('href="https://ok.dev"');
  });

  it('strips inline style attributes', () => {
    const out = sanitizeHtml('<div style="background:url(x)">t</div>');
    expect(out.toLowerCase()).not.toContain('style=');
  });
});

// The regex fallback runs wherever DOMParser is unavailable (e.g. SSR).
// It sees raw markup, so entity-encoded schemes must be decoded before
// URL validation — otherwise `javascript&#58;alert(1)` slips through as a
// "relative" URL and the browser decodes it back to javascript:.
describe('sanitizeHtml regex fallback (no DOMParser)', () => {
  const g = globalThis as { DOMParser?: unknown; NodeFilter?: unknown };
  const savedDOMParser = g.DOMParser;
  const savedNodeFilter = g.NodeFilter;

  beforeEach(() => {
    delete g.DOMParser;
    delete g.NodeFilter;
  });

  afterEach(() => {
    g.DOMParser = savedDOMParser;
    g.NodeFilter = savedNodeFilter;
  });

  it('blocks numeric entity encoded scheme', () => {
    const out = sanitizeHtml('<a href="javascript&#58;alert(1)">x</a>');
    expect(out.toLowerCase()).not.toContain('javascript');
    expect(out.toLowerCase()).not.toContain('alert');
  });

  it('blocks hex entity, semicolon-less numeric and named colon refs', () => {
    const payloads = [
      '<a href="javascript&#x3a;alert(1)">x</a>',
      '<a href="javascript&#X3A;alert(1)">x</a>',
      '<a href="javascript&#58alert(1)">x</a>',
      '<a href="javascript&colon;alert(1)">x</a>',
    ];
    for (const payload of payloads) {
      expect(sanitizeHtml(payload).toLowerCase()).not.toContain('javascript');
    }
  });

  it('blocks tab/newline smuggling stripped by the URL parser', () => {
    const out = sanitizeHtml('<a href="java&Tab;script&colon;alert(1)">x</a>');
    expect(out.toLowerCase()).not.toContain('script');
    expect(out.toLowerCase()).not.toContain('alert');
  });

  it('blocks unquoted href with entity encoded scheme', () => {
    const out = sanitizeHtml('<a href=javascript&#58;alert(1)>x</a>');
    expect(out.toLowerCase()).not.toContain('javascript');
  });

  it('keeps safe links whose query string contains entities', () => {
    // `&amp;#58;` decodes (once) to literal `&#58;` — a harmless query
    // fragment, not a scheme separator. Must survive sanitisation.
    const out = sanitizeHtml('<a href="https://ok.dev/?x=1&amp;#58;y">y</a>');
    expect(out).toContain('href="https://ok.dev/?x=1&amp;#58;y"');
  });

  it('keeps relative and anchor links in the fallback path', () => {
    const out = sanitizeHtml('<a href="/repo/src">a</a><a href="#section">b</a>');
    expect(out).toContain('href="/repo/src"');
    expect(out).toContain('href="#section"');
  });
});

describe('renderMarkdown', () => {
  it('renders basic markdown to sanitized html', () => {
    const out = renderMarkdown('# Title\n\n**bold**');
    expect(out).toContain('<h1>Title</h1>');
    expect(out).toContain('<strong>bold</strong>');
  });

  it('sanitizes raw html script injection from markdown content', () => {
    const out = renderMarkdown('hello\n\n<script>alert(1)</script>\n\nworld');
    expect(out.toLowerCase()).not.toContain('<script');
    expect(out).toContain('hello');
    expect(out).toContain('world');
  });

  it('handles empty content', () => {
    expect(renderMarkdown('')).toBe('');
  });
});
