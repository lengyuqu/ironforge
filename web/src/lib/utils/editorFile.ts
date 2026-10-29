// Pure helpers for the file editor: language detection, path validation and
// conflict detection. Kept free of Svelte so they can be unit-tested directly.

const LANGUAGE_BY_EXTENSION: Record<string, string> = {
  c: 'c',
  h: 'c',
  cpp: 'cpp',
  cc: 'cpp',
  cxx: 'cpp',
  hpp: 'cpp',
  rs: 'rust',
  go: 'go',
  py: 'python',
  js: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  ts: 'typescript',
  jsx: 'javascript',
  tsx: 'typescript',
  java: 'java',
  rb: 'ruby',
  php: 'php',
  swift: 'swift',
  kt: 'kotlin',
  dart: 'dart',
  html: 'xml',
  css: 'css',
  scss: 'scss',
  json: 'json',
  xml: 'xml',
  yaml: 'yaml',
  yml: 'yaml',
  toml: 'ini',
  sh: 'bash',
  bash: 'bash',
  zsh: 'bash',
  sql: 'sql',
  dockerfile: 'dockerfile',
  md: 'markdown',
  markdown: 'markdown',
};

/** highlight.js language for a path, or '' when the extension is unknown. */
export function languageFromPath(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() || '';
  return LANGUAGE_BY_EXTENSION[ext] || '';
}

export function isMarkdownPath(path: string): boolean {
  return /\.(md|markdown)$/i.test(path);
}

export function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/**
 * Catalog key describing why `value` is not a usable repository path, or ''
 * when it is fine. Returning a key keeps this module independent of i18n.
 */
export function filePathErrorKey(value: string): string {
  const normalized = value.trim();
  if (!normalized) return 'repo.editor.path_required';
  if (normalized.startsWith('/') || normalized.startsWith('\\')) {
    return 'repo.editor.path_invalid_leading';
  }
  if (normalized.includes('\\')) return 'repo.editor.path_invalid_separator';
  if (normalized.includes('//') || normalized.endsWith('/')) {
    return 'repo.editor.path_invalid_empty_segment';
  }
  if (normalized.split('/').some((segment) => segment === '.' || segment === '..')) {
    return 'repo.editor.path_invalid_segment';
  }
  if (/[\u0000-\u001f]/.test(normalized)) return 'repo.editor.path_invalid_control';
  return '';
}

/** True when a save error means the file changed underneath the editor. */
export function isConflictError(message: string): boolean {
  const normalized = message.toLowerCase();
  return normalized.includes('sha mismatch') || normalized.includes('conflict');
}
