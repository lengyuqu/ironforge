import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const updatePermission = vi.fn();
const collaboratorRemove = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  collaborators: {
    updatePermission: (...a: unknown[]) => updatePermission(...a),
    remove: (...a: unknown[]) => collaboratorRemove(...a),
  },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: (...a: unknown[]) => toastSuccess(...a), error: (...a: unknown[]) => toastError(...a) },
}));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import CollaboratorTable from './CollaboratorTable.svelte';
import type { RepoCollaborator } from '$lib/types/entities';

const confirmMock = vi.fn(() => true);

function makeCollaborator(overrides: Partial<RepoCollaborator> = {}): RepoCollaborator {
  return {
    id: 9,
    repo_id: 1,
    user_id: 77,
    permission: 'read',
    created_at: '2026-09-01T00:00:00Z',
    ...overrides,
  };
}

function renderTable(overrides: Record<string, unknown> = {}) {
  const onRefresh = vi.fn().mockResolvedValue(undefined);
  const props = {
    owner: 'alice',
    repo: 'demo',
    collaborators: [makeCollaborator()],
    loading: false,
    onRefresh,
    ...overrides,
  };
  const utils = render(CollaboratorTable, props);
  return { ...utils, onRefresh };
}

describe('CollaboratorTable.svelte', () => {
  beforeEach(() => {
    updatePermission.mockReset().mockResolvedValue(undefined);
    collaboratorRemove.mockReset().mockResolvedValue(undefined);
    toastSuccess.mockClear();
    toastError.mockClear();
    confirmMock.mockClear().mockReturnValue(true);
    vi.stubGlobal('confirm', confirmMock);
    return () => vi.unstubAllGlobals();
  });

  it('renders the empty state when the repo has no collaborators', () => {
    renderTable({ collaborators: [] });

    expect(screen.getByText('settings.collaborators.empty')).toBeInTheDocument();
    expect(document.querySelector('table')).not.toBeInTheDocument();
  });

  it('renders rows with user id and a permission select bound to the value', () => {
    renderTable({ collaborators: [makeCollaborator(), makeCollaborator({ id: 10, user_id: 88, permission: 'admin' })] });

    expect(screen.getByText('#77')).toBeInTheDocument();
    expect(screen.getByText('#88')).toBeInTheDocument();

    const selects = document.querySelectorAll('select');
    expect((selects[0] as HTMLSelectElement).value).toBe('read');
    expect((selects[1] as HTMLSelectElement).value).toBe('admin');
  });

  it('saves the current permission value through updatePermission and refreshes', async () => {
    // happy-dom does not implement the :checked pseudo-class Svelte's
    // select binding relies on, so DOM-level option changes never flow
    // back into state. Change permission at the prop layer instead —
    // the contract under test is "save submits the collaborator's
    // current permission".
    const { onRefresh, rerender } = renderTable();

    rerender({
      owner: 'alice',
      repo: 'demo',
      collaborators: [makeCollaborator({ permission: 'write' })],
      loading: false,
      onRefresh,
    });
    await fireEvent.click(screen.getByText('common.save'));

    await waitFor(() =>
      expect(updatePermission).toHaveBeenCalledWith('alice', 'demo', 9, 'write')
    );
    await waitFor(() => expect(onRefresh).toHaveBeenCalledTimes(1));
    expect(toastSuccess).toHaveBeenCalledWith('settings.collaborators.updated');
  });

  it('removes a collaborator by user_id after confirmation', async () => {
    const { onRefresh } = renderTable();

    await fireEvent.click(screen.getByText('common.delete'));
    expect(confirmMock).toHaveBeenCalled();
    await waitFor(() => expect(collaboratorRemove).toHaveBeenCalledWith('alice', 'demo', 77));
    await waitFor(() => expect(onRefresh).toHaveBeenCalledTimes(1));
  });

  it('routes removal failures to an error toast without refreshing', async () => {
    collaboratorRemove.mockRejectedValueOnce(new Error('forbidden'));
    const { onRefresh } = renderTable();

    await fireEvent.click(screen.getByText('common.delete'));
    await waitFor(() => expect(toastError).toHaveBeenCalledWith('forbidden'));
    expect(onRefresh).not.toHaveBeenCalled();
  });
});
