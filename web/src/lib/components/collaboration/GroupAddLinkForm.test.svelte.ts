import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { ApiError } from '$lib/api/_base.svelte';

const api = vi.hoisted(() => ({ addLink: vi.fn(), repoGet: vi.fn() }));
vi.mock('$lib/api/collabGroups', async (orig) => ({
  ...(await orig<typeof import('$lib/api/collabGroups')>()),
  collabGroups: { addLink: (...a: unknown[]) => api.addLink(...a) },
}));
vi.mock('$lib/api/repos', () => ({ repos: { get: (...a: unknown[]) => api.repoGet(...a) } }));
vi.mock('$lib/components/toast.svelte', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock('$lib/i18n', () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === 'object' ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

import GroupAddLinkForm from './GroupAddLinkForm.svelte';

function mount(onChanged = vi.fn()) {
  render(GroupAddLinkForm, { props: { owner: 'alice', repo: 'demo', groupId: 'g1', version: 4, onChanged } });
  return onChanged;
}
const type = (label: string, value: string) =>
  fireEvent.input(screen.getByLabelText(new RegExp(label)), { target: { value } });
const submit = () => fireEvent.click(screen.getByRole('button', { name: 'collab_groups.add.submit' }));

describe('GroupAddLinkForm', () => {
  beforeEach(() => {
    api.addLink.mockReset();
    api.repoGet.mockReset();
  });

  it('resolves owner/name to an id and sends the group version it was given', async () => {
    api.repoGet.mockResolvedValue({ id: 42, name: 'api' });
    api.addLink.mockResolvedValue({});
    const onChanged = mount();
    await type('collab_groups.add.target_label', 'bob/api');
    await type('collab_groups.add.task_label', ' task-1 ');
    await submit();
    await waitFor(() => expect(api.addLink).toHaveBeenCalledTimes(1));
    expect(api.repoGet).toHaveBeenCalledWith('bob', 'api');
    expect(api.addLink).toHaveBeenCalledWith('alice', 'demo', 'g1', {
      client_request_id: expect.any(String),
      target_repo_id: 42,
      target_task_id: 'task-1',
      commit_sha: null,
      expected_group_version: 4,
    });
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it('refuses to send without a task or commit, or with a short commit id', async () => {
    mount();
    await type('collab_groups.add.target_label', 'bob/api');
    await submit();
    expect(await screen.findByRole('alert')).toHaveTextContent('collab_groups.add.need_one');
    await type('collab_groups.add.commit_label', 'abc123');
    await submit();
    expect(await screen.findByRole('alert')).toHaveTextContent('collab_groups.add.sha_invalid');
    expect(api.repoGet).not.toHaveBeenCalled();
    expect(api.addLink).not.toHaveBeenCalled();
  });

  it('gives the same message for a missing and an invisible repository, and never writes', async () => {
    mount();
    await type('collab_groups.add.target_label', 'bob/secret');
    await type('collab_groups.add.task_label', 't');
    api.repoGet.mockRejectedValueOnce(new ApiError('not found', 404));
    await submit();
    const first = (await screen.findByText(/collab_groups.add.repo_not_found/)).textContent;
    api.repoGet.mockRejectedValueOnce(new ApiError('forbidden', 403));
    await submit();
    await waitFor(() => expect(api.repoGet).toHaveBeenCalledTimes(2));
    expect(screen.getByText(/collab_groups.add.repo_not_found/).textContent).toBe(first);
    expect(api.addLink).not.toHaveBeenCalled();
  });

  it('uses a new idempotency key after a failed attempt', async () => {
    api.repoGet.mockResolvedValue({ id: 42, name: 'api' });
    api.addLink.mockRejectedValueOnce(new ApiError('write denied', 403));
    api.addLink.mockResolvedValue({});
    mount();
    await type('collab_groups.add.target_label', 'bob/api');
    await type('collab_groups.add.task_label', 't');
    await submit();
    await screen.findByText(/write denied/);
    await submit();
    await waitFor(() => expect(api.addLink).toHaveBeenCalledTimes(2));
    const keys = api.addLink.mock.calls.map((c) => c[3].client_request_id);
    expect(keys[0]).not.toBe(keys[1]);
  });

  it('asks the page to reload on a version conflict', async () => {
    api.repoGet.mockResolvedValue({ id: 42, name: 'api' });
    api.addLink.mockRejectedValue(new ApiError('moved', 409, 'VERSION_CONFLICT'));
    const onChanged = mount();
    await type('collab_groups.add.target_label', 'bob/api');
    await type('collab_groups.add.task_label', 't');
    await submit();
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(screen.getByText('collab_groups.conflict')).toBeInTheDocument();
  });
});
