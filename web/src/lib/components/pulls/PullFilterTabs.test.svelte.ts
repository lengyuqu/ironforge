import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import PullFilterTabs from './PullFilterTabs.svelte';

describe('PullFilterTabs.svelte', () => {
  it('renders exactly the open/closed/merged tabs', () => {
    render(PullFilterTabs, { filter: 'open', onFilterChange: () => {} });

    expect(screen.getByText('pulls.tabs.open')).toBeInTheDocument();
    expect(screen.getByText('pulls.tabs.closed')).toBeInTheDocument();
    expect(screen.getByText('pulls.tabs.merged')).toBeInTheDocument();
    expect(screen.queryByText('pulls.tabs.all')).not.toBeInTheDocument();
  });

  it('marks only the active filter', () => {
    render(PullFilterTabs, { filter: 'closed', onFilterChange: () => {} });

    const active = document.querySelector('.filter-btn.active');
    expect(active?.textContent?.trim()).toBe('pulls.tabs.closed');
  });

  it('delegates filter changes to the parent', () => {
    const onFilterChange = vi.fn();
    render(PullFilterTabs, { filter: 'open', onFilterChange });

    fireEvent.click(screen.getByText('pulls.tabs.merged'));
    expect(onFilterChange).toHaveBeenCalledWith('merged');
  });
});
