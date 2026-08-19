#!/usr/bin/env node

import { readFileSync } from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const repoPagePath = path.join(root, 'web/src/routes/[owner]/[repo]/+page.svelte');
const repoPage = readFileSync(repoPagePath, 'utf8');
const failures = [];

// F-004: Repository README must be rendered through the shared renderMarkdown utility
// from $lib/utils/markdown, which uses marked + DOMParser sanitization.
// The old hand-written inline parser (escapeHtml / safeMarkdownHref / renderInlineMarkdown)
// has been removed in favour of the same renderer used by the blob page.

if (!/import\s*\{\s*renderMarkdown\s*\}\s*from\s*['"]\$lib\/utils\/markdown['"]/.test(repoPage)) {
  failures.push('Repository page must import renderMarkdown from $lib/utils/markdown');
}

if (!/\{@html\s+renderMarkdown\(readmeContent\)\}/.test(repoPage)) {
  failures.push('Repository page must render README content via {@html renderMarkdown(readmeContent)}');
}

// Ensure the old hand-written helpers are fully removed
if (/function\s+escapeHtml\s*\(/.test(repoPage)) {
  failures.push('Repository page must not contain the old escapeHtml helper');
}

if (/function\s+safeMarkdownHref\s*\(/.test(repoPage)) {
  failures.push('Repository page must not contain the old safeMarkdownHref helper');
}

if (/function\s+renderInlineMarkdown\s*\(/.test(repoPage)) {
  failures.push('Repository page must not contain the old renderInlineMarkdown helper');
}

if (failures.length > 0) {
  console.error('Repository README render frontend/backend contract failed:');
  for (const failure of failures) {
    console.error(`- ${failure}`);
  }
  process.exit(1);
}

console.log('Repository README render frontend/backend contract ok');
