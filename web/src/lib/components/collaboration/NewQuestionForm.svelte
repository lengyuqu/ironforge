<script lang="ts">
  // Ask a question on a task as the signed-in person. The server records it
  // under the user (no session headers), so any tool using this account could
  // have written the same thing. The question is bound to the task version the
  // person was looking at: if the task moved, the server refuses (VERSION_CONFLICT)
  // and this form re-reads the version so the person can look and send again.
  // The answer to a question is a collaboration decision, not an approval.
  import { untrack } from 'svelte';
  import { collaboration, type CollabQuestion, type CollabSession } from '$lib/api/collaboration';
  import { collabDecisions, type RecipientKind } from '$lib/api/collabDecisions';
  import { ApiError } from '$lib/api/_base.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { toast } from '$lib/components/toast.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import Button from '$lib/components/Button.svelte';
  import { newClientId } from '$lib/utils/collaboration';
  import { MAX_OPTIONS, cleanOptions, questionProblem } from '$lib/utils/collabDecisions';

  interface Props {
    owner: string;
    repo: string;
    taskId: string;
    /** Task version as the page last read it; the compare-and-set value. */
    taskVersion: number;
    /** Sessions of this repository the question can be addressed to (empty when they cannot be listed). */
    sessions?: CollabSession[];
    /** Called after the question was created. */
    onCreated?: (q: CollabQuestion) => void;
  }

  let { owner, repo, taskId, taskVersion, sessions = [], onCreated }: Props = $props();
  const t = createT();
  const msg = (e: unknown) => toErrorMessage(e, t('common.unknown_error'));

  let knownVersion = $state(untrack(() => taskVersion));
  $effect(() => {
    knownVersion = taskVersion;
  });

  let open = $state(false);
  let title = $state('');
  let body = $state('');
  let options = $state<string[]>([]);
  let recipientKind = $state<RecipientKind>('repo_writers');
  let userIdText = $state('');
  let sessionId = $state('');
  let recipientId = $derived(recipientKind === 'user' ? userIdText : recipientKind === 'session' ? sessionId : '');
  let key = $state(newClientId());
  let busy = $state(false);
  let error = $state('');

  let problem = $derived(questionProblem({ title, body, options, recipientKind, recipientId }));
  let liveSessions = $derived(sessions.filter((s) => s.state !== 'ended'));

  function reset() {
    title = body = userIdText = sessionId = '';
    options = [];
    recipientKind = 'repo_writers';
    key = newClientId();
    error = '';
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (problem || busy) return;
    busy = true;
    error = '';
    try {
      const created = await collabDecisions.createQuestion(owner, repo, taskId, {
        client_question_id: key,
        recipient:
          recipientKind === 'repo_writers'
            ? { kind: 'repo_writers' }
            : { kind: recipientKind, id: recipientId.trim() },
        title: title.trim(),
        body,
        options: cleanOptions(options),
        context: { task_version: knownVersion },
      });
      toast.success(t('collab_decisions.question.created_toast'));
      reset();
      open = false;
      onCreated?.(created);
    } catch (err) {
      error = msg(err);
      // A new attempt after a failure is a new request, not a replay.
      key = newClientId();
      if (err instanceof ApiError && err.code === 'VERSION_CONFLICT') {
        try {
          knownVersion = (await collaboration.getTask(owner, repo, taskId)).version;
        } catch {
          /* keep the error already shown */
        }
      }
    } finally {
      busy = false;
    }
  }
</script>

