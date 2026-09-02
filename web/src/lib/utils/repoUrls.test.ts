import { describe, it, expect } from 'vitest';
import {
  buildRepoQuery,
  buildTreeHref,
  buildCommitHref,
  buildBlobHref,
  buildEditHref,
  encodeRepoPath,
} from './repoUrls';

// These builders keep tree/blob/commit/edit links consistent across the
// repo overview and sub-components — a regression here breaks navigation
// everywhere at once.
describe('repoUrls', () => {
  describe('buildRepoQuery', () => {
    it('serializes ref and path', () => {
      expect(buildRepoQuery('main', 'src')).toBe('?ref=main&path=src');
    });
    it('omits empty parts and the leading ? when nothing set', () => {
      expect(buildRepoQuery('', 'src')).toBe('?path=src');
      expect(buildRepoQuery('main', '')).toBe('?ref=main');
      expect(buildRepoQuery('', '')).toBe('');
    });
  });

  describe('encodeRepoPath', () => {
    it('URL-encodes each segment but keeps slashes', () => {
      expect(encodeRepoPath('docs/my file #1.md')).toBe('docs/my%20file%20%231.md');
    });
    it('returns plain paths untouched', () => {
      expect(encodeRepoPath('src/main.rs')).toBe('src/main.rs');
    });
  });

  describe('href builders', () => {
    it('buildTreeHref composes owner/repo + query', () => {
      expect(buildTreeHref('alice', 'demo', 'main', 'src')).toBe('/alice/demo?ref=main&path=src');
    });

    it('buildCommitHref appends the sha then the ref query', () => {
      expect(buildCommitHref('alice', 'demo', 'main', 'abc123')).toBe(
        '/alice/demo/commits/abc123?ref=main',
      );
    });

    it('buildBlobHref encodes the file path and preserves the ref', () => {
      expect(buildBlobHref('alice', 'demo', 'dev', 'docs/my file.md')).toBe(
        '/alice/demo/blob/docs/my%20file.md?ref=dev',
      );
    });

    it('buildEditHref includes sha and ref params', () => {
      expect(buildEditHref('alice', 'demo', 'main', 'src/a b.rs', 'dead10')).toBe(
        '/alice/demo/edit/src/a%20b.rs?sha=dead10&ref=main',
      );
    });

    it('buildEditHref omits the query when neither sha nor ref given', () => {
      expect(buildEditHref('alice', 'demo', '', 'README.md', undefined)).toBe(
        '/alice/demo/edit/README.md',
      );
    });
  });
});
