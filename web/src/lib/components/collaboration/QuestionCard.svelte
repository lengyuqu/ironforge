<script lang="ts">
  // One question, loaded fresh from the server: the answer form needs the
  // question's current `version` and `context_hash`. A person answers with their
  // normal login (no session secret). An answer is a collaboration decision,
  // not a review or merge approval. Everything client-written is shown as text.
  import { untrack } from 'svelte';
  import { collaboration, type CollabQuestion } from '$lib/api/collaboration';
  import { ApiError } from '$lib/api/_base.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingSpinner from '$lib/components/LoadingSpinner.svelte';
  import Button from '$lib/components/Button.svelte';
  import StatusBadge from './StatusBadge.svelte';
  import {
    KNOWN,
    answerProblem,
    buildAnswer,
    canAnswerQuestion,
    canCancelQuestion,
    formatTime,
    needsReload,
    newClientId,
    shortId,
  } from '$lib/utils/collaboration';

  interface Props {
    owner: string;
    repo: string;
    questionId: string;
    /** Signed-in user id; decides who may cancel. */
    userId: number | null;
    /** Called after the question changed (answered / cancelled). */
    onChanged?: (q: CollabQuestion) => void;
  }

  let { owner, repo, questionId, userId, onChanged }: Props = $props();
  const t = createT();

  let question = $state<CollabQuestion | null>(null);
  let loading = $state(true);
  let loadError = $state('');
  let actionError = $state('');
  let busy = $state(false);

  let body = $state('');
  let optionId = $state<string | null>(null);
  let clientAnswerId = $state(newClientId());

  async function load() {
    loading = true;
    loadError = '';
    try {
      question = await collaboration.getQuestion(owner, repo, questionId);
    } catch (e) {
      loadError = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    // Reload when the question id changes; the form state is not a dependency.
    void questionId;
    untrack(load);
  });

  let problem = $derived(question ? answerProblem(question, { body, optionId }) : null);
  // The server refuses to record an answer to a stale question (CONTEXT_STALE), so
  // offer no form for one; the asker has to re-ask in the current context.
  let isStale = $derived(question?.context_freshness === 'stale');
  let showAnswerForm = $derived(question !== null && canAnswerQuestion(question) && !isStale);

  function chosenLabel(q: CollabQuestion, id: string): string {
    return q.options.find((o) => o.id === id)?.label ?? id;
  }

  async function submitAnswer(e: SubmitEvent) {
    e.preventDefault();
    if (!question || problem || busy) return;
    busy = true;
    actionError = '';
    try {
      const updated = await collaboration.answerQuestion(
        owner,
        repo,
        question.id,
        buildAnswer(question, { body, optionId, clientAnswerId }),
      );
      question = updated;
      toast.success(t('collab.question.answered_toast'));
      onChanged?.(updated);
    } catch (err) {
      actionError = toErrorMessage(err, t('common.unknown_error'));
      if (err instanceof ApiError && needsReload(err.code)) {
        // The question moved under us: show its new state and use a fresh key.
        clientAnswerId = newClientId();
        await load();
      }
    } finally {
      busy = false;
    }
  }

  async function cancel() {
    if (!question || busy) return;
    if (!(await confirmDialog({
        variant: 'danger',
        message: t('collab.question.cancel_confirm'),
        confirmLabel: t('collab.question.cancel'),
        cancelLabel: t('collab.question.keep'),
      }))) return;
    busy = true;
    actionError = '';
    try {
      const updated = await collaboration.cancelQuestion(owner, repo, question.id, question.version);
      question = updated;
      toast.success(t('collab.question.cancelled_toast'));
      onChanged?.(updated);
    } catch (err) {
      actionError = toErrorMessage(err, t('common.unknown_error'));
      if (err instanceof ApiError && needsReload(err.code)) await load();
    } finally {
      busy = false;
    }
  }
</script>

