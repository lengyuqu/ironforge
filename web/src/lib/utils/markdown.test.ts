import { describe, it, expect } from 'vitest';
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
