import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import StatsSection from './StatsSection.svelte';

describe('StatsSection.svelte', () => {
  it('renders the four highlight numbers with their i18n labels', () => {
    render(StatsSection);

    for (const number of ['50MB', '1', '100%', 'Rust']) {
      expect(screen.getByText(number)).toBeInTheDocument();
    }
    expect(document.querySelectorAll('.stat-item').length).toBe(4);
    // Labels come from t() → keys.
    expect(screen.getByText('home.stats.memory')).toBeInTheDocument();
    expect(screen.getByText('home.stats.language')).toBeInTheDocument();
  });
});
