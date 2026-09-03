import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

const wikiHistory = vi.fn();
const wikiRevision = vi.fn();
const wikiUpdate = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  wiki: {
    history: (...a: unknown[]) => wikiHistory(...a),
    revision: (...a: unknown[]) => wikiRevision(...a),
    update: (...a: unknown[]) => wikiUpdate(...a),
  },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

import WikiHistoryPanel from './WikiHistoryPanel.svelte';
import type { WikiRevision } from '$lib/types/entities';

const rev: WikiRevision = {
  id: 7,
  version: 3,
  message: 'fix typo',
  content: '# v3 content',
  created_at: '2026-09-01T00:00:00Z',
} as unknown as WikiRevision;

describe('WikiHistoryPanel.svelte', () => {
  let confirmMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    confirmMock = vi.fn(() => true);
    vi.stubGlobal('confirm', confirmMock);
    wikiHistory.mockReset().mockResolvedValue([rev]);
    wikiRevision.mockReset();
    wikiUpdate.mockReset();
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('loads and renders the revision list', async () => {
    render(WikiHistoryPanel, {
      owner: 'alice',
      repo: 'demo',
      title: 'Home',
      onRestored: () => {},
    });

    await waitFor(() => expect(wikiHistory).toHaveBeenCalledWith('alice', 'demo', 'Home'));
    expect(await screen.findByText('v3')).toBeInTheDocument();
    expect(screen.getByText('fix typo')).toBeInTheDocument();
    expect(screen.getByText('fmt(2026-09-01T00:00:00Z)')).toBeInTheDocument();
  });

  it('shows the empty state when the page has no revisions', async () => {
    wikiHistory.mockResolvedValue([]);
    render(WikiHistoryPanel, {
      owner: 'alice',
      repo: 'demo',
      title: 'Home',
      onRestored: () => {},
    });

    expect(
      await screen.findByText('No revisions yet. Revisions are saved on every edit.')
    ).toBeInTheDocument();
  });

  it('expands a revision with its full content fetched via the API', async () => {
    wikiRevision.mockResolvedValue({ ...rev, content: '# full v3' });
    render(WikiHistoryPanel, {
      owner: 'alice',
      repo: 'demo',
      title: 'Home',
      onRestored: () => {},
    });

    await screen.findByText('v3');
    await fireEvent.click(screen.getByText('fix typo'));
    await waitFor(() =>
      expect(wikiRevision).toHaveBeenCalledWith('alice', 'demo', 'Home', 7)
    );
    expect(await screen.findByText('# full v3')).toBeInTheDocument();
  });

  it('restores the viewing revision after confirmation', async () => {
    wikiRevision.mockResolvedValue({ ...rev, content: '# full v3' });
    const onRestored = vi.fn();
    render(WikiHistoryPanel, { owner: 'alice', repo: 'demo', title: 'Home', onRestored });

    await screen.findByText('v3');
    await fireEvent.click(screen.getByText('fix typo'));
    await screen.findByText('# full v3');

    await fireEvent.click(screen.getByText('Restore this version'));
    await waitFor(() =>
      expect(wikiUpdate).toHaveBeenCalledWith('alice', 'demo', 'Home', '# full v3')
    );
    await waitFor(() => expect(onRestored).toHaveBeenCalledTimes(1));
    expect(toastSuccess).toHaveBeenCalledWith('Version 3 restored');
  });
});