<div class="qcard" data-testid="question-card">
  {#if loading && !question}
    <LoadingSpinner />
  {:else if loadError}
    <ErrorBanner message={loadError}>
      <Button size="sm" onclick={load}>{t('common.retry')}</Button>
    </ErrorBanner>
  {:else if question}
    <div class="head">
      <strong class="title">{question.title}</strong>
      <StatusBadge
        value={question.status}
        prefix="collab.question_status"
        known={KNOWN.questionStatus}
        good={['answered']}
        warn={['open']}
        title={t('collab.declared_hint')}
      />
    </div>
    <p class="meta">
      {t('collab.question.asked_by', { kind: question.creator_kind, id: shortId(question.creator_id) })}
      · {t('collab.question.recipient', { kind: question.recipient.kind })}
      · {formatTime(question.created_at)}
    </p>
    <div class="body">{question.body}</div>

    <p class="meta ctx">
      {t('collab.question.context', { version: question.context.task_version })}
      {#if question.context.head_sha}<code>{shortId(question.context.head_sha)}</code>{/if}
      · {t('collab.question.freshness')}:
      <StatusBadge
        value={question.context_freshness}
        prefix="collab.freshness"
        known={KNOWN.freshness}
        good={['fresh']}
        warn={['stale']}
      />
    </p>
    {#if isStale && question.status === 'open'}
      <p class="warn" data-testid="stale-warning">{t('collab.question.stale_warning')}</p>
    {/if}

    {#if question.answer}
      <div class="answer" data-testid="question-answer">
        <p class="meta">
          {t('collab.question.answered_by', { kind: question.answer.actor_kind, id: shortId(question.answer.actor_id) })}
          · {formatTime(question.answer.created_at)}
        </p>
        {#if question.answer.option_id}
          <p class="chosen">
            {chosenLabel(question, question.answer.option_id)}
          </p>
        {/if}
        {#if question.answer.body}<div class="body">{question.answer.body}</div>{/if}
      </div>
    {/if}

    {#if showAnswerForm}
      <form onsubmit={submitAnswer} class="form">
        {#if question.options.length > 0}
          <fieldset>
            <legend>{t('collab.question.choose')}</legend>
            {#each question.options as opt (opt.id)}
              <label class="opt">
                <input type="radio" name={`opt-${question.id}`} value={opt.id} bind:group={optionId} />
                <span>{opt.label}</span>
              </label>
            {/each}
          </fieldset>
        {/if}
        <label class="field">
          <span class="form-label">
            {question.options.length > 0 ? t('collab.question.comment') : t('collab.question.your_answer')}
          </span>
          <textarea rows="3" bind:value={body} maxlength="16000"></textarea>
        </label>
        <p class="form-hint">{t('collab.question.answer_hint')}</p>
        <div class="form-actions">
          <Button type="submit" variant="primary" size="sm" disabled={busy || problem !== null}>
            {busy ? t('common.saving') : t('collab.question.send_answer')}
          </Button>
          {#if canCancelQuestion(question, userId)}
            <Button variant="danger" size="sm" disabled={busy} onclick={cancel}>
              {t('collab.question.cancel')}
            </Button>
          {/if}
          {#if problem}<span class="form-hint">{t(`collab.question.problem_${problem}`)}</span>{/if}
        </div>
      </form>
    {:else if canCancelQuestion(question, userId)}
      <Button variant="danger" size="sm" disabled={busy} onclick={cancel}>{t('collab.question.cancel')}</Button>
    {/if}
    {#if actionError}<ErrorBanner message={actionError} />{/if}
  {/if}
</div>

<style>
  .qcard { border: 1px solid var(--border); border-radius: var(--radius); padding: 12px 14px; margin-top: 8px; background: var(--bg-primary); }
  .head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .title { overflow-wrap: anywhere; }
  .meta { margin: 4px 0; font-size: 12px; color: var(--text-muted); }
  .body { white-space: pre-wrap; overflow-wrap: anywhere; margin: 8px 0; font-size: 14px; }
  .warn { color: var(--yellow); font-size: 12px; margin: 4px 0; }
  .answer { border-left: 3px solid var(--green-dim); padding-left: 10px; margin: 10px 0; }
  .chosen { margin: 4px 0; font-weight: 600; overflow-wrap: anywhere; }
  .form { margin-top: 10px; display: flex; flex-direction: column; gap: 8px; }
  fieldset { border: 1px solid var(--border); border-radius: var(--radius); padding: 8px 12px; margin: 0; }
  legend { font-size: 12px; color: var(--text-secondary); padding: 0 4px; }
  .opt { display: flex; align-items: center; gap: 8px; padding: 2px 0; overflow-wrap: anywhere; }
  .field { display: flex; flex-direction: column; gap: 4px; }
  textarea { width: 100%; box-sizing: border-box; }
  code { font-family: var(--font-mono); }
</style>
