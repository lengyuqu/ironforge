#!/usr/bin/env node

import { readFileSync } from 'node:fs';

const source = readFileSync('web/src/lib/api/client.svelte.ts', 'utf8');
const failures = [];

if (!/headers\[['"]Content-Disposition['"]\]\s*=\s*contentDispositionAttachment\(filename\)/.test(source)) {
  failures.push('packages.publish must build Content-Disposition through contentDispositionAttachment(filename)');
}

if (!/filename\*=UTF-8''\$\{encodeURIComponent\(/.test(source)) {
  failures.push('contentDispositionAttachment must use RFC 5987 filename*=UTF-8 percent encoding');
}

if (/Content-Disposition['"\]]\s*=\s*`attachment;\s*filename="\$\{filename\}"/.test(source)) {
  failures.push('packages.publish must not interpolate raw filenames into a quoted Content-Disposition header');
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.error(`❌ ${failure}`);
  }
  process.exit(1);
}

console.log('Package publish frontend/backend contract ok');
