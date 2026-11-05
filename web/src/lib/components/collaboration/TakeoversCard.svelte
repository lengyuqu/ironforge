<script lang="ts">
  // Read-only history of ownership changes of one task: a session claiming it
  // or a person reassigning it, oldest first, each with its reason. The reason
  // is the actor's own claim; `actor_kind` names the door that was used, not
  // proof of who was behind it.
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import Button from '$lib/components/Button.svelte';
  import StatusBadge from './StatusBadge.svelte';
  import { collabRelations, type TakeoverRecord } from '$lib/api/collabRelations';
  import { formatTime, shortId } from '$lib/utils/collaboration';

  interface Props {
    owner: string;
    repo: string;
    taskId: string;
    /** Changing this (e.g. the task's version) re-reads the history. */
    refreshKey?: number | string;
    /** Display label for a session id (falls back to a short id). */
    sessionLabel?: (id: string | null) => string;
  }

  let { owner, repo, taskId, refreshKey = 0, sessionLabel }: Props = $props();
  const t = createT();
  const readMsg = (e: unknown) => toErrorMessage(e, t('common.unknown_error'));

  let items = $state<TakeoverRecord[]>([]);
  let cursor = $state<string | null>(null);
  let loading = $state(true);
  let error = $state('');

  const label = (id: string | null) => (sessionLabel ? sessionLabel(id) : id ? shortId(id) : t('collab.unowned'));

  async function load(append: boolean) {
    loading = true;
    error = '';
    try {
      const res = await collabRelations.listTakeovers(owner, repo, taskId, {
        limit: 25,
        cursor: append ? cursor : null,
      });
      items = append ? [...items, ...res.items] : res.items;
      cursor = res.next_cursor;
    } catch (e) {
      error = readMsg(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void owner;
    void repo;
    void taskId;
    void refreshKey;
    void load(false);
  });
</script>

<div class="takeovers" data-testid="takeovers-card">
  {#if error}<ErrorBanner message={error} />{/if}
  {#if !loading && !error && items.length === 0}
    <p class="muted" data-testid="takeovers-none">{t('collab_relations.takeovers.none')}</p>
  {/if}
  <ol class="timeline">
    {#each items as r (r.id)}
      <li data-testid="takeover-row">
        <div class="row">
          <StatusBadge
            value={r.outcome}
            prefix="collab_relations.takeovers.outcome"
            known={['claimed', 'reassigned']}
          />
          <span class="grow text">
            {t('collab_relations.takeovers.move', {
              from: r.from_session_id ? label(r.from_session_id) : t('collab_relations.takeovers.no_previous'),
              to: label(r.to_session_id),
            })}
          </span>
          <span class="when">{formatTime(r.created_at)}</span>
        </div>
        <p class="muted">
          {t('collab_relations.takeovers.actor', { kind: r.actor_kind, id: r.actor_id })} ·
          {t('collab_relations.takeovers.versions', { from: r.previous_task_version, to: r.new_task_version })}
        </p>
        <p class="sec">{t('collab_relations.takeovers.reason')}</p>
        <div class="body">{r.reason}</div>
      </li>
    {/each}
  </ol>
  {#if cursor}
    <div><Button size="sm" disabled={loading} onclick={() => load(true)}>{t('collab.load_more')}</Button></div>
  {/if}
</div>

<style>
  .takeovers { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .timeline { list-style: none; margin: 0; padding: 0; }
  .timeline > li { padding: 8px 0 8px 12px; border-left: 2px solid var(--border); margin-left: 4px; }
  .row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .grow { flex: 1 1 200px; min-width: 0; }
  .text { overflow-wrap: anywhere; }
  .muted { color: var(--text-muted); font-size: 12px; margin: 2px 0; }
  .sec { margin: 6px 0 0; font-size: 12px; color: var(--text-secondary); }
  .when { color: var(--text-muted); font-size: 12px; white-space: nowrap; }
  .body { white-space: pre-wrap; overflow-wrap: anywhere; margin: 2px 0 6px; font-size: 14px; }
</style>
