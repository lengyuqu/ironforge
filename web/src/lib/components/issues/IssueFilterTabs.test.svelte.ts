import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import IssueFilterTabs from './IssueFilterTabs.svelte';

describe('IssueFilterTabs.svelte', () => {
  it('renders exactly the open/closed/all tabs', () => {
    render(IssueFilterTabs, { filter: 'open', onFilterChange: () => {} });

    expect(screen.getByText('issues.tabs.open')).toBeInTheDocument();
    expect(screen.getByText('issues.tabs.closed')).toBeInTheDocument();
    expect(screen.getByText('issues.tabs.all')).toBeInTheDocument();
    // IssueFilterTabs 没有 merged 维度（PR 专属），不应出现
    expect(screen.queryByText('issues.tabs.merged')).not.toBeInTheDocument();
  });

  it('marks only the active filter', () => {
    render(IssueFilterTabs, { filter: 'closed', onFilterChange: () => {} });

    const active = document.querySelector('.filter-btn.active');
    expect(active?.textContent?.trim()).toBe('issues.tabs.closed');
  });

  it('delegates filter changes to the parent', () => {
    const onFilterChange = vi.fn();
    render(IssueFilterTabs, { filter: 'open', onFilterChange });

    fireEvent.click(screen.getByText('issues.tabs.all'));
    expect(onFilterChange).toHaveBeenCalledWith('all');
  });
});
