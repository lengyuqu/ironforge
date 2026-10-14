/**
 * i18n catalog contract.
 *
 * `scripts/i18n-key-check.mjs` performs the full static analysis (every
 * static translation call site must resolve) and runs as a CI step. This test
 * adds a faster, dependency-free guard on the catalog data itself so a missing
 * key also fails the vitest suite:
 *
 *   1. `en` and `zh-CN` must expose exactly the same key set — a key added to
 *      only one locale silently renders as the raw key (or the English
 *      fallback) for the other.
 *   2. Placeholder tokens (`{name}`) must match between the two locales, or
 *      interpolation silently drops the value in one language.
 *
 * Regression context: the token/pipeline/review component refactor added 52
 * `t()` calls without adding the keys to either catalog, so Chinese users saw
 * hardcoded English fallbacks and CI went red.
 */
import { describe, expect, it } from 'vitest';

import en from './translations/en.json';
import zh from './translations/zh-CN.json';

type Catalog = Record<string, unknown>;

function flatten(obj: Catalog, prefix = ''): Map<string, string> {
  const out = new Map<string, string>();
  for (const [key, value] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (value && typeof value === 'object' && !Array.isArray(value)) {
      for (const [k, v] of flatten(value as Catalog, path)) out.set(k, v);
    } else if (typeof value === 'string') {
      out.set(path, value);
    }
  }
  return out;
}

function placeholders(text: string): string[] {
  return [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
}

const enKeys = flatten(en as Catalog);
const zhKeys = flatten(zh as Catalog);

describe('i18n catalogs', () => {
  it('exposes identical key sets in en and zh-CN', () => {
    const onlyEn = [...enKeys.keys()].filter((k) => !zhKeys.has(k)).sort();
    const onlyZh = [...zhKeys.keys()].filter((k) => !enKeys.has(k)).sort();
    expect(onlyEn, `keys missing from zh-CN.json: ${onlyEn.join(', ')}`).toEqual([]);
    expect(onlyZh, `keys missing from en.json: ${onlyZh.join(', ')}`).toEqual([]);
  });

  it('uses matching interpolation placeholders in both locales', () => {
    const mismatched: string[] = [];
    for (const [key, enText] of enKeys) {
      const zhText = zhKeys.get(key);
      if (zhText === undefined) continue;
      const a = placeholders(enText).join(',');
      const b = placeholders(zhText).join(',');
      if (a !== b) mismatched.push(`${key}: en{${a}} vs zh{${b}}`);
    }
    expect(mismatched, `placeholder mismatch:\n${mismatched.join('\n')}`).toEqual([]);
  });

  it('has no empty translation values', () => {
    const empty: string[] = [];
    for (const [key, text] of [...enKeys, ...zhKeys]) {
      if (text.trim() === '') empty.push(key);
    }
    expect(empty, `empty translations: ${empty.join(', ')}`).toEqual([]);
  });
});
