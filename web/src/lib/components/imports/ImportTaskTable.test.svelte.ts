import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDateTime: (iso: string) => `fmtt(${iso})`,
}));

const importsRemove = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  imports: { remove: (...a: unknown[]) => importsRemove(...a) },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

import ImportTaskTable from './ImportTaskTable.svelte';
import type { ImportTask } from '$lib/api/client.svelte';

const task = {
  id: 3,
  platform: 'github',
  source_url: 'https://github.com/example/project',
  target_owner: 'alice',
  target_name: 'project',
  status: 'running',
  progress: 42,
  stage: 'importing issues',
  error: null,
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-01T01:00:00Z',
} as unknown as ImportTask;

describe('ImportTaskTable.svelte', () => {
  let confirmMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    confirmMock = vi.fn(() => true);
    vi.stubGlobal('confirm', confirmMock);
    importsRemove.mockReset();
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders source/target links, status, progress and stage', () => {
    render(ImportTaskTable, { tasks: [task], onDeleted: () => {} });

    expect(screen.getByText('github')).toBeInTheDocument();
    expect(
      screen.getByText('https://github.com/example/project')
    ).toHaveAttribute('href', 'https://github.com/example/project');
    expect(screen.getByText('alice/project').closest('a')).toHaveAttribute(
      'href',
      '/alice/project'
    );
    expect(screen.getByText('running')).toBeInTheDocument();
    expect(screen.getByText('42%')).toBeInTheDocument();
    expect(screen.getByText('importing issues')).toBeInTheDocument();
    expect(screen.getByText('fmtt(2026-09-01T01:00:00Z)')).toBeInTheDocument();
  });

  it('encodes scoped owners and names in the target link', () => {
    render(ImportTaskTable, {
      tasks: [{ ...task, target_owner: 'my org', target_name: 'a/b' } as ImportTask],
      onDeleted: () => {},
    });

    expect(screen.getByText('my org/a/b').closest('a')).toHaveAttribute(
      'href',
      '/my%20org/a%2Fb'
    );
  });

  it('shows the task error message when present', () => {
    render(ImportTaskTable, {
      tasks: [{ ...task, error: 'rate limited' } as ImportTask],
      onDeleted: () => {},
    });

    expect(screen.getByText('rate limited')).toBeInTheDocument();
  });

  it('deletes a task after confirmation and reports the id', async () => {
    importsRemove.mockResolvedValue(undefined);
    const onDeleted = vi.fn();
    render(ImportTaskTable, { tasks: [task], onDeleted });

    await fireEvent.click(screen.getByText('Delete'));
    await waitFor(() => expect(importsRemove).toHaveBeenCalledWith(3));
    await waitFor(() => expect(onDeleted).toHaveBeenCalledWith(3));
    expect(toastSuccess).toHaveBeenCalledWith('Import deleted');
  });

  it('skips deletion when the confirm dialog is declined', async () => {
    confirmMock.mockReturnValue(false);
    render(ImportTaskTable, { tasks: [task], onDeleted: () => {} });

    await fireEvent.click(screen.getByText('Delete'));
    expect(importsRemove).not.toHaveBeenCalled();
  });
});
