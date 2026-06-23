#!/usr/bin/env node

import { readFileSync } from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const mainClientPath = path.join(root, 'web/src/lib/api/client.svelte.ts');
const splitClientPath = path.join(root, 'web/src/lib/api/releases.ts');
const releasesPagePath = path.join(root, 'web/src/routes/[owner]/[repo]/releases/+page.svelte');
const backendPath = path.join(root, 'crates/rg-http/src/api/releases.rs');

const mainClient = readFileSync(mainClientPath, 'utf8');
const splitClient = readFileSync(splitClientPath, 'utf8');
const page = readFileSync(releasesPagePath, 'utf8');
const backend = readFileSync(backendPath, 'utf8');

const failures = [];

const requiredRoutes = [
  'releases/{release_id}/assets',
  'releases/assets/{asset_id}/download',
  'releases/assets/{asset_id}',
];

for (const route of requiredRoutes) {
  if (!backend.includes(route)) {
    failures.push(`Backend release asset route missing from OpenAPI annotations: ${route}`);
  }
}

for (const [name, source] of [
  ['client.svelte.ts', mainClient],
  ['releases.ts', splitClient],
]) {
  for (const method of ['listAssets', 'uploadAsset', 'getAsset', 'assetDownloadUrl', 'deleteAsset']) {
    if (!new RegExp(`\\b${method}\\s*:`).test(source)) {
      failures.push(`${name} must expose releases.${method}`);
    }
  }

  if (!/x-asset-filename/.test(source)) {
    failures.push(`${name} uploadAsset must send the backend-required x-asset-filename header`);
  }
}

if (!/releases\.listAssets\(/.test(page)) {
  failures.push('Releases page must load release assets from the backend');
}

if (!/releases\.assetDownloadUrl\(/.test(page)) {
  failures.push('Releases page must render backend download URLs for assets');
}

if (!/params\.set\('ref',\s*tag\)/.test(page)) {
  failures.push('Releases Browse files link must pass the release tag as ref, not path');
}

if (/params\.set\('path',\s*tag\)/.test(page)) {
  failures.push('Releases Browse files link still maps release tag into path');
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.log(`FAIL ${failure}`);
  }
  process.exit(1);
}

console.log('Release assets frontend/backend contract ok');
