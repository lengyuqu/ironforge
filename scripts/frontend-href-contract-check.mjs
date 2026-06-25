#!/usr/bin/env node

import { readFileSync } from 'node:fs';
import { relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const files = execFileSync('rg', ['--files', 'web/src'], { cwd: root, encoding: 'utf8' })
  .split('\n')
  .filter((file) => file.endsWith('.svelte'));

const failures = [];
const literalDynamicHref = /\bhref="[^"]*\{[^"]*"/g;

for (const file of files) {
  const source = readFileSync(resolve(root, file), 'utf8');
  for (const match of source.matchAll(literalDynamicHref)) {
    failures.push(`${relative(root, file)}: static href contains Svelte placeholders: ${match[0]}`);
  }
}

if (failures.length > 0) {
  console.error('Frontend href contract failed:');
  for (const failure of failures) {
    console.error(`- ${failure}`);
  }
  process.exit(1);
}

console.log('Frontend href contract ok');
