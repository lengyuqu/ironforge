#!/usr/bin/env node

import { readFileSync } from 'node:fs';

const editorFiles = [
  'web/src/routes/[owner]/[repo]/new/+page.svelte',
  'web/src/routes/[owner]/[repo]/edit/[...path]/+page.svelte',
];

const issues = [];

for (const file of editorFiles) {
  const source = readFileSync(file, 'utf8');

  if (/fetch\s*\(\s*`?\/api\/v1\/repos\//.test(source)) {
    issues.push(`${file}: uses raw /api/v1 fetch instead of the API client`);
  }

  if (source.includes('jwt_token')) {
    issues.push(`${file}: reads legacy jwt_token instead of shared token state`);
  }

  if (!source.includes('repos.saveContent')) {
    issues.push(`${file}: does not use repos.saveContent for content writes`);
  }
}

if (issues.length > 0) {
  console.error('Repository content editor contract check failed:');
  for (const issue of issues) {
    console.error(`- ${issue}`);
  }
  process.exit(1);
}

console.log('Repository content editor frontend/backend contract ok');
