import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import MirrorStatusPanel from './MirrorStatusPanel.svelte';
import type { RepositoryMirror } from '$lib/api/mirrors';

function makeMirror(overrides: Partial<RepositoryMirror> = {}): RepositoryMirror {
  return {
    id: 1,
    repo_id: 1,
    url: 'https://upstream.example/demo.git',
    username: null,
    sync_interval_seconds: 3600,
    next_sync_at: '2026-09-03T12:00:00Z',
    last_sync_at: '2026-09-02T12:00:00Z',
    last_sync_error: null,
    status: 'idle',
    created_at: '2026-09-01T00:00:00Z',
    updated_at: '2026-09-01T00:00:00Z',
    ...overrides,
  };
}

describe('MirrorStatusPanel.svelte', () => {
  it('renders the mirror state and both sync timestamps', () => {
    render(MirrorStatusPanel, { mirror: makeMirror() });

    expect(screen.getByText('idle')).toBeInTheDocument();
    // Timestamps are formatted via toLocaleString() — any localized digits
    // instead of the raw ISO string or the "never" placeholder.
    const dds = document.querySelectorAll('dd');
    expect(dds.length).toBe(3);
    expect(dds[1].textContent ?? '').toMatch(/\d/);
    expect(dds[1].textContent).not.toContain('2026-09-02T12:00:00Z');
    expect(dds[2].textContent ?? '').toMatch(/\d/);
  });

  it('shows the never placeholder for null timestamps and hides error detail', () => {
    render(MirrorStatusPanel, {
      mirror: makeMirror({ last_sync_at: null, next_sync_at: null, last_sync_error: null }),
    });

    // t() mock returns the key when no fallback is provided.
    expect(screen.getAllByText('common.never').length).toBe(2);
    expect(document.querySelector('.error-detail')).not.toBeInTheDocument();
  });

  it('highlights error state and surfaces the last sync error message', () => {
    render(MirrorStatusPanel, {
      mirror: makeMirror({ status: 'error', last_sync_error: 'auth failed' }),
    });

    const stateEl = document.querySelector('dd span');
    expect(stateEl?.textContent).toBe('error');
    expect(stateEl?.classList.contains('error-state')).toBe(true);
    expect(screen.getByText('auth failed')).toBeInTheDocument();
  });
});
