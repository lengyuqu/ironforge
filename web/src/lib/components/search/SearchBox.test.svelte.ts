import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import SearchBox from './SearchBox.svelte';

function renderBox(overrides: Record<string, unknown> = {}) {
  const onQueryChange = vi.fn();
  const onSearch = vi.fn();
  const onTypeChange = vi.fn();
  const props = {
    query: 'initial',
    onQueryChange,
    onSearch,
    activeType: 'all',
    onTypeChange,
    ...overrides,
  };
  render(SearchBox, props);
  return { onQueryChange, onSearch, onTypeChange };
}

describe('SearchBox.svelte', () => {
  it('binds the query value and placeholder to the input', () => {
    renderBox();

    const input = screen.getByPlaceholderText('search.placeholder');
    expect(input).toHaveValue('initial');
  });

  it('bubbles keystrokes to onQueryChange and Enter to onSearch', () => {
    const { onQueryChange, onSearch } = renderBox();

    fireEvent.input(screen.getByPlaceholderText('search.placeholder'), {
      target: { value: 'repo:alice/demo bug' },
    });
    expect(onQueryChange).toHaveBeenCalledWith('repo:alice/demo bug');

    fireEvent.keyDown(screen.getByPlaceholderText('search.placeholder'), { key: 'Enter' });
    expect(onSearch).toHaveBeenCalledTimes(1);

    // Other keys do not trigger a search.
    fireEvent.keyDown(screen.getByPlaceholderText('search.placeholder'), { key: 'a' });
    expect(onSearch).toHaveBeenCalledTimes(1);
  });

  it('toggles the qualifier help panel with the ? button', () => {
    renderBox();

    expect(document.querySelector('.search-help')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTitle('Search help'));
    expect(document.querySelector('.search-help')).toBeInTheDocument();
    expect(screen.getByText('repo:owner/name')).toBeInTheDocument();

    fireEvent.click(screen.getByTitle('Search help'));
    expect(document.querySelector('.search-help')).not.toBeInTheDocument();
  });

  it('renders the four type tabs, highlighting only the active one', () => {
    const { onTypeChange } = renderBox({ activeType: 'issues' });

    for (const label of ['search.all', 'search.repos', 'search.issues', 'search.wiki']) {
      expect(screen.getByText(label)).toBeInTheDocument();
    }
    const active = document.querySelector('.type-tab.active');
    expect(active?.textContent?.trim()).toBe('search.issues');

    fireEvent.click(screen.getByText('search.wiki'));
    expect(onTypeChange).toHaveBeenCalledWith('wiki');
  });
});
