<script lang="ts">
  // A person reassigns a task: the declared owner moves to another live
  // session, with a required reason. Same server door as a session claim, but
  // as the signed-in account (no session credentials).
  //
  // What it does: moves the declared owner, writes an immutable history row
  // (see TakeoversCard) and tells the session that lost the task. What it does
  // NOT do: touch pending handoffs, grant any Git/CI/approval right, or stop or
  // start work. Listing sessions needs write access, like the reassign itself.
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { toast } from '$lib/components/toast.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import Button from '$lib/components/Button.svelte';
  import { collaboration, type CollabSession } from '$lib/api/collaboration';
  import { collabRelations, MAX_REASON_BYTES } from '$lib/api/collabRelations';
  import { newClientId, shortId } from '$lib/utils/collaboration';
  import { byteLength, errorText, isDenied, mustReload } from '$lib/utils/collabRelations';

  interface Props {
    owner: string;
    repo: string;
    /** The task as last read: its `version` and owner are sent as the expected state. */
    task: { id: string; version: number; owner_session_id: string | null };
    /** Called after a reassignment, or after a conflict: re-read the task. */
    onChanged?: () => void;
  }

  let { owner, repo, task, onChanged }: Props = $props();
  const t = createT();
  const readMsg = (e: unknown) => toErrorMessage(e, t('common.unknown_error'));

  let open = $state(false);
  let sessions = $state<CollabSession[]>([]);
  let sessionsLoading = $state(false);
  let sessionsDenied = $state(false);
  let sessionsError = $state('');
  let target = $state('');
  let reason = $state('');
  let busy = $state(false);
  let actionError = $state('');
  let key = $state(newClientId());

  // Live = not ended and not expired; the server refuses anything else.
  let targets = $derived(
    sessions.filter((s) => (s.state === 'online' || s.state === 'offline') && s.id !== task.owner_session_id),
  );
  let currentOwner = $derived.by(() => {
    const id = task.owner_session_id;
    if (!id) return t('collab.unowned');
    const s = sessions.find((x) => x.id === id);
    return s ? `${s.display_name} (${s.client_kind})` : shortId(id);
  });
  let reasonBytes = $derived(byteLength(reason));
  let reasonOk = $derived(reason.trim().length > 0 && reasonBytes <= MAX_REASON_BYTES);

  async function loadSessions() {
    sessionsLoading = true;
    sessionsDenied = false;
    sessionsError = '';
    const all: CollabSession[] = [];
    try {
      let cursor: string | null = null;
      for (let i = 0; i < 5; i++) {
        const page = await collaboration.listSessions(owner, repo, { limit: 100, cursor });
        all.push(...page.items);
        if (!page.next_cursor) break;
        cursor = page.next_cursor;
      }
      sessions = all;
    } catch (e) {
      if (isDenied(e)) sessionsDenied = true;
      else sessionsError = readMsg(e);
    } finally {
      sessionsLoading = false;
    }
  }

  function start() {
    open = true;
    target = '';
    reason = '';
    actionError = '';
    key = newClientId();
    void loadSessions();
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (busy || !target || !reasonOk) return;
    busy = true;
    actionError = '';
    try {
      const res = await collabRelations.reassignTask(owner, repo, task.id, {
        client_takeover_id: key,
        expected_task_version: task.version,
        expected_owner_session_id: task.owner_session_id,
        to_session_id: target,
        reason,
      });
      toast.success(res.replayed ? t('collab_relations.reassign.replayed') : t('collab_relations.reassign.done'));
      open = false;
      key = newClientId();
      onChanged?.();
    } catch (err) {
      actionError = errorText('reassign', err, t);
      // A new attempt after a failure is a new request, not a replay.
      key = newClientId();
      if (mustReload(err)) {
        actionError = `${actionError} ${t('collab_relations.reassign.reloaded')}`;
        target = '';
        onChanged?.();
      }
    } finally {
      busy = false;
    }
  }
</script>

<div class="reassign" data-testid="reassign-form">
  <p class="muted">{t('collab_relations.reassign.explain_does')}</p>
  <p class="muted">{t('collab_relations.reassign.explain_not')}</p>
  <p class="muted">{t('collab_relations.reassign.who')}</p>

  {#if !open}
    <div><Button size="sm" onclick={start}>{t('collab_relations.reassign.open')}</Button></div>
  {:else}
    <form class="form" onsubmit={submit}>
      <p class="muted">{t('collab_relations.reassign.current_owner', { owner: currentOwner })}</p>

      {#if sessionsLoading}
        <p class="muted">{t('collab_relations.reassign.sessions_loading')}</p>
      {:else if sessionsDenied}
        <p class="warn" data-testid="sessions-forbidden">{t('collab_relations.reassign.sessions_forbidden')}</p>
      {:else}
        {#if sessionsError}<ErrorBanner message={sessionsError} />{/if}
        <label class="field">
          <span class="form-label">{t('collab_relations.reassign.target_label')}</span>
          <select value={target} onchange={(e) => (target = e.currentTarget.value)} disabled={busy}>
            <option value="">{t('collab_relations.reassign.target_placeholder')}</option>
            {#each targets as s (s.id)}
              <option value={s.id}>
                {t('collab_relations.reassign.target_option', {
                  name: s.display_name,
                  kind: s.client_kind,
                  state: t(`collab.session_state.${s.state}`),
                })}
              </option>
            {/each}
          </select>
        </label>
        {#if targets.length === 0 && !sessionsError}
          <p class="muted" data-testid="sessions-none">{t('collab_relations.reassign.sessions_none')}</p>
        {/if}
      {/if}

      <label class="field">
        <span class="form-label">{t('collab_relations.reassign.reason_label')}</span>
        <textarea rows="3" bind:value={reason} disabled={busy}></textarea>
      </label>
      <p class="form-hint">{t('collab_relations.reassign.reason_hint', { max: MAX_REASON_BYTES })}</p>
      {#if reasonBytes > MAX_REASON_BYTES}
        <p class="warn">{t('collab_relations.reassign.reason_too_long', { max: MAX_REASON_BYTES })}</p>
      {/if}

      {#if actionError}<ErrorBanner message={actionError} />{/if}
      <div class="form-actions">
        <Button type="submit" variant="primary" size="sm" disabled={busy || !target || !reasonOk}>
          {busy ? t('collab_relations.reassign.submitting') : t('collab_relations.reassign.submit')}
        </Button>
        <Button size="sm" disabled={busy} onclick={() => (open = false)}>{t('common.cancel')}</Button>
      </div>
    </form>
  {/if}
</div>

<style>
  .reassign { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .muted { color: var(--text-muted); font-size: 12px; margin: 2px 0; }
  .warn { color: var(--yellow); font-size: 12px; margin: 2px 0; }
  .form { display: flex; flex-direction: column; gap: 8px; margin-top: 4px; }
  .field { display: flex; flex-direction: column; gap: 4px; }
  select, textarea { width: 100%; box-sizing: border-box; }
</style>
