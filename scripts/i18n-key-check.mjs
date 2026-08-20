#!/usr/bin/env node
// Q6.2: i18n key contract check.
//
// Guards against missing translation keys:
// 1. en.json and zh-CN.json must expose the exact same key set.
// 2. Every static `t('a.b.c')` call in web/src must resolve in both catalogs.
// 3. Every dynamic `t(`a.b.${var}`)` call must resolve to at least one key
//    sharing the static prefix in both catalogs.
//
// Usage: node scripts/i18n-key-check.mjs   (from repo root or web/)

import { readFileSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const webSrc = path.join(repoRoot, 'web', 'src');
const localesDir = path.join(repoRoot, 'web', 'src', 'lib', 'i18n', 'translations');

const LOCALES = ['en', 'zh-CN'];
const catalogs = new Map();
for (const locale of LOCALES) {
  const file = path.join(localesDir, `${locale}.json`);
  catalogs.set(locale, flattenKeys(JSON.parse(readFileSync(file, 'utf8'))));
}

const failures = [];

// ── Check 1: both catalogs expose the same key set ──────────────────────────
for (const [a, b] of [
  ['en', 'zh-CN'],
  ['zh-CN', 'en'],
]) {
  const keysA = catalogs.get(a);
  const keysB = catalogs.get(b);
  const missing = [...keysA].filter((key) => !keysB.has(key));
  for (const key of missing) {
    failures.push(`"${key}" exists in ${a}.json but is missing in ${b}.json`);
  }
}

// ── Collect t() call sites ───────────────────────────────────────────────────
const staticKeys = [];
const dynamicPrefixes = [];

for (const file of walk(webSrc)) {
  const source = readFileSync(file, 'utf8');
  const rel = path.relative(repoRoot, file).replaceAll('\\', '/');

  // Static single/double quoted keys: t('a.b') or t("a.b")
  const staticRe = /\bt\(\s*(['"])([^'"\n]+)\1/g;
  for (const [, , key] of source.matchAll(staticRe)) {
    staticKeys.push({ key, file: rel });
  }

  // Template-literal keys: t(`a.b.${var}`) — keep the static prefix
  const dynamicRe = /\bt\(\s*`([^`$]*)\$\{/g;
  for (const [, prefix] of source.matchAll(dynamicRe)) {
    if (prefix) {
      dynamicPrefixes.push({ prefix, file: rel });
    }
  }
}

// ── Check 2: static keys resolve in every catalog ────────────────────────────
for (const { key, file } of staticKeys) {
  for (const locale of LOCALES) {
    if (!catalogs.get(locale).has(key)) {
      failures.push(`${file}: t('${key}') is missing in ${locale}.json`);
    }
  }
}

// ── Check 3: dynamic prefixes match at least one key per catalog ─────────────
for (const { prefix, file } of dynamicPrefixes) {
  for (const locale of LOCALES) {
    const hasMatch = [...catalogs.get(locale)].some((key) => key.startsWith(prefix));
    if (!hasMatch) {
      failures.push(
        `${file}: t(\`${prefix}\${…}\`) has no matching key prefix in ${locale}.json`,
      );
    }
  }
}

// ── Helpers ──────────────────────────────────────────────────────────────────
function flattenKeys(obj, prefix = '') {
  const keys = new Set();
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === 'object') {
      for (const nested of flattenKeys(v, key)) keys.add(nested);
    } else {
      keys.add(key);
    }
  }
  return keys;
}

function* walk(dir) {
  for (const entry of readdirSync(dir)) {
    const full = path.join(dir, entry);
    const stat = statSync(full);
    if (stat.isDirectory()) {
      yield* walk(full);
    } else if (/\.(svelte|ts|js)$/.test(entry)) {
      yield full;
    }
  }
}

// ── Report ───────────────────────────────────────────────────────────────────
if (failures.length > 0) {
  console.error('i18n key contract check failed:');
  for (const failure of failures) {
    console.error(`- ${failure}`);
  }
  console.error(`\n${failures.length} issue(s) across ${LOCALES.join(' / ')}.`);
  process.exit(1);
}

console.log(
  `i18n key contract ok: ${staticKeys.length} static keys, ${dynamicPrefixes.length} dynamic prefixes, ` +
    `catalog sizes ${[...catalogs].map(([l, k]) => `${l}=${k.size}`).join(' ')}`,
);
