<script lang="ts">
  // The things a signed-in person can do with a task's decisions: record one,
  // and supersede or revoke an active one. The read-only list stays in the
  // page; this panel shows only what can still be acted on.
  //
  // A decision is a collaboration RECORD written under the signed-in account.
  // It is not an approval, a review or a merge authorization and it grants no
  // Git, CI or branch-protection right. A decision whose context went stale is
  // shown as stale and stays active until someone revokes it. Each decision
  // takes exactly one terminal change; the server arbitrates races.
  import { untrack } from 'svelte';
  import { collaboration, type CollabDecision, type CollabQuestion } from '$lib/api/collaboration';
  import { collabDecisions, type DecisionChangeKind } from '$lib/api/collabDecisions';
  import { ApiError } from '$lib/api/_base.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import Button from '$lib/components/Button.svelte';
  import StatusBadge from './StatusBadge.svelte';
  import { KNOWN, formatTime, newClientId, shortId } from '$lib/utils/collaboration';
  import {
    MAX_REASON_BYTES,
    MAX_RATIONALE_BYTES,
    decisionNeedsReload,
    replacementCandidates,
    textProblem,
    titleProblem,
  } from '$lib/utils/collabDecisions';

  interface Props {
    owner: string;
    repo: string;
    taskId: string;
    /** Task version as the page last read it; the compare-and-set value for new decisions. */
    taskVersion: number;
    /** The task's decisions as the page loaded them. */
    decisions: CollabDecision[];
    /** The task's questions (summaries), for the optional link. */
    questions: CollabQuestion[];
    /** Called after the decisions changed, or must be re-read. */
    onChanged?: () => void;
  }

  let { owner, repo, taskId, taskVersion, decisions, questions, onChanged }: Props = $props();
  const t = createT();
  const msg = (e: unknown) => toErrorMessage(e, t('common.unknown_error'));

  // The version this panel acts on; re-read from the server after VERSION_CONFLICT.
  let knownVersion = $state(untrack(() => taskVersion));
  $effect(() => {
    knownVersion = taskVersion;
  });

  let active = $derived(decisions.filter((d) => d.status === 'active'));

  // ── record a decision ──
  let formOpen = $state(false);
  let title = $state('');
  let rationale = $state('');
  let questionId = $state('');
  let citeAnswer = $state(false);
  let answerId = $state<string | null>(null);
  let answerLoading = $state(false);
  let createKey = $state(newClientId());
  let busy = $state(false);
  let createError = $state('');

  let titleIssue = $derived(titleProblem(title));
  let rationaleIssue = $derived(textProblem(rationale, MAX_RATIONALE_BYTES));
  let canCreate = $derived(!titleIssue && !rationaleIssue && !busy && !answerLoading && !(citeAnswer && !answerId));

  async function onQuestionPicked(picked: string) {
    // Only an answered question has an answer to cite; fetch its id.
    citeAnswer = false;
    answerId = null;
    const q = questions.find((x) => x.id === picked);
    if (!q || q.status !== 'answered') return;
    answerLoading = true;
    try {
      const got = (await collaboration.getQuestion(owner, repo, q.id)).answer?.id ?? null;
      if (questionId === picked) answerId = got;
    } catch (e) {
      if (questionId === picked) createError = msg(e);
    } finally {
      answerLoading = false;
    }
  }
  $effect(() => {
    const picked = questionId;
    untrack(() => void onQuestionPicked(picked));
  });

  /** Re-read the task so the next attempt is judged against what it looks like now. */
  async function refreshVersion() {
    try {
      knownVersion = (await collaboration.getTask(owner, repo, taskId)).version;
    } catch {
      /* the error already shown is the more useful one */
    }
  }

  async function submitCreate(e: SubmitEvent) {
    e.preventDefault();
    if (!canCreate) return;
    busy = true;
    createError = '';
    try {
      await collabDecisions.createDecision(owner, repo, taskId, {
        client_decision_id: createKey,
        title: title.trim(),
        rationale,
        question_id: questionId || null,
        answer_id: questionId && citeAnswer ? answerId : null,
        context: { task_version: knownVersion },
      });
      toast.success(t('collab_decisions.panel.created_toast'));
      title = rationale = questionId = '';
      citeAnswer = false;
      answerId = null;
      createKey = newClientId();
      formOpen = false;
      onChanged?.();
    } catch (err) {
      createError = msg(err);
      // A new attempt after a failure is a new request, not a replay.
      createKey = newClientId();
      if (err instanceof ApiError && decisionNeedsReload(err.code)) {
        await refreshVersion();
        onChanged?.();
      }
    } finally {
      busy = false;
    }
  }

  // ── supersede / revoke ──
  let changing = $state<{ id: string; kind: DecisionChangeKind } | null>(null);
  let reason = $state('');
  let replacementId = $state('');
  let changeKey = $state(newClientId());
  let changeError = $state('');

  let target = $derived(changing ? (decisions.find((d) => d.id === changing!.id) ?? null) : null);
  let candidates = $derived(target ? replacementCandidates(target, decisions) : []);
  let reasonIssue = $derived(textProblem(reason, MAX_REASON_BYTES));
  let canChange = $derived(
    changing !== null && !busy && !reasonIssue && (changing.kind === 'revoked' || replacementId !== ''),
  );

  function startChange(d: CollabDecision, kind: DecisionChangeKind) {
    changing = { id: d.id, kind };
    reason = '';
    replacementId = '';
    changeKey = newClientId();
    changeError = '';
  }

  async function submitChange(e: SubmitEvent) {
    e.preventDefault();
    if (!changing || !canChange) return;
    const { id, kind } = changing;
    if (
      !(await confirmDialog({
        variant: 'danger',
        message: t(kind === 'revoked' ? 'collab_decisions.panel.revoke_confirm' : 'collab_decisions.panel.supersede_confirm'),
        confirmLabel: t(kind === 'revoked' ? 'collab_decisions.panel.revoke' : 'collab_decisions.panel.supersede'),
        cancelLabel: t('common.cancel'),
      }))
    )
      return;
    busy = true;
    changeError = '';
    try {
      await collabDecisions.changeDecision(owner, repo, id, {
        client_change_id: changeKey,
        kind,
        replacement_decision_id: kind === 'superseded' ? replacementId : null,
        reason,
      });
      toast.success(t(kind === 'revoked' ? 'collab_decisions.panel.revoked_toast' : 'collab_decisions.panel.superseded_toast'));
      changing = null;
      onChanged?.();
    } catch (err) {
      changeError = msg(err);
      changeKey = newClientId();
      if (err instanceof ApiError && decisionNeedsReload(err.code)) {
        // Someone else changed it, or the task moved: show the new state.
        if (err.code === 'DECISION_ALREADY_CHANGED') changing = null;
        await refreshVersion();
        onChanged?.();
      }
    } finally {
      busy = false;
    }
  }

  function cancelChange() {
    changing = null;
    changeError = '';
  }

  function questionLabel(q: CollabQuestion): string {
    return `${q.title} (${KNOWN.questionStatus.includes(q.status as never) ? t(`collab.question_status.${q.status}`) : q.status})`;
  }
