// Re-export shim: the reactive core lives in i18n.svelte.ts so the
// Svelte plugin compiles its module-level runes (F-013). Keeping this
// index preserves the existing '/i18n' import paths.
export * from './i18n.svelte';
