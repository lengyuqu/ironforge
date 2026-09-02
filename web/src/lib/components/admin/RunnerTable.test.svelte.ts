import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

// The i18n module uses module-level runes in a plain .ts file, which the
// vitest pipeline cannot compile (rune_outside_svelte). Mock it so the
// component under test renders without the reactive locale core.
vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallback?: string) => fallback ?? key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import RunnerTable from './RunnerTable.svelte';
import type { RunnerListItem } from '$lib/api/client.svelte';

const runner: RunnerListItem = {
  id: 7,
  name: 'linux-runner-01',
  status: 'online',
  labels: ['linux', 'docker'],
  last_seen: '2026-09-02T10:00:00Z',
  last_seen_at: '2026-09-02T10:00:00Z',
  version: '1.2.3',
  os: 'linux',
  arch: 'x86_64',
};

describe('RunnerTable.svelte', () => {
  it('renders name, status badge, labels and version', () => {
    render(RunnerTable, { items: [runner], onDelete: () => {} });

    expect(screen.getByText('linux-runner-01')).toBeInTheDocument();
    expect(screen.getByText('online')).toBeInTheDocument();
    expect(screen.getByText('linux')).toBeInTheDocument();
    expect(screen.getByText('docker')).toBeInTheDocument();
    expect(screen.getByText('1.2.3')).toBeInTheDocument();
  });

  it('falls back to "-" for missing version and the t() key for missing last_seen', () => {
    render(RunnerTable, {
      items: [
        {
          ...runner,
          version: null,
          last_seen: '',
          last_seen_at: '',
        },
      ],
      onDelete: () => {},
    });

    expect(screen.getByText('-')).toBeInTheDocument();
    // mocked createT returns the key when no fallback is provided
    expect(screen.getByText('common.never')).toBeInTheDocument();
  });

  it('delegates delete intent with the runner payload', () => {
    const onDelete = vi.fn();
    render(RunnerTable, { items: [runner], onDelete });

    // mocked createT returns keys — the delete button label is the t() key
    fireEvent.click(screen.getByRole('button', { name: 'common.delete' }));
    expect(onDelete).toHaveBeenCalledWith(runner);
  });
});
