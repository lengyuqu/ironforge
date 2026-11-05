<script lang="ts">
  // The completion conditions someone defined for a task, judged now, with the
  // two things only a person can do from here: confirm a `manual_confirmation`
  // condition and store the current evaluation as a snapshot.
  //
  // Display only. Nothing here blocks, approves or merges anything: a
  // confirmation is a collaboration record by the signed-in account (any client
  // using that account could write it), tied to the branch head it was given at.
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { toast } from '$lib/components/toast.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import Button from '$lib/components/Button.svelte';
  import StatusBadge from './StatusBadge.svelte';
  import {
    collaboration,
    type DefinedCondition,
    type DefinedStatus,
    type SnapshotSummary,
    type SnapshotView,
  } from '$lib/api/collaboration';
  import { KNOWN, formatTime, knownLabelKey, newClientId, shortId } from '$lib/utils/collaboration';

  interface Props {
    owner: string;
    repo: string;
    taskId: string;
    /** The `defined` part of the completion status; null when nothing is defined. */
    defined: DefinedStatus | null;
    /** Called after something changed that the completion status depends on. */
    onChanged?: () => void;
  }

  let { owner, repo, taskId, defined, onChanged }: Props = $props();
  const t = createT();
  const msg = (e: unknown) => toErrorMessage(e, t('common.unknown_error'));

  // ── manual confirmation ──
  let confirmingId = $state<string | null>(null);
  let note = $state('');
  let confirmKey = $state(newClientId());
  let busy = $state(false);
  let actionError = $state('');

  function startConfirm(c: DefinedCondition) {
    confirmingId = c.id;
    note = '';
    confirmKey = newClientId();
    actionError = '';
  }

  async function submitConfirm(e: SubmitEvent) {
    e.preventDefault();
    if (!confirmingId || busy) return;
    busy = true;
    actionError = '';
    try {
      await collaboration.confirmCondition(owner, repo, taskId, confirmingId, {
        client_request_id: confirmKey,
        note: note.trim() === '' ? undefined : note.trim(),
      });
      toast.success(t('collab.conditions.confirmed_toast'));
      confirmingId = null;
      onChanged?.();
      void loadSnapshots();
    } catch (err) {
      actionError = msg(err);
      // A new attempt after a failure is a new request, not a replay.
      confirmKey = newClientId();
    } finally {
      busy = false;
    }
  }

  // ── snapshots ──
  let snapshots = $state<SnapshotSummary[]>([]);
  let snapshotsLoaded = $state(false);
  let snapshotsError = $state('');
  let shown = $state<SnapshotView | null>(null);
  let shownLoading = $state(false);
  let recordKey = $state(newClientId());

  async function loadSnapshots() {
    snapshotsError = '';
    try {
      snapshots = (await collaboration.listSnapshots(owner, repo, taskId, { limit: 20 })).items;
    } catch (err) {
      snapshotsError = msg(err);
    } finally {
      snapshotsLoaded = true;
    }
  }

  async function record() {
    if (busy) return;
    busy = true;
    actionError = '';
    try {
      const snap = await collaboration.createSnapshot(owner, repo, taskId, recordKey);
      toast.success(snap.replayed ? t('collab.conditions.snapshot_replayed') : t('collab.conditions.snapshot_recorded'));
      recordKey = newClientId();
      await loadSnapshots();
    } catch (err) {
      actionError = msg(err);
    } finally {
      busy = false;
    }
  }

  async function view(id: string) {
    if (shown?.id === id) {
      shown = null;
      return;
    }
    shownLoading = true;
    snapshotsError = '';
    try {
      shown = await collaboration.getSnapshot(owner, repo, taskId, id);
    } catch (err) {
      snapshotsError = msg(err);
    } finally {
      shownLoading = false;
    }
  }

  $effect(() => {
    void owner;
    void repo;
    void taskId;
    shown = null;
    confirmingId = null;
    snapshotsLoaded = false;
    void loadSnapshots();
  });

  function typeLabel(type: string): string {
    const key = knownLabelKey('collab.condition_type', type, KNOWN.conditionType);
    return key ? t(key) : type;
  }

  function paramsText(c: { type: string; params: Record<string, unknown> }): string {
    const entries = Object.entries(c.params ?? {});
    return entries.map(([k, v]) => `${k}: ${typeof v === 'string' ? (k.endsWith('sha') ? shortId(v) : v) : JSON.stringify(v)}`).join(' · ');
  }
</script>

