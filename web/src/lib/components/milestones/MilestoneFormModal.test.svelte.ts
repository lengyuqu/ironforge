import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const milestonesCreate = vi.fn();
const milestonesUpdate = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  milestones: {
    create: (...a: unknown[]) => milestonesCreate(...a),
    update: (...a: unknown[]) => milestonesUpdate(...a),
  },
}));

const toastSuccess = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: vi.fn(),
  },
}));

import MilestoneFormModal from './MilestoneFormModal.svelte';
import type { Milestone } from '$lib/types/entities';

describe('MilestoneFormModal.svelte', () => {
  beforeEach(() => {
    milestonesCreate.mockReset();
    milestonesUpdate.mockReset();
    toastSuccess.mockClear();
  });

  it('creates a milestone converting the local due date to RFC 3339', async () => {
    milestonesCreate.mockResolvedValue(undefined);
    const onClose = vi.fn();
    const onSaved = vi.fn();
    render(MilestoneFormModal, { owner: 'alice', repo: 'demo', milestone: null, onClose, onSaved });

    expect(screen.getByText('settings.new_milestone')).toBeInTheDocument();
    await fireEvent.input(screen.getByLabelText('settings.milestone_title'), {
      target: { value: '  v2.0 ' },
    });
    await fireEvent.input(screen.getByLabelText('settings.milestone_desc'), {
      target: { value: 'next release' },
    });
    await fireEvent.input(screen.getByLabelText('settings.milestone_due'), {
      target: { value: '2026-12-31T23:59' },
    });

    await fireEvent.click(screen.getByText('settings.create_milestone'));
    await waitFor(() =>
      expect(milestonesCreate).toHaveBeenCalledWith('alice', 'demo', {
        title: 'v2.0',
        description: 'next release',
        // Timezone-agnostic expectation: the same local instant in RFC 3339.
        due_date: new Date('2026-12-31T23:59').toISOString(),
        state: 'open',
      })
    );
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(onSaved).toHaveBeenCalledTimes(1);
  });

  it('updates an existing milestone keeping its closed state (prop-level)', async () => {
    milestonesUpdate.mockResolvedValue(undefined);
    const milestone = {
      id: 6,
      title: 'v1.0',
      description: 'first',
      state: 'closed',
      due_date: null,
    } as unknown as Milestone;
    const onClose = vi.fn();
    render(MilestoneFormModal, { owner: 'alice', repo: 'demo', milestone, onClose, onSaved: () => {} });

    expect(screen.getByText('settings.edit_milestone')).toBeInTheDocument();
    expect((screen.getByLabelText('settings.milestone_title') as HTMLInputElement).value).toBe(
      'v1.0'
    );

    await fireEvent.click(screen.getByText('settings.save_milestone'));
    await waitFor(() =>
      expect(milestonesUpdate).toHaveBeenCalledWith('alice', 'demo', 6, {
        title: 'v1.0',
        description: 'first',
        state: 'closed',
        due_date: null,
      })
    );
    expect(toastSuccess).toHaveBeenCalledWith('Milestone saved');
  });

  it('blocks blank titles inline without calling the API', async () => {
    render(MilestoneFormModal, { owner: 'alice', repo: 'demo', milestone: null, onClose: () => {}, onSaved: () => {} });

    await fireEvent.click(screen.getByText('settings.create_milestone'));
    expect(await screen.findByText('Title is required')).toBeInTheDocument();
    expect(milestonesCreate).not.toHaveBeenCalled();
  });

  it('shows an inline error when the API rejects', async () => {
    milestonesCreate.mockRejectedValue(new Error('conflict'));
    render(MilestoneFormModal, { owner: 'alice', repo: 'demo', milestone: null, onClose: () => {}, onSaved: () => {} });

    await fireEvent.input(screen.getByLabelText('settings.milestone_title'), {
      target: { value: 'v9' },
    });
    await fireEvent.click(screen.getByText('settings.create_milestone'));

    expect(await screen.findByText('conflict')).toBeInTheDocument();
  });
});
