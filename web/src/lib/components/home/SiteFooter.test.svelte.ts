import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import SiteFooter from './SiteFooter.svelte';

describe('SiteFooter.svelte', () => {
  it('renders brand, external links and the copyright line', () => {
    render(SiteFooter);

    expect(screen.getByText('IronForge')).toBeInTheDocument();
    expect(screen.getByText('home.footer.github')).toHaveAttribute(
      'href',
      'https://github.com/lengyuqu/ironforge'
    );
    expect(screen.getByText('home.footer.explore')).toHaveAttribute('href', '/explore');
    // External links open safely in a new tab.
    const ext = screen.getByText('home.footer.github');
    expect(ext).toHaveAttribute('target', '_blank');
    expect(ext).toHaveAttribute('rel', 'noreferrer');
    expect(screen.getByText(/© 2026 IronForge\./)).toBeInTheDocument();
  });
});
