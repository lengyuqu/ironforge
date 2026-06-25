import { marked } from 'marked';

function stripDangerousAttributes(tag: string): string {
  return tag
    .replace(/\s+on[a-z]+\s*=\s*"[^"]*"/gi, '')
    .replace(/\s+on[a-z]+\s*=\s*'[^']*'/gi, '')
    .replace(/\s+on[a-z]+\s*=\s*[^\s>]+/gi, '')
    .replace(/\s+(href|src)\s*=\s*"javascript:[^"]*"/gi, ' $1="#"')
    .replace(/\s+(href|src)\s*=\s*'javascript:[^']*'/gi, " $1='#'")
    .replace(/\s+(href|src)\s*=\s*javascript:[^\s>]*/gi, ' $1="#"');
}

export function sanitizeHtml(html: string): string {
  return html
    .replace(/<script[\s\S]*?>[\s\S]*?<\/script>/gi, '')
    .replace(/<style[\s\S]*?>[\s\S]*?<\/style>/gi, '')
    .replace(/<\/?(iframe|object|embed|link|meta|base)[^>]*>/gi, '')
    .replace(/<[^>]+>/g, stripDangerousAttributes);
}

export function renderMarkdown(content: string): string {
  const html = marked.parse(content || '', { async: false }) as string;
  return sanitizeHtml(html);
}
