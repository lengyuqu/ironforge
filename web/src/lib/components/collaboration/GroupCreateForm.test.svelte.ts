import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { ApiError } from '$lib/api/_base.svelte';

const create = vi.hoisted(() => vi.fn());
vi.mock('$lib/api/collabGroups', async (orig) => ({
  ...(await orig<typeof import('$lib/api/collabGroups')>()),
  collabGroups: { create: (...a: unknown[]) => create(...a) },
}));
vi.mock('$lib/i18n', () => {
  const t = (key: string) => key;
  return { createT: () => t, t };
});

import GroupCreateForm from './GroupCreateForm.svelte';

const fill = (label: string, value: string) => fireEvent.input(screen.getByLabelText(label), { target: { value } });
const submit = () => fireEvent.click(screen.getByRole('button', { name: 'collab_groups.create.submit' }));

describe('GroupCreateForm', () => {
  beforeEach(() => create.mockReset());

  it('warns up front who can read title and description', () => {
    render(GroupCreateForm, { props: { owner: 'a', repo: 'r', onCreated: vi.fn() } });
    expect(screen.getByTestId('sharing-warning')).toHaveTextContent('collab_groups.sharing_warning');
  });

  it('needs a title and does not call the server without one', async () => {
    render(GroupCreateForm, { props: { owner: 'a', repo: 'r', onCreated: vi.fn() } });
    await submit();
    expect(await screen.findByRole('alert')).toHaveTextContent('collab_groups.create.title_required');
    expect(create).not.toHaveBeenCalled();
  });

  it('creates, then hands the group on; a retry after a failure uses a new key', async () => {
    const onCreated = vi.fn();
    create.mockRejectedValueOnce(new ApiError('write access required', 403));
    create.mockResolvedValue({ id: 'g9', title: 'T' });
    render(GroupCreateForm, { props: { owner: 'a', repo: 'r', onCreated } });
    await fill('collab_groups.create.title_label', ' T ');
    await submit();
    await screen.findByText(/write access required/);
    expect(onCreated).not.toHaveBeenCalled();
    await submit();
    await waitFor(() => expect(onCreated).toHaveBeenCalledWith({ id: 'g9', title: 'T' }));
    const [first, second] = create.mock.calls;
    expect(first[2].title).toBe('T');
    expect(first[2].client_request_id).not.toBe(second[2].client_request_id);
  });
});
