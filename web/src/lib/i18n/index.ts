// i18n module — Svelte 5 runes (migrated from Svelte 4 stores, tech-debt F-013 resolved).
//
// Module-level `$state` / `$derived` give us a single reactive locale that
// every component shares without Svelte 4 writable/derived or `get()`.
// `createT()` closes over `currentCatalog`, so locale changes propagate to
// every component that calls `t()` — no stale snapshots.

import en from './translations/en.json';
import zhCN from './translations/zh-CN.json';

export type Locale = 'en' | 'zh-CN';

type TranslationCatalog = typeof en;

const translations: Record<Locale, TranslationCatalog> = {
  en,
  'zh-CN': zhCN,
};

// ── Locale detection ──────────────────────────────────────────────

function detectLocale(): Locale {
  if (typeof window === 'undefined') return 'en';
  const stored = localStorage.getItem('locale') as Locale | null;
  if (stored && translations[stored]) return stored;
  const browser = navigator.language;
  if (browser.startsWith('zh')) return 'zh-CN';
  return 'en';
}

// ── Reactive core ────────────────────────────────────────────────
//
// These are module-level runes.  Svelte 5's runtime sees them the same
// way it sees component-scoped `$state` / `$derived` — every read inside
// a component template or `$effect` is tracked automatically.

let currentLocale = $state<Locale>(detectLocale());
const currentCatalog = $derived(translations[currentLocale]);

// Public `locale` object — a mutable ref that exposes both read and write.
// Navbar & other components call `locale.set('zh-CN')`; template code uses
// `locale.value` (runes don't support the Svelte-4 `$locale` auto-subscribe
// shorthand for plain objects, but `.value` is always reactive).
export const locale: { value: Locale; set: (l: Locale) => void; init: () => void } = {
  get value() {
    return currentLocale;
  },
  set(newLocale: Locale) {
    if (typeof window !== 'undefined') {
      localStorage.setItem('locale', newLocale);
    }
    currentLocale = newLocale;
  },
  init() {
    currentLocale = detectLocale();
  },
};

// ── Translation helpers ──────────────────────────────────────────

type TranslationParams = Record<string, string | number>;
type TranslationOptions = TranslationParams | string;
type Translator = {
  (key: string, params?: TranslationParams): string;
  (key: string, fallback?: string): string;
};

function getNestedValue(obj: unknown, path: string): string | undefined {
  return path.split('.').reduce((acc: unknown, part) => (acc as Record<string, unknown>)?.[part], obj) as string | undefined;
}

function interpolate(str: string, params: Record<string, string | number>): string {
  return str.replace(/\{(\w+)\}/g, (_, key) => String(params[key] ?? `{${key}}`));
}

/**
 * Resolve a translation key against the currently-active catalog.
 * Called every time `t()` runs inside `createT()` — because `currentCatalog`
 * is a `$derived`, the read is tracked and components re-render on locale
 * switch (no stale snapshot from `get()`).
 */
function resolveTranslation(key: string, options?: TranslationOptions): string {
  const value = getNestedValue(currentCatalog, key);
  const fallback = typeof options === 'string' ? options : key;
  if (typeof value !== 'string') {
    if (import.meta.env.DEV) {
      console.warn(`[i18n] Missing translation: "${key}"`);
    }
    return fallback;
  }
  if (options && typeof options !== 'string') {
    return interpolate(value, options);
  }
  return value;
}

// Main standalone t() — used outside Svelte components (APIs, utils).
// **Not reactive** — reads the current locale once.  Prefer `createT()`
// inside components.
export function t(key: string, params?: TranslationParams): string;
export function t(key: string, fallback?: string): string;
export function t(key: string, options?: TranslationOptions): string {
  return resolveTranslation(key, options);
}

/**
 * Returns a `t()` function that's reactive inside Svelte 5 components.
 * Closing over `currentCatalog` (a `$derived`) means each read is tracked
 * by Svelte's runtime — components that use `t('nav.dashboard')` re-render
 * automatically when `locale.set()` is called.
 */
export function createT(): Translator {
  const translate: Translator = (key: string, options?: TranslationOptions): string => {
    return resolveTranslation(key, options);
  };
  return translate;
}

// ── Date/number formatting ───────────────────────────────────────

export function formatDate(dateStr: string, options?: Intl.DateTimeFormatOptions): string {
  const date = new Date(dateStr);
  const defaultOptions: Intl.DateTimeFormatOptions = { year: 'numeric', month: 'short', day: 'numeric' };
  return date.toLocaleDateString(currentLocale === 'zh-CN' ? 'zh-CN' : 'en-US', options ?? defaultOptions);
}

export function formatDateTime(dateStr: string): string {
  const date = new Date(dateStr);
  return date.toLocaleString(currentLocale === 'zh-CN' ? 'zh-CN' : 'en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}
