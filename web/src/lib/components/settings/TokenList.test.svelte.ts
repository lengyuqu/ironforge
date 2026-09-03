import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const tokensDelete = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  tokens: { delete: (...a: unknown[]) => tokensDelete(...a) },
}));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import TokenList from './TokenList.svelte';
import type { AccessToken } from '$lib/api/tokens';

const confirmMock = vi.fn(() => true);

function makeToken(overrides: Partial<AccessToken> = {}): AccessToken {
  return {
    id: 5,
    name: 'ci-token',
    scopes: 'repo,admin',
    expires_at: null,
    last_used_at: null,
    created_at: '2026-09-01T00:00:00Z',
    ...overrides,
  };
}

function renderList(overrides: Record<string, unknown> = {}) {
  const onRefresh = vi.fn().mockResolvedValue(undefined);
  const props = {
    tokenList: [makeToken()],
    loading: false,
    onRefresh,
    ...overrides,
  };
  render(TokenList, props);
  return { onRefresh };
}

describe('TokenList.svelte', () => {
  beforeEach(() => {
    tokensDelete.mockReset().mockResolvedValue(undefined);
    confirmMock.mockClear().mockReturnValue(true);
    vi.stubGlobal('confirm', confirmMock);
    return () => vi.unstubAllGlobals();
  });

  it('renders the empty state and hides the table when there are no tokens', () => {
    renderList({ tokenList: [] });

    expect(screen.getByText('No personal access tokens yet.')).toBeInTheDocument();
    expect(document.querySelector('table')).not.toBeInTheDocument();
  });

  it('renders one row per token with name, scopes and date fallbacks', () => {
    renderList({
      tokenList: [
        makeToken(),
        makeToken({ id: 6, name: 'deploy', scopes: 'repo', last_used_at: '2026-09-02T00:00:00Z' }),
      ],
    });

    expect(screen.getAllByText('ci-token').length).toBe(1);
    expect(screen.getByText('deploy')).toBeInTheDocument();
    expect(screen.getByText('repo,admin')).toBeInTheDocument();

    // Untouched dates fall back to the "Never" placeholder
    // (token 1: expires + last_used; token 2: expires only).
    expect(screen.getAllByText('Never').length).toBe(3);
    // A real last_used date renders localized (locale-agnostic check):
    // digits, not the raw ISO string, and not the Never placeholder.
    const row = screen.getByText('deploy').closest('tr') as HTMLTableRowElement;
    const lastUsedCell = row.querySelectorAll('td')[3];
    expect(lastUsedCell.textContent ?? '').toMatch(/\d/);
    expect(lastUsedCell.textContent).not.toContain('2026-09-02T00:00:00Z');
    expect(lastUsedCell.textContent).not.toBe('Never');
  });

  it('revokes a token after confirmation and refreshes', async () => {
    const { onRefresh } = renderList();

    await fireEvent.click(screen.getByText('Revoke'));
    expect(confirmMock).toHaveBeenCalled();
    await waitFor(() => expect(tokensDelete).toHaveBeenCalledWith(5));
    await waitFor(() => expect(onRefresh).toHaveBeenCalledTimes(1));
  });

  it('surfaces a revoke failure in the inline error box', async () => {
    tokensDelete.mockRejectedValueOnce(new Error('denied'));
    renderList();

    await fireEvent.click(screen.getByText('Revoke'));
    await waitFor(() => expect(screen.getByText('denied')).toBeInTheDocument());
  });
});
