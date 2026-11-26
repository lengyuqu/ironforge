import { describe, it, expect, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

vi.mock('$lib/utils/markdown', () => ({
  renderMarkdown: vi.fn((s: string) => `<p>${s}</p>`),
}));

const highlightCodeToLines = vi.fn(async (code: string, _lang?: string) =>
  code.split('\n').map((line) => `<span class="hljs-comment">${line}</span>`),
);
vi.mock('$lib/utils/highlight', () => ({
  highlightCodeToLines: (...a: [string, string]) => highlightCodeToLines(...a),
}));

import BlobContentView from './BlobContentView.svelte';
import type { BlobContent } from '$lib/types/entities';

function makeBlob(overrides: Partial<BlobContent> = {}): BlobContent {
  return {
    path: 'src/main.rs',
    sha: 'abc123',
    size: 100,
    content: '',
    encoding: 'utf-8',
    is_binary: false,
    ...overrides,
  } as BlobContent;
}

const baseProps = {
  filePath: 'src/main.rs',
  isText: true,
  isMarkdown: false,
  viewMode: 'source' as const,
};

describe('BlobContentView.svelte', () => {
  it('renders the binary placeholder for binary blobs', () => {
    render(BlobContentView, {
      ...baseProps,
      blob: makeBlob({ is_binary: true }),
    });
    expect(screen.getByText('repo.blob.binary_file')).toBeInTheDocument();
    expect(document.querySelector('.code-view')).not.toBeInTheDocument();
  });

  it('renders line-numbered code rows and applies per-line highlight output', async () => {
    render(BlobContentView, {
      ...baseProps,
      blob: makeBlob({ content: 'fn main() {\n    println!("hi");\n}' }),
    });

    const lineNumbers = document.querySelectorAll('.line-number');
    expect(lineNumbers.length).toBe(3);
    expect(screen.getByText('fn main() {')).toBeInTheDocument();
    expect(document.querySelector('.code-view')).toBeInTheDocument();
    await waitFor(() => expect(highlightCodeToLines).toHaveBeenCalled());
    await waitFor(() => expect(document.querySelectorAll('.code-view .hljs-comment')).toHaveLength(3));
  });

  it('renders the markdown body when in rendered mode', () => {
    render(BlobContentView, {
      ...baseProps,
      isText: true,
      isMarkdown: true,
      viewMode: 'rendered',
      blob: makeBlob({ content: '# Title' }),
    });

    expect(document.querySelector('.markdown-body')).toBeInTheDocument();
    expect(document.querySelector('.code-view')).not.toBeInTheDocument();
  });

  it('renders the empty placeholder when there is no content', () => {
    render(BlobContentView, {
      ...baseProps,
      blob: makeBlob({ content: '' }),
    });
    expect(screen.getByText('repo.blob.empty')).toBeInTheDocument();
  });
});
