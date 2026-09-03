import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

// No i18n needed: Pagination's info line is hardcoded English.

import Pagination from './Pagination.svelte';

function renderPager(overrides: Record<string, unknown> = {}) {
  const onPageChange = vi.fn();
  const props = { total: 100, page: 1, perPage: 10, onPageChange, ...overrides };
  render(Pagination, props);
  return { onPageChange };
}

describe('Pagination.svelte', () => {
  it('renders nothing for a single page of items', () => {
    const { container } = render(Pagination, {
      total: 10,
      page: 1,
      perPage: 10,
      onPageChange: () => {},
    });

    expect(container.querySelector('nav.pagination')).not.toBeInTheDocument();
  });

  it('shows the item range summary alongside the page buttons', () => {
    renderPager({ page: 2, total: 95 });

    expect(screen.getByText('11-20 of 95')).toBeInTheDocument();
  });

  it('collapses distant pages into ellipses around the current window', () => {
    // page=5, 10 pages, siblingCount=2 → [1, …, 3, 4, 5, 6, 7, …, 10]
    renderPager({ page: 5, siblingCount: 2 });

    const ellipses = screen.getAllByText('…');
    expect(ellipses.length).toBe(2);
    for (const n of [1, 3, 4, 5, 6, 7, 10]) {
      expect(screen.getByText(String(n))).toBeInTheDocument();
    }
    expect(screen.queryByText('2')).not.toBeInTheDocument();
    expect(screen.queryByText('8')).not.toBeInTheDocument();
  });

  it('disables prev on the first and next on the last page', () => {
    const { rerender } = render(Pagination, {
      total: 100,
      page: 1,
      perPage: 10,
      onPageChange: () => {},
    });
    expect(screen.getByLabelText('Previous page')).toBeDisabled();
    expect(screen.getByLabelText('Next page')).toBeEnabled();

    rerender({ total: 100, page: 10, perPage: 10, onPageChange: () => {} });
    expect(screen.getByLabelText('Previous page')).toBeEnabled();
    expect(screen.getByLabelText('Next page')).toBeDisabled();
  });

  it('delegates page changes but ignores the current page', () => {
    const { onPageChange } = renderPager({ page: 3 });

    fireEvent.click(screen.getByText('4'));
    expect(onPageChange).toHaveBeenCalledWith(4);
    expect(onPageChange).toHaveBeenCalledTimes(1);

    // Clicking the current page is a no-op (guard in goto()).
    fireEvent.click(screen.getByText('3'));
    expect(onPageChange).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByLabelText('Next page'));
    expect(onPageChange).toHaveBeenCalledWith(4);
    expect(onPageChange).toHaveBeenCalledTimes(2);
  });
});
