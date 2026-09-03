import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const listMock = vi.fn();
const updateMock = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  milestones: { list: (...a: unknown[]) => listMock(...a) },
  pulls: { update: (...a: unknown[]) => updateMock(...a) },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

import PrMilestoneBox from './PrMilestoneBox.svelte';
import type { Milestone, PullRequest } from '$lib/types/entities';

const openMilestones: Milestone[] = [
  { id: 5, repo_id: 1, title: 'v1', state: 'open' },
  { id: 6, repo_id: 1, title: 'v2', state: 'open' },
];

function prWith(milestoneId: number | null): PullRequest {
  return {
    id: 1,
    number: 7,
    title: 'feat: x',
    body: null,
    state: 'open',
    is_draft: false,
    author_id: 1,
    head_branch: 'feature',
    base_branch: 'main',
    milestone_id: milestoneId ?? null,
  };
}

describe('PrMilestoneBox.svelte', () => {
  beforeEach(() => {
    listMock.mockReset();
    updateMock.mockReset();
    toastSuccess.mockClear();
    toastError.mockClear();
    listMock.mockResolvedValue(openMilestones);
    updateMock.mockResolvedValue(undefined);
  });

  it('loads the repo milestones and shows the current milestone title', async () => {
    render(PrMilestoneBox, { owner: 'acme', repo: 'web', pr: prWith(5), onChanged: vi.fn() });

    await waitFor(() => expect(screen.getByText('v1')).toBeInTheDocument());
    expect(listMock).toHaveBeenCalledWith('acme', 'web');
  });

  it('shows the no-milestone option when the PR has none', async () => {
    render(PrMilestoneBox, { owner: 'acme', repo: 'web', pr: prWith(null), onChanged: vi.fn() });

    await waitFor(() =>
      expect(screen.getByText('No milestone', { selector: 'option' })).toBeInTheDocument()
    );
  });

  it('attaches a milestone via pulls.update with the id', async () => {
    const onChanged = vi.fn();
    const { container } = render(PrMilestoneBox, {
      owner: 'acme',
      repo: 'web',
      pr: prWith(null),
      onChanged,
    });

    const select = (await waitFor(() => {
      const el = container.querySelector('select') as HTMLSelectElement;
      if (!el) throw new Error('select not rendered');
      return el;
    })) as HTMLSelectElement;
    // happy-dom needs the value set before dispatching change
    select.value = '5';
    await fireEvent.change(select);

    await waitFor(() =>
      expect(updateMock).toHaveBeenCalledWith('acme', 'web', 7, { milestone_id: 5 })
    );
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith('Milestone updated'));
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('clears the milestone by sending null', async () => {
    const onChanged = vi.fn();
    const { container } = render(PrMilestoneBox, {
      owner: 'acme',
      repo: 'web',
      pr: prWith(5),
      onChanged,
    });

    const select = (await waitFor(() => {
      const el = container.querySelector('select') as HTMLSelectElement;
      if (!el) throw new Error('select not rendered');
      return el;
    })) as HTMLSelectElement;
    select.value = '';
    await fireEvent.change(select);

    await waitFor(() =>
      expect(updateMock).toHaveBeenCalledWith('acme', 'web', 7, { milestone_id: null })
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
  });

  it('surfaces an error toast when the milestone list fails to load', async () => {
    listMock.mockRejectedValue(new Error('fail'));
    render(PrMilestoneBox, { owner: 'acme', repo: 'web', pr: prWith(null), onChanged: vi.fn() });

    await waitFor(() => expect(toastError).toHaveBeenCalledWith('fail'));
  });
});