<div class="wrap" data-testid="new-question">
  <p class="notice">{t('collab_decisions.question.notice')}</p>
  {#if !open}
    <Button size="sm" variant="primary" onclick={() => (open = true)}>{t('collab_decisions.question.ask')}</Button>
  {:else}
    <form class="form" onsubmit={submit}>
      <label class="field">
        <span class="form-label">{t('collab_decisions.question.title_label')}</span>
        <input type="text" bind:value={title} maxlength="200" />
      </label>
      <label class="field">
        <span class="form-label">{t('collab_decisions.question.body_label')}</span>
        <textarea rows="4" bind:value={body}></textarea>
      </label>

      <fieldset>
        <legend>{t('collab_decisions.question.options_label')}</legend>
        <p class="hint">{t('collab_decisions.question.options_hint', { max: MAX_OPTIONS })}</p>
        {#each options as _, i (i)}
          <div class="optrow">
            <input
              type="text"
              bind:value={options[i]}
              maxlength="200"
              aria-label={t('collab_decisions.question.option_n', { n: i + 1 })}
            />
            <Button size="sm" onclick={() => (options = options.filter((_, j) => j !== i))}>
              {t('collab_decisions.question.remove_option')}
            </Button>
          </div>
        {/each}
        {#if options.length < MAX_OPTIONS}
          <Button size="sm" onclick={() => (options = [...options, ''])}>{t('collab_decisions.question.add_option')}</Button>
        {/if}
      </fieldset>

      <label class="field">
        <span class="form-label">{t('collab_decisions.question.recipient_label')}</span>
        <select value={recipientKind} onchange={(e) => (recipientKind = e.currentTarget.value as RecipientKind)}>
          <option value="repo_writers">{t('collab_decisions.question.recipient_repo_writers')}</option>
          <option value="user">{t('collab_decisions.question.recipient_user')}</option>
          {#if liveSessions.length > 0}
            <option value="session">{t('collab_decisions.question.recipient_session')}</option>
          {/if}
        </select>
      </label>
      {#if recipientKind === 'user'}
        <label class="field">
          <span class="form-label">{t('collab_decisions.question.user_id_label')}</span>
          <input type="text" inputmode="numeric" bind:value={userIdText} />
        </label>
      {:else if recipientKind === 'session'}
        <label class="field">
          <span class="form-label">{t('collab_decisions.question.session_label')}</span>
          <select value={sessionId} onchange={(e) => (sessionId = e.currentTarget.value)}>
            <option value="">{t('collab_decisions.question.choose_session')}</option>
            {#each liveSessions as s (s.id)}
              <option value={s.id}>{s.display_name} ({s.client_kind})</option>
            {/each}
          </select>
        </label>
      {/if}

      <p class="hint">{t('collab_decisions.question.context_hint', { version: knownVersion })}</p>
      <div class="actions">
        <Button type="submit" variant="primary" size="sm" disabled={busy || problem !== null}>
          {busy ? t('common.saving') : t('collab_decisions.question.send')}
        </Button>
        <Button size="sm" disabled={busy} onclick={() => (open = false)}>{t('common.cancel')}</Button>
        {#if problem}<span class="hint">{t(`collab_decisions.question.problem_${problem}`)}</span>{/if}
      </div>
      {#if error}<ErrorBanner message={error} />{/if}
    </form>
  {/if}
</div>

<style>
  .wrap { display: flex; flex-direction: column; gap: 8px; }
  .notice { margin: 0; font-size: 12px; color: var(--text-secondary); max-width: 80ch; }
  .form { display: flex; flex-direction: column; gap: 8px; border: 1px solid var(--border); border-radius: var(--radius); padding: 10px 12px; background: var(--bg-primary); }
  .field { display: flex; flex-direction: column; gap: 4px; }
  input[type='text'], textarea, select { width: 100%; box-sizing: border-box; }
  fieldset { border: 1px solid var(--border); border-radius: var(--radius); padding: 8px 12px; margin: 0; display: flex; flex-direction: column; gap: 6px; }
  legend { font-size: 12px; color: var(--text-secondary); padding: 0 4px; }
  .optrow { display: flex; gap: 8px; align-items: center; }
  .hint { margin: 0; font-size: 12px; color: var(--text-muted); }
  .actions { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
</style>
