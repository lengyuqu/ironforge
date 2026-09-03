// Test stub for the SvelteKit virtual module $app/environment.
// The real module re-exports from '__sveltekit/environment', which cannot
// resolve under vitest. happy-dom simulates a browser, so browser=true.
export const browser = true;
export const dev = true;
export const building = false;
export const version = 'test';
