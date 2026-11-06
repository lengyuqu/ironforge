<script lang="ts">
  // "This task waits for …": the declared dependencies of one task, with the two
  // things a signed-in person can do — add one (from a picker of the
  // repository's tasks) and remove one.
  //
  // A dependency is a declaration. It blocks, starts and finishes nothing, and
  // the status shown beside each dependency is what its task's participants
  // declared, not evidence (`blocked_by` is derived from those declarations).
  // Every change is checked against the repository's graph version; a stale
  // version, a cycle, a full graph and a duplicate are refused by the server
  // and explained here.
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { toast } from '$lib/components/toast.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import Button from '$lib/components/Button.svelte';
  import StatusBadge from './StatusBadge.svelte';
  import { collaboration, type CollabTask, type DependencyList } from '$lib/api/collaboration';
  import { collabRelations, MAX_DIRECT_DEPENDENCIES } from '$lib/api/collabRelations';
  import { KNOWN, newClientId } from '$lib/utils/collaboration';
  import { errorText, mustReload } from '$lib/utils/collabRelations';

  interface Props {
    owner: string;
    repo: string;
    taskId: string;
    deps: DependencyList;
    /** Called after a change, or after a conflict: re-read the dependency list. */
    onChanged?: () => void;
  }

  let { owner, repo, taskId, deps, onChanged }: Props = $props();
  const t = createT();
  const readMsg = (e: unknown) => toErrorMessage(e, t('common.unknown_error'));

  const taskHref = (id: string) => `/${owner}/${repo}/collaboration/tasks/${id}`;

  // ── picker ──
  let picking = $state(false);
  let tasks = $state<CollabTask[]>([]);
  let cursor = $state<string | null>(null);
  let pickerLoading = $state(false);
  let pickerError = $state('');
  let selected = $state('');

  let existing = $derived(new Set(deps.dependencies.map((d) => d.depends_on_task_id)));
  let candidates = $derived(tasks.filter((x) => x.id !== taskId && !existing.has(x.id)));
  let atLimit = $derived(deps.dependencies.length >= MAX_DIRECT_DEPENDENCIES);

  async function loadTasks(append: boolean) {
    pickerLoading = true;
    pickerError = '';
    try {
      const page = await collaboration.listTasks(owner, repo, { limit: 100, cursor: append ? cursor : null });
      tasks = append ? [...tasks, ...page.items] : page.items;
      cursor = page.next_cursor;
    } catch (e) {
      pickerError = readMsg(e);
    } finally {
      pickerLoading = false;
    }
  }

  function openPicker() {
    picking = true;
    selected = '';
    actionError = '';
    addKey = newClientId();
    void loadTasks(false);
  }

  // ── writes ──
  let busy = $state(false);
  let actionError = $state('');
  // One key per attempt; a failed attempt gets a fresh one so a retry is a new request.
  let addKey = $state(newClientId());
  let removeKey = $state(newClientId());
  let removingId = $state<string | null>(null);

  async function add(e: SubmitEvent) {
    e.preventDefault();
    if (!selected || busy) return;
    busy = true;
    actionError = '';
    try {
      const res = await collabRelations.addDependency(owner, repo, taskId, {
        client_request_id: addKey,
        depends_on_task_id: selected,
        expected_graph_version: deps.graph_version,
      });
      toast.success(res.replayed ? t('collab_relations.deps.added_replayed') : t('collab_relations.deps.added'));
      picking = false;
      selected = '';
      addKey = newClientId();
      onChanged?.();
    } catch (err) {
      actionError = errorText('deps', err, t);
      addKey = newClientId();
      if (mustReload(err)) {
        actionError = `${actionError} ${t('collab_relations.deps.reloaded')}`;
        onChanged?.();
      }
    } finally {
      busy = false;
    }
  }

  async function remove(dependsOnId: string) {
    if (busy) return;
    busy = true;
    removingId = dependsOnId;
    actionError = '';
    try {
      const res = await collabRelations.removeDependency(owner, repo, taskId, dependsOnId, {
        client_request_id: removeKey,
        expected_graph_version: deps.graph_version,
      });
      toast.success(res.removed ? t('collab_relations.deps.removed') : t('collab_relations.deps.removed_already'));
      removeKey = newClientId();
      onChanged?.();
    } catch (err) {
      actionError = errorText('deps', err, t);
      removeKey = newClientId();
      if (mustReload(err)) {
        actionError = `${actionError} ${t('collab_relations.deps.reloaded')}`;
        onChanged?.();
      }
    } finally {
      busy = false;
      removingId = null;
    }
  }
