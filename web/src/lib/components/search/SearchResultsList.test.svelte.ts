import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

import SearchResultsList from './SearchResultsList.svelte';
import type { SearchResult } from '$lib/api/search';

const results: SearchResult[] = [
  {
    id: 1,
    result_type: 'repo',
    title: 'demo',
    repo_owner: 'alice',
    repo_name: 'demo',
    excerpt: 'A fast demo repo',
    state: null,
    number: null,
  } as unknown as SearchResult,
  {
    id: 2,
    result_type: 'issue',
    title: 'Bug in parser',
    repo_owner: 'alice',
    repo_name: 'demo',
    excerpt: null,
    state: 'open',
    number: 42,
  } as unknown as SearchResult,
  {
    id: 3,
    result_type: 'wiki',
    title: 'Home Page',
    repo_owner: 'alice',
    repo_name: 'demo',
    excerpt: null,
    state: null,
    number: null,
  } as unknown as SearchResult,
];

function renderList(overrides: Record<string, unknown> = {}) {
  render(SearchResultsList, {
    results,
    query: 'demo',
    total: 30,
    currentPage: 2,
    totalPages: 3,
    onPageChange: () => {},
    ...overrides,
  });
}

describe('SearchResultsList.svelte', () => {
  it('renders repo, issue and wiki cards with correct hrefs', () => {
    renderList();

    // The repo-path text appears on every card; the repo title is first.
    const repoTitles = screen.getAllByText('alice/demo');
    expect(repoTitles.length).toBe(3);
    expect(repoTitles[0].closest('a')).toHaveAttribute('href', '/alice/demo');
    expect(repoTitles[0].closest('a')).toHaveAttribute(
      'class',
      expect.stringContaining('result-card')
    );

    const issueTitle = screen.getByText('Bug in parser');
    expect(issueTitle.closest('a')).toHaveAttribute('href', '/alice/demo/issues/42');
    expect(screen.getByText('#42')).toBeInTheDocument();
    // stateLabel falls back to the raw state value.
    expect(screen.getByText('open')).toBeInTheDocument();

    const wikiTitle = screen.getByText('Home Page');
    expect(wikiTitle.closest('a')).toHaveAttribute(
      'href',
      '/alice/demo/wiki/Home%20Page'
    );
  });

  it('paginates with prev/next boundaries', async () => {
    const onPageChange = vi.fn();
    renderList({ onPageChange });

    expect(screen.getByText('← common.previous')).toBeEnabled();
    expect(screen.getByText('common.next →')).toBeEnabled();
    await fireEvent.click(screen.getByText('common.next →'));
    expect(onPageChange).toHaveBeenCalledWith(3);
    await fireEvent.click(screen.getByText('← common.previous'));
    expect(onPageChange).toHaveBeenCalledWith(1);
  });

  it('hides the pagination bar for a single page', () => {
    renderList({ currentPage: 1, totalPages: 1 });

    expect(screen.queryByText('common.next →')).toBeNull();
  });
});
