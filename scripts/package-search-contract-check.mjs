#!/usr/bin/env node

import { readFileSync } from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const clientPath = path.join(root, 'web/src/lib/api/client.svelte.ts');
const pagePath = path.join(root, 'web/src/routes/[owner]/[repo]/packages/+page.svelte');
const formatPagePath = path.join(root, 'web/src/routes/[owner]/[repo]/packages/[format]/+page.svelte');
const uploadPath = path.join(root, 'web/src/routes/[owner]/[repo]/packages/upload/+page.svelte');
const backendPackageServicePath = path.join(root, 'crates/rg-core/src/package_registry/service.rs');

const client = readFileSync(clientPath, 'utf8');
const page = readFileSync(pagePath, 'utf8');
const formatPage = readFileSync(formatPagePath, 'utf8');
const uploadPage = readFileSync(uploadPath, 'utf8');
const backendPackageService = readFileSync(backendPackageServicePath, 'utf8');

const failures = [];

function extractQuotedArray(source, name) {
  const match = source.match(new RegExp(`const\\s+${name}\\s*=\\s*\\[([\\s\\S]*?)\\]`));
  if (!match) return null;
  return [...match[1].matchAll(/['"]([^'"]+)['"]/g)].map((m) => m[1]);
}

function extractBackendPackageTypes(source) {
  const constants = new Map();
  const moduleMatch = source.match(/pub\s+mod\s+package_types\s*\{([\s\S]*?)\n\}/);
  const moduleSource = moduleMatch?.[1] || source;

  for (const match of moduleSource.matchAll(/pub\s+const\s+([A-Z0-9_]+)\s*:\s*&str\s*=\s*"([^"]+)"/g)) {
    constants.set(match[1], match[2]);
  }

  const allMatch = moduleSource.match(/pub\s+const\s+ALL\s*:\s*&\[&str\]\s*=\s*&\[([\s\S]*?)\];/);
  if (!allMatch) return null;

  return [...allMatch[1].matchAll(/\b([A-Z0-9_]+)\b/g)]
    .map((m) => constants.get(m[1]))
    .filter(Boolean);
}

if (!/list:\s*async\s*\([^)]*query\?:\s*string/.test(client)) {
  failures.push('packages.list must accept an optional search query');
}

if (!/function\s+filterPackagesByQuery\s*\(/.test(client)) {
  failures.push('packages.list must normalize package search through filterPackagesByQuery');
}

if (!/res\.status\s*===\s*204[\s\S]*return\s+undefined\s+as\s+T/.test(client)) {
  failures.push('API client request() must treat 204 No Content as a successful empty response');
}

if (/delete:\s*\([^)]*version[^)]*\)\s*=>\s*\n?\s*request<\{\s*deleted:\s*boolean\s*\}>/.test(client)) {
  failures.push('packages.delete must not expect a JSON deleted envelope from the backend 204 response');
}

const filteredIndex = client.indexOf('filteredList');
const sliceIndex = client.indexOf('filteredList.slice');
if (filteredIndex === -1 || sliceIndex === -1 || filteredIndex > sliceIndex) {
  failures.push('packages.list must filter package results before paginating them');
}

if (!/packages\.list\([\s\S]*searchQuery[\s\S]*\)/.test(page)) {
  failures.push('Packages page must pass searchQuery into packages.list');
}

if (!/function\s+packageHref\s*\([\s\S]*encodeURIComponent\(pkg\.format\)[\s\S]*encodeURIComponent\(pkg\.name\)/.test(page)) {
  failures.push('Packages page must encode package format and name in detail links');
}

if (!/function\s+packageHref\s*\([\s\S]*encodeURIComponent\(format!\)[\s\S]*encodeURIComponent\(pkg\.name\)/.test(formatPage)) {
  failures.push('Package format page must encode package names in detail links');
}

if (/href="\/\{owner\}\/\{repo\}\/packages\/\{[^"]*\}\//.test(page + formatPage)) {
  failures.push('Package list pages must not interpolate raw package names into href attributes');
}

const backendTypes = extractBackendPackageTypes(backendPackageService);
const pageTypes = extractQuotedArray(page, 'formats');
const uploadTypes = extractQuotedArray(uploadPage, 'formats');

if (!backendTypes || backendTypes.length === 0) {
  failures.push('Could not extract backend package_types::ALL');
}

if (!pageTypes || pageTypes.length === 0) {
  failures.push('Could not extract package list page format filter');
}

if (!uploadTypes || uploadTypes.length === 0) {
  failures.push('Could not extract package upload page format selector');
}

if (backendTypes && pageTypes) {
  const missingFromFilter = backendTypes.filter((type) => !pageTypes.includes(type));
  if (missingFromFilter.length > 0) {
    failures.push(`Packages page format filter is missing backend package types: ${missingFromFilter.join(', ')}`);
  }
}

if (backendTypes && uploadTypes) {
  const missingFromUpload = backendTypes.filter((type) => !uploadTypes.includes(type));
  if (missingFromUpload.length > 0) {
    failures.push(`Package upload selector is missing backend package types: ${missingFromUpload.join(', ')}`);
  }
}

if (failures.length > 0) {
  for (const failure of failures) {
    console.log(`FAIL ${failure}`);
  }
  process.exit(1);
}

console.log('Package search frontend/backend contract ok');
