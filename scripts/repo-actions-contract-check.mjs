#!/usr/bin/env node

import { readFileSync } from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const clientPaths = [
  path.join(root, 'web/src/lib/api/client.svelte.ts'),
  path.join(root, 'web/src/lib/api/repos.ts'),
];
const headerPath = path.join(root, 'web/src/lib/components/RepoHeader.svelte');
const backendPath = path.join(root, 'crates/rg-http/src/api/repos.rs');
const routerPath = path.join(root, 'crates/rg-http/src/lib.rs');

const backend = readFileSync(backendPath, 'utf8');
const router = readFileSync(routerPath, 'utf8');
const header = readFileSync(headerPath, 'utf8');
const failures = [];

for (const route of [
  'path = "/repos/{owner}/{name}/star"',
  'path = "/repos/{owner}/{name}/starred"',
  'path = "/repos/{owner}/{name}/watch"',
]) {
  if (!backend.includes(route)) {
    failures.push(`Backend repo action OpenAPI annotation missing: ${route}`);
  }
}

for (const [label, pattern] of [
  ['PUT /star', /\.route\(\s*"\/repos\/\{owner\}\/\{name\}\/star",\s*put\(api::repos::star_repo\)\s*\)/],
  ['GET /starred', /\.route\(\s*"\/repos\/\{owner\}\/\{name\}\/starred",\s*get\(api::repos::get_starred_status\),?\s*\)/],
  ['GET /watch', /"\/repos\/\{owner\}\/\{name\}\/watch"[\s\S]*get\(api::repos::get_watch_status\)/],
  ['PUT /watch', /"\/repos\/\{owner\}\/\{name\}\/watch"[\s\S]*\.put\(api::repos::watch_repo\)/],
  ['DELETE /watch', /"\/repos\/\{owner\}\/\{name\}\/watch"[\s\S]*\.delete\(api::repos::unwatch_repo\)/],
]) {
  if (!pattern.test(router)) {
    failures.push(`Backend repo action router binding missing: ${label}`);
  }
}

for (const clientPath of clientPaths) {
  const source = readFileSync(clientPath, 'utf8');
  const name = path.relative(root, clientPath);

  if (!/starred:\s*\([^)]*\)\s*=>\s*\n?\s*request<\{\s*starred:\s*boolean\s*\}>\(`\/repos\/\$\{owner\}\/\$\{repo\}\/starred`,\s*\{\s*method:\s*'GET'\s*\}/.test(source)) {
    failures.push(`${name} must expose repos.starred using GET /starred`);
  }

  if (!/watchStatus:\s*\([^)]*\)\s*=>\s*\n?\s*request<\{\s*watch_state:\s*'not_watching'\s*\|\s*'watching'\s*\|\s*'ignoring'\s*\}>/.test(source)) {
    failures.push(`${name} must expose repos.watchStatus with the backend watch-state union`);
  }

  const unstarBlock = source.match(/unstar:\s*async\s*\([^)]*\)\s*=>\s*\{[\s\S]*?\n\s*\},/);
  if (!unstarBlock) {
    failures.push(`${name} must expose async repos.unstar`);
    continue;
  }

  if (!/repos\.starred\(owner,\s*repo\)/.test(unstarBlock[0])) {
    failures.push(`${name} repos.unstar must check GET /starred before toggling`);
  }

  if (!/if\s*\(!status\.starred\)\s*return\s*\{\s*starred:\s*false\s*\}/.test(unstarBlock[0])) {
    failures.push(`${name} repos.unstar must be idempotent when already unstarred`);
  }
}

if (!/repos\.starred\(owner,\s*repo\)/.test(header)) {
  failures.push('RepoHeader must load starred status from the backend before rendering the star action');
}

if (!/repos\.watchStatus\(owner,\s*repo\)/.test(header)) {
  failures.push('RepoHeader must load watch status from the backend before rendering the watch action');
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.log(`FAIL ${failure}`);
  }
  process.exit(1);
}

console.log('Repo action frontend/backend contract ok');
