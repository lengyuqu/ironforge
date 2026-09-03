import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import FeaturesSection from './FeaturesSection.svelte';

describe('FeaturesSection.svelte', () => {
  it('renders the section title and six feature cards with icons', () => {
    render(FeaturesSection);

    expect(screen.getByText('home.features.title')).toBeInTheDocument();
    expect(document.querySelectorAll('.feature-card').length).toBe(6);

    // Each card pairs an icon with its i18n title and description.
    for (const key of ['lightweight', 'all_in_one', 'security', 'self_hosted', 'fast', 'extensible']) {
      expect(screen.getByText(`home.features.${key}.title`)).toBeInTheDocument();
      expect(screen.getByText(`home.features.${key}.desc`)).toBeInTheDocument();
    }
    expect(document.querySelector('.feature-icon')?.textContent).toContain('🚀');
  });
});
