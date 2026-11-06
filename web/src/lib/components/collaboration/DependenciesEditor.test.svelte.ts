import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { ApiError } from '$lib/api/_base.svelte';

const listTasks = vi.fn();
const addDependency = vi.fn();
const removeDependency = vi.fn();
const toastSuccess = vi.fn();

vi.mock('$lib/api/collaboration', () => ({
  collaboration: { listTasks: (...a: unknown[]) => listTasks(...a) },
}));
vi.mock('$lib/api/collabRelations', async (importOriginal) => {
  const orig = await importOriginal<typeof import('$lib/api/collabRelations')>();
  return {
    ...orig,
    collabRelations: {
      addDependency: (...a: unknown[]) => addDependency(...a),
      removeDependency: (...a: unknown[]) => removeDependency(...a),
    },
  };
});
vi.mock('$lib/components/toast.svelte', () => ({ toast: { success: (...a: unknown[]) => toastSuccess(...a), error: vi.fn() } }));
vi.mock('$lib/i18n', () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === 'object' ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

import DependenciesEditor from './DependenciesEditor.svelte';
import type { CollabTask, DependencyItem, DependencyList } from '$lib/api/collaboration';

function task(id: string, title: string, status = 'open'): CollabTask {
  return {
    id, title, description: null, status, owner_session_id: null, branch: null, paths: [], paths_declared_at: null,
    issue_number: null, pr_number: null, version: 1, last_message_seq: 0, created_at: '', updated_at: '', origin: 'user_content',
  };
}

function dep(id: string, title: string, status = 'open'): DependencyItem {
  return {
    task_id: 'me', depends_on_task_id: id, created_by_kind: 'user', graph_version: 1, created_at: '2026-11-05T10:00:00Z',
    title, status, task_version: 1, task_updated_at: '2026-11-05T10:00:00Z',
  };
}

function list(deps: DependencyItem[], blocked: string[] = [], graph = 3): DependencyList {
  return { task_id: 'me', graph_version: graph, dependencies: deps, blocked_by: blocked, status_source: 'declared', origin: 'user_content' };
}

function mount(deps: DependencyList, onChanged = vi.fn()) {
  render(DependenciesEditor, { props: { owner: 'alice', repo: 'demo', taskId: 'me', deps, onChanged } });
  return onChanged;
}

describe('DependenciesEditor.svelte', () => {
  beforeEach(() => {
    listTasks.mockReset().mockResolvedValue({ items: [], next_cursor: null });
    addDependency.mockReset();
    removeDependency.mockReset();
    toastSuccess.mockReset();
    delete (window as unknown as { __pwned?: number }).__pwned;
  });

  it('lists dependencies with their declared status and the declared-blocking note', () => {
    mount(list([dep('d1', 'Build the thing', 'in_progress'), dep('d2', 'Done thing', 'done')], ['d1']));
    expect(screen.getAllByTestId('dependency-row')).toHaveLength(2);
    expect(screen.getAllByText('collab.deps.declared_blocking')).toHaveLength(1);
    expect(screen.getByText('collab_relations.deps.write_note')).toBeInTheDocument();
  });

  it('renders client-written titles as inert text in the list and in the picker', async () => {
    const hostile = '<img src=x onerror=window.__pwned=1><script>window.__pwned=1</script>';
    listTasks.mockResolvedValue({ items: [task('t9', hostile)], next_cursor: null });
    mount(list([dep('d1', hostile)]));
    expect(screen.getAllByText(hostile)[0]).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.deps.add' }));
    await waitFor(() => expect(screen.getAllByRole('option').length).toBe(2));
    expect(document.querySelector('[data-testid="dependencies-editor"] img')).toBeNull();
    expect(document.querySelector('[data-testid="dependencies-editor"] script')).toBeNull();
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });

  it('leaves out the task itself and tasks it already waits for, and shows declared status', async () => {
    listTasks.mockResolvedValue({
      items: [task('me', 'Myself'), task('d1', 'Already waiting'), task('t2', 'Candidate', 'blocked')],
      next_cursor: null,
    });
    mount(list([dep('d1', 'Already waiting')]));
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.deps.add' }));
    await waitFor(() => expect(screen.getAllByRole('option').length).toBe(2));
    const options = screen.getAllByRole('option').map((o) => o.textContent ?? '');
    expect(options.some((o) => o.includes('Candidate') && o.includes('blocked'))).toBe(true);
    expect(options.some((o) => o.includes('Myself'))).toBe(false);
    expect(options.some((o) => o.includes('Already waiting'))).toBe(false);
  });

  it('adds with the graph version it read and a fresh key, then asks for a reload', async () => {
    listTasks.mockResolvedValue({ items: [task('t2', 'Candidate')], next_cursor: null });
    addDependency.mockResolvedValue({ dependency: {}, graph_version: 4, replayed: false });
    const onChanged = mount(list([], [], 7));
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.deps.add' }));
    await waitFor(() => expect(screen.getAllByRole('option').length).toBe(2));
    await fireEvent.change(screen.getByRole('combobox'), { target: { value: 't2' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.deps.submit' }));
    await waitFor(() => expect(addDependency).toHaveBeenCalledTimes(1));
    const [owner, repo, taskId, input] = addDependency.mock.calls[0];
    expect([owner, repo, taskId]).toEqual(['alice', 'demo', 'me']);
    expect(input.depends_on_task_id).toBe('t2');
    expect(input.expected_graph_version).toBe(7);
    expect(input.client_request_id).toMatch(/\S+/);
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(toastSuccess).toHaveBeenCalledWith('collab_relations.deps.added');
  });

  it('explains a refused cycle, keeps the selection, and retries with a NEW key', async () => {
    listTasks.mockResolvedValue({ items: [task('t2', 'Candidate')], next_cursor: null });
    addDependency
      .mockRejectedValueOnce(new ApiError('the dependency would make tasks wait for each other', 409, 'DEPENDENCY_CYCLE'))
      .mockResolvedValueOnce({ dependency: {}, graph_version: 4, replayed: false });
    const onChanged = mount(list([]));
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.deps.add' }));
    await waitFor(() => expect(screen.getAllByRole('option').length).toBe(2));
    await fireEvent.change(screen.getByRole('combobox'), { target: { value: 't2' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.deps.submit' }));
    expect(await screen.findByText(/collab_relations\.err_deps\.DEPENDENCY_CYCLE/)).toBeInTheDocument();
    expect(onChanged).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.deps.submit' }));
    await waitFor(() => expect(addDependency).toHaveBeenCalledTimes(2));
    expect(addDependency.mock.calls[1][3].client_request_id).not.toBe(addDependency.mock.calls[0][3].client_request_id);
  });

  it('reloads on VERSION_CONFLICT', async () => {
    listTasks.mockResolvedValue({ items: [task('t2', 'Candidate')], next_cursor: null });
    addDependency.mockRejectedValue(new ApiError('stale', 409, 'VERSION_CONFLICT'));
    const onChanged = mount(list([]));
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.deps.add' }));
    await waitFor(() => expect(screen.getAllByRole('option').length).toBe(2));
    await fireEvent.change(screen.getByRole('combobox'), { target: { value: 't2' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.deps.submit' }));
    expect(await screen.findByText(/collab_relations\.err_deps\.VERSION_CONFLICT/)).toBeInTheDocument();
    expect(onChanged).toHaveBeenCalled();
  });

  it('explains a permission refusal (403) in plain words', async () => {
    listTasks.mockResolvedValue({ items: [task('t2', 'Candidate')], next_cursor: null });
    addDependency.mockRejectedValue(new ApiError('write access denied', 403));
    mount(list([]));
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.deps.add' }));
    await waitFor(() => expect(screen.getAllByRole('option').length).toBe(2));
    await fireEvent.change(screen.getByRole('combobox'), { target: { value: 't2' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.deps.submit' }));
    expect(await screen.findByText(/collab_relations\.err_forbidden.*write access denied/)).toBeInTheDocument();
  });

  it('removes with the graph version, reports an already-gone edge, and uses a new key after a failure', async () => {
    removeDependency
      .mockRejectedValueOnce(new ApiError('boom', 500))
      .mockResolvedValueOnce({ removed: false, graph_version: 9 });
    const onChanged = mount(list([dep('d1', 'Build the thing')], ['d1'], 5));
    const btn = screen.getByRole('button', { name: /collab_relations\.deps\.remove_label/ });
    await fireEvent.click(btn);
    expect(await screen.findByText('boom')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: /collab_relations\.deps\.remove_label/ }));
    await waitFor(() => expect(removeDependency).toHaveBeenCalledTimes(2));
    const [, , taskId, dependsOn, input] = removeDependency.mock.calls[1];
    expect(taskId).toBe('me');
    expect(dependsOn).toBe('d1');
    expect(input.expected_graph_version).toBe(5);
    expect(input.client_request_id).not.toBe(removeDependency.mock.calls[0][4].client_request_id);
    expect(toastSuccess).toHaveBeenCalledWith('collab_relations.deps.removed_already');
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it('offers no add button at the 100-dependency limit', () => {
    const many = Array.from({ length: 100 }, (_, i) => dep(`d${i}`, `T${i}`));
    mount(list(many));
    expect(screen.getByTestId('deps-limit')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'collab_relations.deps.add' })).toBeNull();
  });

  it('says so when no other task can be added, and pages the picker', async () => {
    listTasks
      .mockResolvedValueOnce({ items: [task('me', 'Myself')], next_cursor: 'c1' })
      .mockResolvedValueOnce({ items: [task('t3', 'Second page')], next_cursor: null });
    mount(list([]));
    await fireEvent.click(screen.getByRole('button', { name: 'collab_relations.deps.add' }));
    await fireEvent.click(await screen.findByRole('button', { name: 'collab_relations.deps.pick_more' }));
    await waitFor(() => expect(screen.getAllByRole('option').some((o) => o.textContent?.includes('Second page'))).toBe(true));
    expect(listTasks.mock.calls[1][2].cursor).toBe('c1');
  });
});
