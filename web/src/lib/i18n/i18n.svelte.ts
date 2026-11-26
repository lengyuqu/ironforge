// i18n module — Svelte 5 runes (migrated from Svelte 4 stores, tech-debt F-013 resolved).
//
// `en.json` stays synchronous so the default render has zero flash and unit
// tests resolve immediately; `zh-CN.json` is code-split and loaded on demand
// when selected or detected from browser preferences.

import en from './translations/en.json';

export type Locale = 'en' | 'zh-CN';

type TranslationCatalog = typeof en;

const SUPPORTED_LOCALES: readonly Locale[] = ['en', 'zh-CN'];

let loadedCatalogs = $state<Partial<Record<Locale, TranslationCatalog>>>({ en });
const pendingLoads = new Map<Locale, Promise<void>>();

function isSupportedLocale(value: string | null): value is Locale {
  return value !== null && (SUPPORTED_LOCALES as readonly string[]).includes(value);
}

function syncDocumentLang(targetLocale: Locale) {
  if (typeof document !== 'undefined') {
    document.documentElement.lang = targetLocale;
  }
}

async function ensureLocaleLoaded(targetLocale: Locale): Promise<void> {
  if (loadedCatalogs[targetLocale]) return;
  let pending = pendingLoads.get(targetLocale);
  if (!pending) {
    pending = (async () => {
      if (targetLocale === 'zh-CN') {
        const mod = await import('./translations/zh-CN.json');
        loadedCatalogs = { ...loadedCatalogs, 'zh-CN': mod.default };
      }
    })().finally(() => {
      pendingLoads.delete(targetLocale);
    });
    pendingLoads.set(targetLocale, pending);
  }
  await pending;
}

// ── Locale detection ──────────────────────────────────────────────

function detectLocale(): Locale {
  if (typeof window === 'undefined') return 'en';
  const stored = localStorage.getItem('locale');
  if (isSupportedLocale(stored)) return stored;
  const browser = navigator.language;
  if (browser.startsWith('zh')) return 'zh-CN';
  return 'en';
}

// ── Reactive core ────────────────────────────────────────────────

const initialLocale = detectLocale();
let currentLocale = $state<Locale>(initialLocale);
const currentCatalog = $derived(loadedCatalogs[currentLocale] ?? en);

syncDocumentLang(initialLocale);
if (initialLocale !== 'en') {
  void ensureLocaleLoaded(initialLocale);
}

export const locale: { value: Locale; set: (l: Locale) => void; init: () => void } = {
  get value() {
    return currentLocale;
  },
  set(newLocale: Locale) {
    if (typeof window !== 'undefined') {
      localStorage.setItem('locale', newLocale);
    }
    currentLocale = newLocale;
    syncDocumentLang(newLocale);
    void ensureLocaleLoaded(newLocale);
  },
  init() {
    const detected = detectLocale();
    currentLocale = detected;
    syncDocumentLang(detected);
    void ensureLocaleLoaded(detected);
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

export function t(key: string, params?: TranslationParams): string;
export function t(key: string, fallback?: string): string;
export function t(key: string, options?: TranslationOptions): string {
  return resolveTranslation(key, options);
}

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
