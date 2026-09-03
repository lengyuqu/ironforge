import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

import WikiSidebar from './WikiSidebar.svelte';
import type { WikiPageSummary } from '$lib/types/entities';

const pages: WikiPageSummary[] = [
  { title: 'Home', path: 'Home' },
  { title: 'Getting Started', path: 'Getting Started' },
] as unknown as WikiPageSummary[];

describe('WikiSidebar.svelte', () => {
  it('renders page links with encoded hrefs and the active marker', () => {
    render(WikiSidebar, {
      owner: 'alice',
      repo: 'demo',
      pages,
      currentTitle: 'Home',
      toc: [],
      onScrollToHeading: () => {},
    });

    expect(screen.getByText('Home').closest('a')).toHaveAttribute(
      'href',
      '/alice/demo/wiki/Home'
    );
    const started = screen.getByText('Getting Started').closest('a')!;
    expect(started).toHaveAttribute('href', '/alice/demo/wiki/Getting%20Started');
    expect(started.className).not.toContain('active');
    expect(screen.getByText('Home').closest('a')!.className).toContain('active');
  });

  it('renders the table of contents and scrolls to headings', async () => {
    const onScrollToHeading = vi.fn();
    render(WikiSidebar, {
      owner: 'alice',
      repo: 'demo',
      pages,
      currentTitle: 'Home',
      toc: [
        { id: 'intro', text: 'Introduction', level: 2 },
        { id: 'setup', text: 'Setup', level: 3 },
      ],
      onScrollToHeading,
    });

    const buttons = screen.getAllByRole('button');
    expect(screen.getByText('Introduction')).toBeInTheDocument();
    expect(buttons[0].getAttribute('style')).toContain('24px'); // level 2 * 12
    expect(buttons[1].getAttribute('style')).toContain('36px'); // level 3 * 12

    await fireEvent.click(screen.getByText('Setup'));
    expect(onScrollToHeading).toHaveBeenCalledWith('setup');
  });

  it('hides the toc section when there are no headings', () => {
    render(WikiSidebar, {
      owner: 'alice',
      repo: 'demo',
      pages,
      currentTitle: 'Home',
      toc: [],
      onScrollToHeading: () => {},
    });

    expect(screen.queryByText('wiki.toc')).toBeNull();
  });
});
