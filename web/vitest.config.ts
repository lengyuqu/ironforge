import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';
import { resolve } from 'node:path';

export default defineConfig({
  plugins: [svelte()],
  resolve: {
    alias: {
      $lib: resolve(__dirname, './src/lib'),
    },
    // Force Svelte 5 to resolve the client build (mount() available) even
    // when the entry is a plain .svelte.ts file. Vitest with happy-dom
    // provides a real DOM; without this Svelte picks the server build and
    // @testing-library/svelte's render() throws `mount not available`.
    conditions: ['browser', 'development', 'import'],
  },
  test: {
    environment: 'happy-dom',
    globals: true,
    setupFiles: ['./src/test-setup.ts'],
    include: ['src/**/*.{test,spec}.{ts,js,svelte.ts}'],
    testTimeout: 5_000,
    hookTimeout: 5_000,
    // SSR off: we test client-side components.
    server: {
      deps: {
        inline: [/svelte/],
      },
    },
    deps: {
      optimizer: {
        web: {
          include: ['svelte'],
        },
      },
    },
    environmentOptions: {
      happyDom: {
        url: 'http://localhost/',
      },
    },
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html', 'lcovonly'],
      include: ['src/lib/**/*.{ts,svelte,svelte.ts}'],
      exclude: [
        'src/lib/**/*.d.ts',
        'src/lib/i18n/locales/**',
        'src/lib/i18n/translations/**',
        'src/test-setup.ts',
      ],
    },
  },
});
