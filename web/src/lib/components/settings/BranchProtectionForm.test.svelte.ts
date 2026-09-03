import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const bpCreate = vi.fn();
const bpUpdate = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  branchProtections: {
    create: (...a: unknown[]) => bpCreate(...a),
    update: (...a: unknown[]) => bpUpdate(...a),
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

import BranchProtectionForm from './BranchProtectionForm.svelte';
import type { BranchProtectionRule } from '$lib/api/branchProtections';

function makeRule(overrides: Partial<BranchProtectionRule> = {}): BranchProtectionRule {
  return {
    id: 3,
    repo_id: 10,
    branch_name: 'release',
    require_pr: true,
    require_status_check: true,
    required_status_checks: '["test", "lint"]',
    require_approval: true,
    required_approvals: 2,
    allow_force_push: false,
    require_signed_commits: true,
    allowed_push_user_ids: '[42, 108]',
    created_at: '2026-09-01T00:00:00Z',
    updated_at: '2026-09-01T00:00:00Z',
    ...overrides,
  };
}

function renderForm(editingRule: BranchProtectionRule | null = null) {
  const onSaved = vi.fn();
  const onCancel = vi.fn();
  render(BranchProtectionForm, { owner: 'alice', repo: 'demo', editingRule, onSaved, onCancel });
  return { onSaved, onCancel };
}

describe('BranchProtectionForm.svelte', () => {
  beforeEach(() => {
    bpCreate.mockReset().mockResolvedValue(undefined);
    bpUpdate.mockReset().mockResolvedValue(undefined);
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  it('create mode: renders defaults without a cancel button', () => {
    renderForm(null);

    expect(screen.getByText('settings.branch_protection.create_title')).toBeInTheDocument();
    expect(screen.getByLabelText('settings.branch_protection.branch')).toHaveValue('main');
    // require_pr and require_approval default to on.
    const checks = document.querySelectorAll<HTMLInputElement>('.check-row input[type="checkbox"]');
    expect(checks[0].checked).toBe(true);
    expect(checks[1].checked).toBe(true);
    expect(screen.queryByText('common.cancel')).not.toBeInTheDocument();
  });

  it('create mode: submits with parsed lists and undefined approvals when unchecked', async () => {
    const { onSaved } = renderForm(null);

    fireEvent.input(screen.getByLabelText('settings.branch_protection.required_checks'), {
      target: { value: ' test , lint ,,' },
    });
    fireEvent.input(screen.getByLabelText('settings.branch_protection.allowed_pushers'), {
      target: { value: '42, 108, abc, 0' }, // abc and 0 filtered out
    });
    // Turn require_approval off → required_approvals must be undefined.
    const approvalCheck = document.querySelectorAll<HTMLInputElement>('.check-row input[type="checkbox"]')[1];
    await fireEvent.change(approvalCheck, { target: { checked: false } });

    await fireEvent.submit(
      screen.getByRole('button', { name: 'common.save' }).closest('form')!
    );

    await waitFor(() => expect(bpCreate).toHaveBeenCalledTimes(1));
    expect(bpCreate).toHaveBeenCalledWith('alice', 'demo', {
      branch_name: 'main',
      require_pr: true,
      require_status_check: false,
      required_status_checks: ['test', 'lint'],
      require_approval: false,
      required_approvals: undefined,
      allow_force_push: false,
      require_signed_commits: false,
      allowed_push_user_ids: [42, 108],
    });
    expect(toastSuccess).toHaveBeenCalledWith('Protection rule created');
    expect(onSaved).toHaveBeenCalledTimes(1);
  });

  it('edit mode: prefills from the rule, locks the branch and omits branch_name on update', async () => {
    const { onSaved } = renderForm(makeRule());

    expect(screen.getByText('settings.branch_protection.edit_title')).toBeInTheDocument();
    expect(screen.getByLabelText('settings.branch_protection.branch')).toHaveValue('release');
    expect(screen.getByLabelText('settings.branch_protection.branch')).toBeDisabled();
    // JSON arrays are unpacked to comma lists.
    expect(screen.getByLabelText('settings.branch_protection.required_checks')).toHaveValue('test, lint');
    expect(screen.getByLabelText('settings.branch_protection.allowed_pushers')).toHaveValue('42, 108');
    expect(screen.getByText('common.cancel')).toBeInTheDocument();

    await fireEvent.submit(
      screen.getByRole('button', { name: 'common.save' }).closest('form')!
    );

    await waitFor(() => expect(bpUpdate).toHaveBeenCalledTimes(1));
    // update(owner, repo, ruleId, payload) — payload is the 4th argument.
    const [, , , payload] = bpUpdate.mock.calls[0] as unknown as [
      string,
      string,
      number,
      Record<string, unknown>
    ];
    expect(payload).not.toHaveProperty('branch_name');
    expect(payload.required_status_checks).toEqual(['test', 'lint']);
    expect(payload.allowed_push_user_ids).toEqual([42, 108]);
    expect(payload.required_approvals).toBe(2);
    expect(toastSuccess).toHaveBeenCalledWith('Protection rule updated');
    expect(onSaved).toHaveBeenCalledTimes(1);
  });

  it('blocks empty branch names with an error toast and no API call', async () => {
    const { onSaved } = renderForm(null);

    fireEvent.input(screen.getByLabelText('settings.branch_protection.branch'), {
      target: { value: '   ' },
    });
    await fireEvent.submit(
      screen.getByRole('button', { name: 'common.save' }).closest('form')!
    );

    expect(toastError).toHaveBeenCalledWith('Branch name is required');
    expect(bpCreate).not.toHaveBeenCalled();
    expect(onSaved).not.toHaveBeenCalled();
  });
});
