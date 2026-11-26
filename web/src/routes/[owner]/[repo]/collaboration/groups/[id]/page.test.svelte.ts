import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, within } from '@testing-library/svelte';
import { ApiError } from '$lib/api/_base.svelte';

vi.mock('$app/state', () => ({ page: { params: { owner: 'alice', repo: 'demo', id: 'g1' } } }));

const api = vi.hoisted(() => ({ get: vi.fn(), listLinks: vi.fn(), removeLink: vi.fn(), addLink: vi.fn(), repoGet: vi.fn() }));
vi.mock('$lib/api/collabGroups', async (orig) => ({
  ...(await orig<typeof import('$lib/api/collabGroups')>()),
  collabGroups: {
    get: (...a: unknown[]) => api.get(...a),
    listLinks: (...a: unknown[]) => api.listLinks(...a),
    removeLink: (...a: unknown[]) => api.removeLink(...a),
    addLink: (...a: unknown[]) => api.addLink(...a),
  },
}));
vi.mock('$lib/api/repos', () => ({ repos: { get: (...a: unknown[]) => api.repoGet(...a) } }));

const confirm = vi.hoisted(() => vi.fn());
vi.mock('$lib/components/confirm.svelte', () => ({ confirmDialog: (...a: unknown[]) => confirm(...a) }));
vi.mock('$lib/components/toast.svelte', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock('$lib/components/RepoHeader.svelte', async () => ({ default: (await import('../../../../../../test-stubs/EmptyStub.svelte')).default }));
vi.mock('$lib/components/collaboration/CollabNav.svelte', async () => ({ default: (await import('../../../../../../test-stubs/EmptyStub.svelte')).default }));
vi.mock('$lib/i18n', () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === 'object' ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

import Page from './+page.svelte';
import type { GroupDetail, GroupLink } from '$lib/api/collabGroups';

function link(over: Partial<GroupLink> = {}): GroupLink {
  return {
    id: 'l1',
    group_id: 'g1',
    target_repo: { id: 2, owner: 'bob', name: 'api' },
    task: { id: 't1', title: 'Wire the endpoint', status: 'in_progress', version: 1, origin: 'user_content' },
    commit: { sha: 'a'.repeat(40), commit_state: 'available' },
    created_by_kind: 'user',
    created_by_id: '7',
    created_at: '2026-11-05T10:00:00Z',
    removed_at: null,
    ...over,
  };
}

function detail(over: Partial<GroupDetail> = {}, links: GroupLink[] = [link()], next: string | null = null): GroupDetail {
  return {
    id: 'g1',
    anchor_repo_id: 1,
    title: 'Release 2',
    description: 'Cross repo work',
    created_by_kind: 'user',
    created_by_id: '7',
    version: 4,
    created_at: '2026-11-05T09:00:00Z',
    origin: 'user_content',
    links: { items: links, next_cursor: next, has_more: next !== null, as_of: '2026-11-06T00:00:00Z' },
    ...over,
  };
}

async function mount(d: GroupDetail) {
  api.get.mockResolvedValue(d);
  render(Page);
  await waitFor(() => expect(api.get).toHaveBeenCalled());
}

describe('coordination group page', () => {
  beforeEach(() => {
    Object.values(api).forEach((f) => f.mockReset());
    confirm.mockReset();
    localStorage.clear();
  });

  it('renders every client-written field as inert text', async () => {
    const hostile = '<img src=x onerror="window.__pwned=1"><script>window.__pwned=1</script>';
    await mount(
      detail(
        { title: hostile, description: hostile },
        [link({ target_repo: { id: 2, owner: 'bob', name: 'api' }, task: { id: 't1', title: hostile, status: hostile, version: 1, origin: 'user_content' } })],
      ),
    );
    expect(await screen.findByTestId('group-title')).toHaveTextContent(hostile);
    expect(screen.getByTestId('group-description')).toHaveTextContent(hostile);
    expect(document.querySelector('img')).toBeNull();
    expect(document.querySelector('script')).toBeNull();
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });

  it('shows no count, total or hint of hidden links, with items or without', async () => {
    await mount(detail({}, []));
    const empty = await screen.findByTestId('links-empty');
    expect(empty).toHaveTextContent('collab_groups.links.empty');
    // Nothing numeric describing links: no badge on the card title, no total.
    expect(document.querySelector('.count')).toBeNull();
    expect(screen.queryByText(/hidden|total|filtered/i)).toBeNull();
  });

  it('keeps the same structure for a group whose links are all invisible and for an empty one', async () => {
    // The server answers both identically (an empty page, has_more false).
    await mount(detail({}, []));
    await screen.findByTestId('links-empty');
    const html = document.body.innerHTML;
    expect(html).not.toMatch(/\bhidden\b|\binvisible\b/i);
    expect(screen.queryByTestId('links-list')).toBeNull();
  });

  it('gives a reader no write controls and no version', async () => {
    await mount(detail({ version: undefined }));
    await screen.findByTestId('group-link');
    expect(screen.getByTestId('read-only-note')).toBeInTheDocument();
    expect(screen.queryByTestId('group-version')).toBeNull();
    expect(screen.queryByRole('button', { name: 'collab_groups.links.remove' })).toBeNull();
    expect(screen.queryByTestId('group-add-link-form')).toBeNull();
  });

  it('continues a page that is short but has a cursor, without duplicating links', async () => {
    await mount(detail({}, [link()], 'CUR1'));
    await screen.findByTestId('group-link');
    api.listLinks.mockResolvedValue({
      items: [link(), link({ id: 'l2', target_repo: { id: 3, owner: 'bob', name: 'web' } })],
      next_cursor: null,
      has_more: false,
      as_of: '2026-11-06T00:01:00Z',
    });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_groups.links.load_more' }));
    await waitFor(() => expect(screen.getAllByTestId('group-link')).toHaveLength(2));
    expect(api.listLinks).toHaveBeenCalledWith('alice', 'demo', 'g1', { limit: 50, cursor: 'CUR1' });
    expect(screen.queryByRole('button', { name: 'collab_groups.links.load_more' })).toBeNull();
  });

  it('removes a link only after confirmation, naming the version read', async () => {
    await mount(detail());
    const row = await screen.findByTestId('group-link');
    confirm.mockResolvedValueOnce(false);
    await fireEvent.click(within(row).getByRole('button', { name: 'collab_groups.links.remove' }));
    await waitFor(() => expect(confirm).toHaveBeenCalledTimes(1));
    expect(api.removeLink).not.toHaveBeenCalled();

    confirm.mockResolvedValueOnce(true);
    api.removeLink.mockResolvedValue(link({ removed_at: '2026-11-06T00:00:00Z' }));
    api.get.mockResolvedValue(detail({ version: 5 }, []));
    await fireEvent.click(within(row).getByRole('button', { name: 'collab_groups.links.remove' }));
    await waitFor(() => expect(api.removeLink).toHaveBeenCalledTimes(1));
    const [, , gid, lid, body] = api.removeLink.mock.calls[0];
    expect([gid, lid]).toEqual(['g1', 'l1']);
    expect(body.expected_group_version).toBe(4);
    expect(typeof body.client_request_id).toBe('string');
    await screen.findByTestId('links-empty');
  });

  it('reloads and says so on a version conflict, and a retry uses a new key', async () => {
    await mount(detail());
    const row = await screen.findByTestId('group-link');
    confirm.mockResolvedValue(true);
    api.removeLink.mockRejectedValueOnce(new ApiError('moved', 409, 'VERSION_CONFLICT'));
    api.get.mockResolvedValue(detail({ version: 6 }));
    await fireEvent.click(within(row).getByRole('button', { name: 'collab_groups.links.remove' }));
    await screen.findByTestId('conflict-notice');
    expect(api.get).toHaveBeenCalledTimes(2);

    api.removeLink.mockRejectedValueOnce(new ApiError('boom', 500));
    const row2 = await screen.findByTestId('group-link');
    await fireEvent.click(within(row2).getByRole('button', { name: 'collab_groups.links.remove' }));
    await waitFor(() => expect(api.removeLink).toHaveBeenCalledTimes(2));
    await screen.findByText(/boom/);
    api.removeLink.mockResolvedValue(link());
    await fireEvent.click(within(screen.getByTestId('group-link')).getByRole('button', { name: 'collab_groups.links.remove' }));
    await waitFor(() => expect(api.removeLink).toHaveBeenCalledTimes(3));
    const keys = api.removeLink.mock.calls.map((c) => c[4].client_request_id);
    expect(new Set(keys).size).toBe(3);
    expect(api.removeLink.mock.calls[1][4].expected_group_version).toBe(6);
  });

  it('says "no such group" on 404 and "no access" on 403', async () => {
    api.get.mockRejectedValue(new ApiError('nope', 404));
    render(Page);
    expect(await screen.findByText('collab_groups.detail.not_found')).toBeInTheDocument();
  });

  it('says "no access" on 403', async () => {
    api.get.mockRejectedValue(new ApiError('nope', 403));
    render(Page);
    expect(await screen.findByText('collab.access_denied')).toBeInTheDocument();
  });

  it('remembers the group it opened', async () => {
    await mount(detail());
    await screen.findByTestId('group-title');
    expect(JSON.parse(localStorage.getItem('ironforge:collab-groups:alice/demo')!)).toEqual([{ id: 'g1', title: 'Release 2' }]);
  });
});
