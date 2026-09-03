import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const milestonesDelete = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  milestones: { delete: (...a: unknown[]) => milestonesDelete(...a) },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

import MilestoneDeleteModal from './MilestoneDeleteModal.svelte';
import type { Milestone } from '$lib/types/entities';

const milestone = {
  id: 4,
  title: 'v1.0',
  description: '',
  state: 'open',
  due_date: null,
} as unknown as Milestone;

describe('MilestoneDeleteModal.svelte', () => {
  beforeEach(() => {
    milestonesDelete.mockReset();
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  it('renders the confirmation dialog naming the milestone', () => {
    render(MilestoneDeleteModal, {
      owner: 'alice',
      repo: 'demo',
      milestone,
      onClose: () => {},
      onDeleted: () => {},
    });

    expect(screen.getByText('Confirm Delete')).toBeInTheDocument();
    expect(screen.getByText('v1.0')).toBeInTheDocument();
    expect(screen.getByText('settings.delete_milestone')).toBeInTheDocument();
  });

  it('deletes, toasts, closes once and refreshes', async () => {
    milestonesDelete.mockResolvedValue(undefined);
    const onClose = vi.fn();
    const onDeleted = vi.fn();
    render(MilestoneDeleteModal, {
      owner: 'alice',
      repo: 'demo',
      milestone,
      onClose,
      onDeleted,
    });

    await fireEvent.click(screen.getByText('settings.delete_milestone'));
    await waitFor(() => expect(milestonesDelete).toHaveBeenCalledWith('alice', 'demo', 4));
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(onDeleted).toHaveBeenCalledTimes(1);
  });

  it('toasts the failure and stays open', async () => {
    milestonesDelete.mockRejectedValue(new Error('has issues'));
    const onClose = vi.fn();
    render(MilestoneDeleteModal, {
      owner: 'alice',
      repo: 'demo',
      milestone,
      onClose,
      onDeleted: () => {},
    });

    await fireEvent.click(screen.getByText('settings.delete_milestone'));
    await waitFor(() => expect(toastError).toHaveBeenCalled());
    expect(onClose).not.toHaveBeenCalled();
  });
});