</script>

<div class="deps" data-testid="dependencies-editor">
  {#if deps.dependencies.length === 0}
    <p class="muted">{t('collab.none')}</p>
  {:else}
    <ul class="list">
      {#each deps.dependencies as d (d.depends_on_task_id)}
        <li class="row" data-testid="dependency-row">
          <a class="grow text" href={taskHref(d.depends_on_task_id)}>{d.title}</a>
          <StatusBadge value={d.status} prefix="collab.task_status" known={KNOWN.taskStatus} good={['done']} title={t('collab.declared_hint')} />
          {#if deps.blocked_by.includes(d.depends_on_task_id)}
            <span class="warn">{t('collab.deps.declared_blocking')}</span>
          {/if}
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            ariaLabel={t('collab_relations.deps.remove_label', { title: d.title })}
            onclick={() => remove(d.depends_on_task_id)}
          >
            {removingId === d.depends_on_task_id ? t('common.saving') : t('collab_relations.deps.remove')}
          </Button>
        </li>
      {/each}
    </ul>
  {/if}

  <p class="muted">
    {t('collab_relations.deps.graph_version', { version: deps.graph_version })} · {t('collab_relations.deps.graph_hint')}
  </p>
  <p class="muted">{t('collab_relations.deps.write_note')}</p>

  {#if actionError && !picking}<ErrorBanner message={actionError} />{/if}

  {#if atLimit}
    <p class="warn" data-testid="deps-limit">{t('collab_relations.deps.limit', { max: MAX_DIRECT_DEPENDENCIES })}</p>
  {:else if !picking}
    <div><Button size="sm" onclick={openPicker}>{t('collab_relations.deps.add')}</Button></div>
  {:else}
    <form class="form" onsubmit={add}>
      <label class="field">
        <span class="form-label">{t('collab_relations.deps.pick_label')}</span>
        <select value={selected} onchange={(e) => (selected = e.currentTarget.value)} disabled={busy || (pickerLoading && candidates.length === 0)}>
          <option value="">
            {pickerLoading && candidates.length === 0 ? t('collab_relations.deps.pick_loading') : t('collab_relations.deps.pick_placeholder')}
          </option>
          {#each candidates as c (c.id)}
            <option value={c.id}>{t('collab_relations.deps.option', { title: c.title, status: c.status })}</option>
          {/each}
        </select>
      </label>
      <p class="form-hint">{t('collab_relations.deps.pick_hint')}</p>
      {#if pickerError}<ErrorBanner message={pickerError} />{/if}
      {#if !pickerLoading && !pickerError && candidates.length === 0 && !cursor}
        <p class="muted" data-testid="picker-empty">{t('collab_relations.deps.pick_none')}</p>
      {/if}
      {#if cursor}
        <div>
          <Button size="sm" disabled={pickerLoading} onclick={() => loadTasks(true)}>{t('collab_relations.deps.pick_more')}</Button>
        </div>
      {/if}
      {#if actionError}<ErrorBanner message={actionError} />{/if}
      <div class="form-actions">
        <Button type="submit" variant="primary" size="sm" disabled={busy || !selected}>
          {busy ? t('collab_relations.deps.adding') : t('collab_relations.deps.submit')}
        </Button>
        <Button size="sm" disabled={busy} onclick={() => { picking = false; actionError = ''; }}>{t('common.cancel')}</Button>
      </div>
    </form>
  {/if}
</div>

<style>
  .deps { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .list { list-style: none; margin: 0; padding: 0; }
  .list > li { padding: 8px 0; border-top: 1px solid var(--border-light); }
  .list > li:first-child { border-top: none; }
  .row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .grow { flex: 1 1 200px; min-width: 0; }
  .text { overflow-wrap: anywhere; }
  .muted { color: var(--text-muted); font-size: 12px; margin: 2px 0; }
  .warn { color: var(--yellow); font-size: 12px; margin: 2px 0; }
  .form { display: flex; flex-direction: column; gap: 8px; }
  .field { display: flex; flex-direction: column; gap: 4px; }
  select { width: 100%; box-sizing: border-box; }
</style>
