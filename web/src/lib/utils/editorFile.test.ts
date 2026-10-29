import { describe, it, expect } from 'vitest';
import {
  escapeHtml,
  filePathErrorKey,
  isConflictError,
  isMarkdownPath,
  languageFromPath,
} from './editorFile';

describe('languageFromPath', () => {
  it.each([
    ['main.rs', 'rust'],
    ['src/App.TSX', 'typescript'],
    ['a/b/c.yml', 'yaml'],
    ['Cargo.toml', 'ini'],
    ['x.html', 'xml'],
    ['script.zsh', 'bash'],
    ['README.markdown', 'markdown'],
  ])('%s -> %s', (path, lang) => {
    expect(languageFromPath(path)).toBe(lang);
  });

  it('returns an empty string for unknown or missing extensions', () => {
    expect(languageFromPath('notes.unknownext')).toBe('');
    expect(languageFromPath('')).toBe('');
  });
});

describe('isMarkdownPath', () => {
  it('accepts .md and .markdown case-insensitively', () => {
    expect(isMarkdownPath('README.md')).toBe(true);
    expect(isMarkdownPath('docs/Guide.MARKDOWN')).toBe(true);
  });

  it('rejects other files, including names that merely contain md', () => {
    expect(isMarkdownPath('md')).toBe(false);
    expect(isMarkdownPath('a.mdx')).toBe(false);
    expect(isMarkdownPath('file.md.bak')).toBe(false);
  });
});

describe('escapeHtml', () => {
  it('escapes the five HTML-significant characters', () => {
    expect(escapeHtml(`<a href="x">'&'</a>`)).toBe(
      '&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;'
    );
  });
});

describe('filePathErrorKey', () => {
  it.each([
    ['', 'repo.editor.path_required'],
    ['  ', 'repo.editor.path_required'],
    ['/a', 'repo.editor.path_invalid_leading'],
    ['\\a', 'repo.editor.path_invalid_leading'],
    ['a\\b', 'repo.editor.path_invalid_separator'],
    ['a//b', 'repo.editor.path_invalid_empty_segment'],
    ['a/', 'repo.editor.path_invalid_empty_segment'],
    ['a/./b', 'repo.editor.path_invalid_segment'],
    ['a/../b', 'repo.editor.path_invalid_segment'],
    ['a\u0000b', 'repo.editor.path_invalid_control'],
  ])('%j -> %s', (value, key) => {
    expect(filePathErrorKey(value)).toBe(key);
  });

  it('accepts ordinary and surrounding-whitespace paths', () => {
    expect(filePathErrorKey('docs/api.md')).toBe('');
    expect(filePathErrorKey('  .github/workflows/ci.yml ')).toBe('');
    expect(filePathErrorKey('..hidden/file')).toBe('');
  });
});

describe('isConflictError', () => {
  it('matches sha mismatch and conflict in any case', () => {
    expect(isConflictError('SHA mismatch')).toBe(true);
    expect(isConflictError('edit Conflict')).toBe(true);
  });

  it('ignores unrelated failures', () => {
    expect(isConflictError('disk full')).toBe(false);
  });
});
