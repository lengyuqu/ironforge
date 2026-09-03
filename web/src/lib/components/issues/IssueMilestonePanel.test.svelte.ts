import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const updateMock = vi.fn();
const listMock = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  issues: { update: (...a: unknown[]) => updateMock(...a) },
  milestones: { list: (...a: unknown[]) => listMock(...a) },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

import IssueMilestonePanel from './IssueMilestonePanel.svelte';
import type { Milestone } from '$lib/types/entities';

const openMilestones: Milestone[] = [
  { id: 5, repo_id: 1, title: 'v1', state: 'open' },
  { id: 6, repo_id: 1, title: 'v2', state: 'open' },
];

describe('IssueMilestonePanel.svelte', () => {
  beforeEach(() => {
    updateMock.mockReset();
    listMock.mockReset();
    toastSuccess.mockClear();
    toastError.mockClear();
    listMock.mockResolvedValue(openMilestones);
    updateMock.mockResolvedValue(undefined);
  });

  it('resolves the current milestone title from the loaded list', async () => {
    render(IssueMilestonePanel, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      milestoneId: 5,
      onChanged: vi.fn(),
    });

    await waitFor(() => expect(screen.getByText('v1')).toBeInTheDocument());
    expect(listMock).toHaveBeenCalledWith('acme', 'web');
  });

  it('shows the none label when there is no milestone', async () => {
    render(IssueMilestonePanel, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      milestoneId: null,
      onChanged: vi.fn(),
    });

    await waitFor(() => expect(screen.getByText('issues.milestone_none')).toBeInTheDocument());
  });

  it('saves the selected milestone id', async () => {
    const onChanged = vi.fn().mockResolvedValue(undefined);
    render(IssueMilestonePanel, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      milestoneId: 5,
      onChanged,
    });

    await waitFor(() => expect(screen.getByText('v1')).toBeInTheDocument());
    await fireEvent.click(screen.getByText('issues.milestone_edit'));
    await fireEvent.click(screen.getByText('issues.milestone_save'));

    await waitFor(() =>
      expect(updateMock).toHaveBeenCalledWith('acme', 'web', 7, { milestone_id: 5 })
    );
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith('Milestone updated'));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('clears the milestone by sending null', async () => {
    const onChanged = vi.fn().mockResolvedValue(undefined);
    render(IssueMilestonePanel, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      milestoneId: null,
      onChanged,
    });

    await waitFor(() => expect(screen.getByText('issues.milestone_none')).toBeInTheDocument());
    await fireEvent.click(screen.getByText('issues.milestone_edit'));
    await fireEvent.click(screen.getByText('issues.milestone_save'));

    await waitFor(() =>
      expect(updateMock).toHaveBeenCalledWith('acme', 'web', 7, { milestone_id: null })
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('surfaces a toast when the milestone list fails to load', async () => {
    listMock.mockRejectedValue(new Error('fail'));
    render(IssueMilestonePanel, {
      owner: 'acme',
      repo: 'web',
      issueNumber: 7,
      milestoneId: null,
      onChanged: vi.fn(),
    });

    await waitFor(() => expect(toastError).toHaveBeenCalledWith('fail'));
  });
});
