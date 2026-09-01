// Vitest setup file — runs once before every test file.
//
// - Imports @testing-library/jest-dom for custom Jest matchers on HTMLElement
//   (toBeInTheDocument, toHaveTextContent, toBeVisible, etc.).
// - Exposes a minimal `globalThis` cleanup hook so Svelte 5 component tests
//   always start from a clean document.

import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/svelte';
import { afterEach } from 'vitest';

afterEach(() => {
  cleanup();
  // Reset any leftover body content from test components that render directly.
  while (document.body.firstChild) {
    document.body.removeChild(document.body.firstChild);
  }
});
