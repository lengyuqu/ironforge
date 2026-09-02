import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

// renderMarkdown is exercised for real: it has its own thorough unit tests
// (markdown.test.ts), so here we only assert the wiring: content flows
// through it into the markdown-body container.

import ReadmeSection from './ReadmeSection.svelte';

describe('ReadmeSection.svelte', () => {
  it('renders markdown content inside the README card', () => {
    const { container } = render(ReadmeSection, { content: '# Title\n\n**bold** text' });

    // Header span carries an emoji prefix — match the full text.
    expect(screen.getByText('📄 README.md')).toBeInTheDocument();
    const h1 = container.querySelector('.markdown-body h1');
    expect(h1?.textContent).toBe('Title');
    expect(container.querySelector('.markdown-body strong')?.textContent).toBe('bold');
  });

  it('shows a loading placeholder while loading with no content yet', () => {
    render(ReadmeSection, { content: null, loading: true });

    expect(screen.getByText('common.loading')).toBeInTheDocument();
    expect(screen.queryByText(/README\.md/)).not.toBeInTheDocument();
  });

  it('renders nothing at all when there is no README and not loading', () => {
    const { container } = render(ReadmeSection, { content: null, loading: false });

    expect(container.querySelector('.readme-section')).not.toBeInTheDocument();
  });
});