</script>

<div class="panel" data-testid="decision-panel">
  <p class="notice">{t('collab_decisions.panel.not_approval')}</p>

  {#if !formOpen}
    <Button size="sm" variant="primary" onclick={() => (formOpen = true)}>{t('collab_decisions.panel.record')}</Button>
  {:else}
    <form class="form" onsubmit={submitCreate} data-testid="decision-form">
      <label class="field">
        <span class="form-label">{t('collab_decisions.panel.title_label')}</span>
        <input type="text" bind:value={title} maxlength="200" />
      </label>
      <label class="field">
        <span class="form-label">{t('collab_decisions.panel.rationale_label')}</span>
        <textarea rows="4" bind:value={rationale}></textarea>
      </label>
      <p class="hint">{t('collab_decisions.panel.rationale_hint')}</p>
      {#if questions.length > 0}
        <label class="field">
          <span class="form-label">{t('collab_decisions.panel.question_label')}</span>
          <select value={questionId} onchange={(e) => (questionId = e.currentTarget.value)}>
            <option value="">{t('collab_decisions.panel.no_question')}</option>
            {#each questions as q (q.id)}
              <option value={q.id}>{questionLabel(q)}</option>
            {/each}
          </select>
        </label>
        {#if answerId}
          <label class="opt">
            <input type="checkbox" bind:checked={citeAnswer} />
            <span>{t('collab_decisions.panel.cite_answer')}</span>
          </label>
        {/if}
      {/if}
      <p class="hint">{t('collab_decisions.panel.context_hint', { version: knownVersion })}</p>
      <div class="actions">
        <Button type="submit" variant="primary" size="sm" disabled={!canCreate}>
          {busy ? t('common.saving') : t('collab_decisions.panel.save')}
        </Button>
        <Button size="sm" disabled={busy} onclick={() => (formOpen = false)}>{t('common.cancel')}</Button>
        {#if titleIssue}<span class="hint">{t(`collab_decisions.panel.problem_title_${titleIssue}`)}</span>
        {:else if rationaleIssue}<span class="hint">{t(`collab_decisions.panel.problem_rationale_${rationaleIssue}`)}</span>{/if}
      </div>
      {#if createError}<ErrorBanner message={createError} />{/if}
    </form>
  {/if}

  <h3>{t('collab_decisions.panel.actionable')}</h3>
  {#if active.length === 0}
    <p class="muted">{t('collab_decisions.panel.none_active')}</p>
  {:else}
    <ul class="list">
      {#each active as d (d.id)}
        <li data-testid="active-decision">
          <div class="row">
            <span class="muted">#{d.sequence}</span>
            <strong class="grow text">{d.title}</strong>
            <StatusBadge value={d.context_freshness} prefix="collab.freshness" known={KNOWN.freshness} good={['fresh']} warn={['stale']} />
          </div>
          <p class="muted">
            {t('collab.decisions.by', { kind: d.actor_kind, id: shortId(d.actor_id) })} · {formatTime(d.created_at)}
          </p>
          {#if d.context_freshness === 'stale'}
            <p class="warn">{t('collab_decisions.panel.stale_note')}</p>
          {/if}
          {#if changing?.id === d.id}
            <form class="form" onsubmit={submitChange} data-testid="change-form">
              <p class="form-label">
                {t(changing.kind === 'revoked' ? 'collab_decisions.panel.revoking' : 'collab_decisions.panel.superseding')}
              </p>
              {#if changing.kind === 'superseded'}
                {#if candidates.length === 0}
                  <p class="warn">{t('collab_decisions.panel.no_replacement')}</p>
                {:else}
                  <label class="field">
                    <span class="form-label">{t('collab_decisions.panel.replacement_label')}</span>
                    <select value={replacementId} onchange={(e) => (replacementId = e.currentTarget.value)}>
                      <option value="">{t('collab_decisions.panel.choose_replacement')}</option>
                      {#each candidates as c (c.id)}
                        <option value={c.id}>#{c.sequence} {c.title}</option>
                      {/each}
                    </select>
                  </label>
                {/if}
              {/if}
              <label class="field">
                <span class="form-label">{t('collab_decisions.panel.reason_label')}</span>
                <textarea rows="3" bind:value={reason}></textarea>
              </label>
              <p class="hint">{t('collab_decisions.panel.change_hint')}</p>
              <div class="actions">
                <Button type="submit" variant="danger" size="sm" disabled={!canChange}>
                  {busy ? t('common.saving') : t(changing.kind === 'revoked' ? 'collab_decisions.panel.revoke' : 'collab_decisions.panel.supersede')}
                </Button>
                <Button size="sm" disabled={busy} onclick={cancelChange}>{t('common.cancel')}</Button>
              </div>
              {#if changeError}<ErrorBanner message={changeError} />{/if}
            </form>
          {:else}
            <div class="actions">
              <Button size="sm" disabled={busy} onclick={() => startChange(d, 'superseded')}>{t('collab_decisions.panel.supersede')}</Button>
              <Button size="sm" variant="danger" disabled={busy} onclick={() => startChange(d, 'revoked')}>{t('collab_decisions.panel.revoke')}</Button>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#if changeError && !changing}<ErrorBanner message={changeError} />{/if}
</div>

<style>
  .panel { display: flex; flex-direction: column; gap: 8px; }
  .notice { margin: 0; font-size: 12px; color: var(--text-secondary); max-width: 80ch; }
  h3 { margin: 12px 0 0; font-size: 14px; }
  .form { display: flex; flex-direction: column; gap: 8px; border: 1px solid var(--border); border-radius: var(--radius); padding: 10px 12px; background: var(--bg-primary); }
  .field { display: flex; flex-direction: column; gap: 4px; }
  input[type='text'], textarea, select { width: 100%; box-sizing: border-box; }
  .opt { display: flex; align-items: center; gap: 8px; font-size: 13px; }
  .hint { margin: 0; font-size: 12px; color: var(--text-muted); }
  .muted { color: var(--text-muted); font-size: 12px; margin: 2px 0; }
  .warn { color: var(--yellow); font-size: 12px; margin: 4px 0; }
  .actions { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .list { list-style: none; margin: 0; padding: 0; }
  .list > li { padding: 8px 0; border-top: 1px solid var(--border-light); }
  .list > li:first-child { border-top: none; }
  .row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .grow { flex: 1 1 200px; min-width: 0; }
  .text { overflow-wrap: anywhere; }
</style>