<div class="cond" data-testid="conditions-card">
  {#if !defined}
    <p class="muted" data-testid="conditions-none">{t('collab.conditions.none')}</p>
  {:else}
    <p class="row">
      <StatusBadge
        value={defined.overall}
        prefix="collab.overall"
        known={KNOWN.overall}
        good={['all_satisfied']}
        warn={['incomplete']}
        bad={['unsatisfied']}
      />
      <span class="muted">{t('collab.conditions.version', { version: defined.condition_set_version })}</span>
    </p>
    <ul class="list">
      {#each defined.conditions as c (c.id)}
        <li data-testid="defined-condition">
          <div class="row">
            <strong class="text grow">{c.label}</strong>
            <span class="muted">{typeLabel(c.type)}</span>
            <StatusBadge
              value={c.status}
              prefix="collab.condition_status"
              known={KNOWN.conditionStatus}
              good={['satisfied']}
              warn={['stale', 'unknown']}
              bad={['unsatisfied']}
            />
          </div>
          {#if paramsText(c)}<p class="muted mono text">{paramsText(c)}</p>{/if}
          <p class="muted text">{c.detail}</p>
          {#each c.evidence as ev}
            <p class="muted mono">
              {t('collab.completion.pipeline', { id: ev.pipeline_id })} · {ev.status ?? '—'}
              {#if ev.commit_sha}· {shortId(ev.commit_sha)}{/if}
            </p>
          {/each}
          {#if c.confirmation}
            <p class="muted" data-testid="confirmation-info">
              {t('collab.conditions.confirmed_by', {
                user: c.confirmation.confirmed_by_user_id,
                time: formatTime(c.confirmation.created_at),
              })}
              {#if c.confirmation.head_sha}· {t('collab.conditions.at_head')} <code>{shortId(c.confirmation.head_sha)}</code>{/if}
            </p>
            {#if c.confirmation.note}<div class="note text">{c.confirmation.note}</div>{/if}
          {/if}

          {#if c.type === 'manual_confirmation'}
            {#if confirmingId === c.id}
              <form class="form" onsubmit={submitConfirm}>
                <label class="field">
                  <span class="form-label">{t('collab.conditions.note_label')}</span>
                  <textarea rows="2" bind:value={note} maxlength="1000"></textarea>
                </label>
                <p class="form-hint">{t('collab.conditions.confirm_hint')}</p>
                <div class="form-actions">
                  <Button type="submit" variant="primary" size="sm" disabled={busy}>
                    {busy ? t('common.saving') : t('collab.conditions.confirm')}
                  </Button>
                  <Button size="sm" disabled={busy} onclick={() => (confirmingId = null)}>{t('common.cancel')}</Button>
                </div>
              </form>
            {:else if c.status !== 'satisfied'}
              <!-- A current confirmation needs nothing more; a stale or missing one can be (re)given. -->
              <Button size="sm" onclick={() => startConfirm(c)}>
                {c.confirmation ? t('collab.conditions.confirm_again') : t('collab.conditions.confirm')}
              </Button>
            {/if}
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#if actionError}<ErrorBanner message={actionError} />{/if}

  <div class="snaps">
    <div class="row">
      <h3 class="grow">{t('collab.conditions.snapshots')}</h3>
      <Button size="sm" disabled={busy} onclick={record}>{t('collab.conditions.record')}</Button>
    </div>
    <p class="muted">{t('collab.conditions.snapshots_note')}</p>
    {#if snapshotsError}<ErrorBanner message={snapshotsError} />{/if}
    {#if snapshotsLoaded && snapshots.length === 0}
      <p class="muted">{t('collab.none')}</p>
    {/if}
    <ul class="list">
      {#each snapshots as s (s.id)}
        <li data-testid="snapshot-row">
          <div class="row">
            <StatusBadge
              value={s.overall}
              prefix="collab.overall"
              known={KNOWN.overall}
              good={['all_satisfied']}
              warn={['incomplete']}
              bad={['unsatisfied']}
            />
            <span class="grow muted">
              {formatTime(s.created_at)} · {t('collab.conditions.version', { version: s.condition_set_version })} ·
              {t('collab.conditions.task_version', { version: s.task_version })}
            </span>
            <Button size="sm" variant="ghost" disabled={shownLoading} onclick={() => view(s.id)}>
              {shown?.id === s.id ? t('collab.hide') : t('collab.show')}
            </Button>
          </div>
          {#if shown?.id === s.id}
            <div class="stored" data-testid="snapshot-stored">
              <p class="muted">{t('collab.conditions.stored_as_recorded')}</p>
              {#if shown.result.defined}
                <ul class="list">
                  {#each shown.result.defined.conditions as sc (sc.id)}
                    <li class="row">
                      <span class="text grow">{sc.label}</span>
                      <StatusBadge
                        value={sc.status}
                        prefix="collab.condition_status"
                        known={KNOWN.conditionStatus}
                        good={['satisfied']}
                        warn={['stale', 'unknown']}
                        bad={['unsatisfied']}
                      />
                    </li>
                  {/each}
                </ul>
              {:else}
                <p class="muted">{t('collab.conditions.none')}</p>
              {/if}
              {#if shown.result.declared_done_without_evidence}
                <p class="warn">{t('collab.completion.done_without_evidence')}</p>
              {/if}
              {#if shown.result.observed_branch_head}
                <p class="muted">{t('collab.completion.head')}: <code>{shortId(shown.result.observed_branch_head)}</code></p>
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  </div>
</div>

<style>
  .cond { display: flex; flex-direction: column; gap: 8px; min-width: 0; }
  .row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .grow { flex: 1; min-width: 0; }
  .list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 10px; }
  .list > li { border-top: 1px solid var(--border); padding-top: 8px; }
  .list > li:first-child { border-top: none; padding-top: 0; }
  .muted { color: var(--text-muted); font-size: 12px; margin: 2px 0; }
  .text { overflow-wrap: anywhere; }
  .mono { font-family: var(--font-mono); }
  .warn { color: var(--yellow); font-size: 12px; margin: 4px 0; }
  .note { white-space: pre-wrap; border-left: 3px solid var(--border); padding-left: 10px; margin: 6px 0; font-size: 13px; }
  .form { margin-top: 8px; display: flex; flex-direction: column; gap: 8px; }
  .field { display: flex; flex-direction: column; gap: 4px; }
  textarea { width: 100%; box-sizing: border-box; }
  .snaps { margin-top: 8px; border-top: 1px solid var(--border); padding-top: 10px; }
  h3 { margin: 0; font-size: 14px; }
  .stored { margin: 6px 0 0 12px; padding: 8px 12px; background: var(--bg-primary); border-radius: var(--radius); }
  code { font-family: var(--font-mono); }
</style>
