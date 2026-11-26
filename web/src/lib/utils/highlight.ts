import type { HLJSApi, LanguageFn } from 'highlight.js';
import { escapeHtml } from './editorFile';

const MAX_HIGHLIGHT_CHARS = 300_000;

const LANGUAGE_LOADERS: Record<string, () => Promise<{ default: LanguageFn }>> = {
  c: () => import('highlight.js/lib/languages/c'),
  cpp: () => import('highlight.js/lib/languages/cpp'),
  rust: () => import('highlight.js/lib/languages/rust'),
  go: () => import('highlight.js/lib/languages/go'),
  python: () => import('highlight.js/lib/languages/python'),
  javascript: () => import('highlight.js/lib/languages/javascript'),
  typescript: () => import('highlight.js/lib/languages/typescript'),
  java: () => import('highlight.js/lib/languages/java'),
  ruby: () => import('highlight.js/lib/languages/ruby'),
  php: () => import('highlight.js/lib/languages/php'),
  swift: () => import('highlight.js/lib/languages/swift'),
  kotlin: () => import('highlight.js/lib/languages/kotlin'),
  dart: () => import('highlight.js/lib/languages/dart'),
  xml: () => import('highlight.js/lib/languages/xml'),
  html: () => import('highlight.js/lib/languages/xml'),
  css: () => import('highlight.js/lib/languages/css'),
  scss: () => import('highlight.js/lib/languages/scss'),
  json: () => import('highlight.js/lib/languages/json'),
  yaml: () => import('highlight.js/lib/languages/yaml'),
  ini: () => import('highlight.js/lib/languages/ini'),
  bash: () => import('highlight.js/lib/languages/bash'),
  sql: () => import('highlight.js/lib/languages/sql'),
  dockerfile: () => import('highlight.js/lib/languages/dockerfile'),
  markdown: () => import('highlight.js/lib/languages/markdown'),
  makefile: () => import('highlight.js/lib/languages/makefile'),
};

let corePromise: Promise<HLJSApi> | null = null;
const registeredLanguages = new Map<string, Promise<boolean>>();

async function getCore(): Promise<HLJSApi> {
  if (!corePromise) {
    corePromise = import('highlight.js/lib/core').then((mod) => mod.default);
  }
  return corePromise;
}

async function ensureLanguage(hljs: HLJSApi, lang: string): Promise<string> {
  const normalized = lang === 'html' ? 'xml' : lang;
  const loader = LANGUAGE_LOADERS[normalized];
  if (!loader) return '';

  if (hljs.getLanguage(normalized)) return normalized;

  let pending = registeredLanguages.get(normalized);
  if (!pending) {
    pending = loader()
      .then((mod) => {
        if (!hljs.getLanguage(normalized)) {
          hljs.registerLanguage(normalized, mod.default);
        }
        return true;
      })
      .catch(() => false);
    registeredLanguages.set(normalized, pending);
  }

  return (await pending) ? normalized : '';
}

/**
 * Highlight `code` using only `highlight.js/lib/core` and the single requested
 * grammar chunk. Falls back to HTML-escaped plain text for unknown languages,
 * oversized payloads, or load failures.
 */
export async function highlightCode(code: string, lang: string): Promise<string> {
  if (!code) return '';
  if (!lang || code.length > MAX_HIGHLIGHT_CHARS) {
    return escapeHtml(code);
  }

  try {
    const hljs = await getCore();
    const resolved = await ensureLanguage(hljs, lang);
    if (!resolved) return escapeHtml(code);
    return hljs.highlight(code, { language: resolved, ignoreIllegals: true }).value;
  } catch {
    return escapeHtml(code);
  }
}

/**
 * Split highlighted HTML into per-line fragments while balancing open `<span>`
 * tags across line breaks so multi-line comments and strings keep their colour
 * inside row-based code tables.
 */
export function splitHighlightedHtmlLines(html: string): string[] {
  const rawLines = html.split('\n');
  const openTags: string[] = [];
  const tagRe = /<\/?span\b[^>]*>/g;

  return rawLines.map((line) => {
    const prefix = openTags.join('');
    let match: RegExpExecArray | null;
    tagRe.lastIndex = 0;
    while ((match = tagRe.exec(line)) !== null) {
      const token = match[0];
      if (token.startsWith('</')) {
        openTags.pop();
      } else {
        openTags.push(token);
      }
    }
    const suffix = '</span>'.repeat(openTags.length);
    return `${prefix}${line}${suffix}`;
  });
}

/** Highlight `code` once and return one balanced HTML fragment per line. */
export async function highlightCodeToLines(code: string, lang: string): Promise<string[]> {
  if (!code) return [];
  if (!lang || code.length > MAX_HIGHLIGHT_CHARS) {
    return code.split('\n').map(escapeHtml);
  }
  const html = await highlightCode(code, lang);
  return splitHighlightedHtmlLines(html);
}
