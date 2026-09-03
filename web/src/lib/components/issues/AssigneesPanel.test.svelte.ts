import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const listAssigneesMock = vi.fn();
const setAssigneesMock = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  issues: {
    listAssignees: (...a: unknown[]) => listAssigneesMock(...a),
    setAssignees: (...a: unknown[]) => setAssigneesMock(...a),
  },
}));

const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: vi.fn(), error: (...a: unknown[]) => toastError(...a) },
}));

import AssigneesPanel from './AssigneesPanel.svelte';

describe('AssigneesPanel.svelte', () => {
  beforeEach(() => {
    listAssigneesMock.mockReset();
    setAssigneesMock.mockReset();
    toastError.mockClear();
    listAssigneesMock.mockResolvedValue({ assignees: [] });
    setAssigneesMock.mockResolvedValue({ assignees: [] });
  });

  it('loads assignees and renders badges, marking the first as primary', async () => {
    listAssigneesMock.mockResolvedValue({ assignees: ['alice', 'bob'] });
    render(AssigneesPanel, { owner: 'acme', repo: 'web', issueNumber: 7 });

    await waitFor(() => expect(screen.getByText('alice')).toBeInTheDocument());
    expect(screen.getByText('bob')).toBeInTheDocument();
    expect(listAssigneesMock).toHaveBeenCalledWith('acme', 'web', 7);

    const badges = document.querySelectorAll('.assignee-badge');
    expect(badges).toHaveLength(2);
    expect(badges[0].classList.contains('primary')).toBe(true);
    expect(badges[1].classList.contains('primary')).toBe(false);
  });

  it('shows the empty message when there are no assignees', async () => {
    listAssigneesMock.mockResolvedValue({ assignees: [] });
    render(AssigneesPanel, { owner: 'acme', repo: 'web', issueNumber: 7 });
    await waitFor(() => expect(screen.getByText('issues.assignees.empty')).toBeInTheDocument());
  });

  it('saves trimmed assignee names via the editor', async () => {
    listAssigneesMock.mockResolvedValue({ assignees: ['alice'] });
    render(AssigneesPanel, { owner: 'acme', repo: 'web', issueNumber: 7 });
    await waitFor(() => expect(screen.getByText('alice')).toBeInTheDocument());

    await fireEvent.click(screen.getByText('issues.assignees.edit'));
    const input = screen.getByPlaceholderText('issues.assignees.placeholder') as HTMLInputElement;
    expect(input.value).toBe('alice');
    await fireEvent.input(input, { target: { value: 'alice, bob, ' } });
    setAssigneesMock.mockImplementation(async (_o: unknown, _r: unknown, _n: unknown, names: string[]) => ({
      assignees: names,
    }));
    await fireEvent.click(screen.getByText('issues.assignees.save'));

    await waitFor(() =>
      expect(setAssigneesMock).toHaveBeenCalledWith('acme', 'web', 7, ['alice', 'bob'])
    );
    await waitFor(() => expect(screen.getByText('bob')).toBeInTheDocument());
    expect(screen.queryByPlaceholderText('issues.assignees.placeholder')).toBeNull();
  });

  it('surfaces a toast and keeps editing when the save fails', async () => {
    listAssigneesMock.mockResolvedValue({ assignees: ['alice'] });
    setAssigneesMock.mockRejectedValue(new Error('nope'));
    render(AssigneesPanel, { owner: 'acme', repo: 'web', issueNumber: 7 });
    await waitFor(() => expect(screen.getByText('alice')).toBeInTheDocument());

    await fireEvent.click(screen.getByText('issues.assignees.edit'));
    await fireEvent.click(screen.getByText('issues.assignees.save'));

    await waitFor(() => expect(toastError).toHaveBeenCalledWith('nope'));
    expect(screen.getByPlaceholderText('issues.assignees.placeholder')).not.toBeNull();
  });

  it('cancels editing without persisting', async () => {
    listAssigneesMock.mockResolvedValue({ assignees: ['alice'] });
    render(AssigneesPanel, { owner: 'acme', repo: 'web', issueNumber: 7 });
    await waitFor(() => expect(screen.getByText('alice')).toBeInTheDocument());

    await fireEvent.click(screen.getByText('issues.assignees.edit'));
    await fireEvent.click(screen.getByText('issues.assignees.cancel'));

    await waitFor(() =>
      expect(screen.queryByPlaceholderText('issues.assignees.placeholder')).toBeNull()
    );
    expect(setAssigneesMock).not.toHaveBeenCalled();
  });
});
