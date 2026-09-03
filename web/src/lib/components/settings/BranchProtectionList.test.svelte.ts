import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const bpRemove = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  branchProtections: { remove: (...a: unknown[]) => bpRemove(...a) },
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

import BranchProtectionList from './BranchProtectionList.svelte';
import type { BranchProtectionRule } from '$lib/api/branchProtections';

const confirmMock = vi.fn(() => true);

function makeRule(overrides: Partial<BranchProtectionRule> = {}): BranchProtectionRule {
  return {
    id: 3,
    repo_id: 10,
    branch_name: 'release',
    require_pr: true,
    require_status_check: false,
    required_status_checks: null,
    require_approval: true,
    required_approvals: 2,
    allow_force_push: false,
    require_signed_commits: true,
    allowed_push_user_ids: null,
    created_at: '2026-09-01T00:00:00Z',
    updated_at: '2026-09-01T00:00:00Z',
    ...overrides,
  };
}

function renderList(overrides: Record<string, unknown> = {}) {
  const onEdit = vi.fn();
  const onDeleted = vi.fn();
  const onChanged = vi.fn().mockResolvedValue(undefined);
  const props = {
    owner: 'alice',
    repo: 'demo',
    rules: [makeRule()],
    loading: false,
    editingId: null,
    onEdit,
    onDeleted,
    onChanged,
    ...overrides,
  };
  render(BranchProtectionList, props);
  return { onEdit, onDeleted, onChanged };
}

describe('BranchProtectionList.svelte', () => {
  beforeEach(() => {
    bpRemove.mockReset().mockResolvedValue(undefined);
    toastSuccess.mockClear();
    toastError.mockClear();
    confirmMock.mockClear().mockReturnValue(true);
    vi.stubGlobal('confirm', confirmMock);
    return () => vi.unstubAllGlobals();
  });

  it('renders the empty state when there are no rules', () => {
    renderList({ rules: [] });

    expect(screen.getByText('settings.branch_protection.empty')).toBeInTheDocument();
    expect(document.querySelector('table')).not.toBeInTheDocument();
  });

  it('renders one row per rule with only the active protection badges', () => {
    renderList({
      rules: [
        makeRule(),
        makeRule({ id: 4, branch_name: 'dev', require_pr: false, require_approval: false, require_signed_commits: false }),
      ],
    });

    expect(document.querySelectorAll('tbody tr').length).toBe(2);

    const firstBadges = document.querySelectorAll('tbody tr')[0].querySelector('.rule-list');
    expect(firstBadges?.textContent).toContain('settings.branch_protection.require_pr');
    expect(firstBadges?.textContent).toContain('settings.branch_protection.approvals_count');
    expect(firstBadges?.textContent).toContain('Signed commits required');
    // status check / force push are off for this rule → no badges.
    expect(firstBadges?.textContent).not.toContain('status_checks_enabled');
    expect(firstBadges?.textContent).not.toContain('force_push_allowed');

    const secondBadges = document.querySelectorAll('tbody tr')[1].querySelector('.rule-list');
    expect(secondBadges?.textContent).not.toContain('settings.branch_protection.require_pr');
  });

  it('delegates editing to the parent with the rule', () => {
    const { onEdit } = renderList();

    fireEvent.click(screen.getByText('common.edit'));
    expect(onEdit).toHaveBeenCalledTimes(1);
    expect((onEdit.mock.calls[0] as unknown[])[0]).toMatchObject({ id: 3, branch_name: 'release' });
  });

  it('deletes a rule after confirmation, notifying onDeleted and onChanged', async () => {
    const { onDeleted, onChanged } = renderList();

    await fireEvent.click(screen.getByText('common.delete'));
    expect(confirmMock).toHaveBeenCalled();
    await waitFor(() => expect(bpRemove).toHaveBeenCalledWith('alice', 'demo', 3));
    expect(onDeleted).toHaveBeenCalledWith(3);
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
    expect(toastSuccess).toHaveBeenCalledWith('Protection rule deleted');
  });

  it('keeps the rule on confirm dismissal and surfaces API failures via toast', async () => {
    confirmMock.mockReturnValue(false);
    const { onDeleted } = renderList();

    await fireEvent.click(screen.getByText('common.delete'));
    expect(bpRemove).not.toHaveBeenCalled();
    expect(onDeleted).not.toHaveBeenCalled();

    // Confirm path with a failing API call.
    confirmMock.mockReturnValue(true);
    bpRemove.mockRejectedValueOnce(new Error('rule in use'));
    await fireEvent.click(screen.getByText('common.delete'));
    await waitFor(() => expect(toastError).toHaveBeenCalledWith('rule in use'));
    expect(onDeleted).not.toHaveBeenCalled();
  });
});
