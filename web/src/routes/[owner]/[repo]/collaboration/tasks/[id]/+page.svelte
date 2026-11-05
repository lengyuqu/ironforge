<script lang="ts">
  // One collaboration task, read-only except for answering / cancelling
  // questions. Every section loads independently so one failing record does not
  // hide the rest. Client-written fields are rendered as text only (no HTML,
  // no Markdown). Statuses, `blocked_by` and overlaps are declarations or
  // hints, never proof — each section says so.
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingSpinner from '$lib/components/LoadingSpinner.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Button from '$lib/components/Button.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { ApiError } from '$lib/api/_base.svelte';
  import {
    collaboration,
    loadSessionMap,
    type CollabDecision,
    type CollabHandoff,
    type CollabMessage,
    type CollabQuestion,
    type CollabSession,
    type CollabTask,
    type CompletionStatus,
    type DependencyList,
    type Diagnosis,
    type OverlapItem,
  } from '$lib/api/collaboration';
  import { getUser } from '$lib/stores/auth.svelte';
  import { KNOWN, formatTime, shortId } from '$lib/utils/collaboration';
  import SectionCard from '$lib/components/collaboration/SectionCard.svelte';
  import StatusBadge from '$lib/components/collaboration/StatusBadge.svelte';
  import QuestionCard from '$lib/components/collaboration/QuestionCard.svelte';
  import ConditionsCard from '$lib/components/collaboration/ConditionsCard.svelte';
  import DependenciesEditor from '$lib/components/collaboration/DependenciesEditor.svelte';
  import ReassignForm from '$lib/components/collaboration/ReassignForm.svelte';
  import TakeoversCard from '$lib/components/collaboration/TakeoversCard.svelte';
  import BudgetCard from '$lib/components/collaboration/BudgetCard.svelte';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let taskId = $derived($page.params.id!);
  let userId = $derived(getUser()?.id ?? null);

  let task = $state<CollabTask | null>(null);
  let taskLoading = $state(true);
  let taskError = $state('');
  let notFound = $state(false);
  let sessions = $state<Map<string, CollabSession> | null>(null);

  // Independent sections
  let completion = $state<CompletionStatus | null>(null);
  let completionError = $state('');
  let deps = $state<DependencyList | null>(null);
  let depsError = $state('');
  let overlaps = $state<OverlapItem[]>([]);
  let overlapsCursor = $state<string | null>(null);
  let overlapsError = $state('');
  let overlapsLoading = $state(true);
  let questions = $state<CollabQuestion[]>([]);
  let questionsCursor = $state<string | null>(null);
  let questionsError = $state('');
  let questionsLoading = $state(true);
  let openQuestionId = $state<string | null>(null);
  let decisions = $state<CollabDecision[]>([]);
  let decisionsCursor = $state<string | null>(null);
  let decisionsError = $state('');
  let decisionsLoading = $state(true);
  let handoffs = $state<CollabHandoff[]>([]);
  let handoffsCursor = $state<string | null>(null);
  let handoffsError = $state('');
  let handoffsLoading = $state(true);
  let messages = $state<CollabMessage[]>([]);
  let messagesAfter = $state(0);
  let messagesMore = $state(false);
  let messagesError = $state('');
  let messagesLoading = $state(true);
  let diagnosis = $state<Diagnosis | null>(null);
  let diagnosisError = $state('');
  let diagnosisLoading = $state(false);

  const msg = (e: unknown) => toErrorMessage(e, t('common.unknown_error'));

  async function loadTask() {
    taskLoading = true;
    taskError = '';
    notFound = false;
    try {
      task = await collaboration.getTask(owner, repo, taskId);
    } catch (e) {
      task = null;
      if (e instanceof ApiError && e.status === 404) notFound = true;
      else taskError = msg(e);
    } finally {
      taskLoading = false;
    }
  }

  /** Re-read the task without the loading state, so the sections below keep their form state. */
  async function refreshTask() {
    try {
      task = await collaboration.getTask(owner, repo, taskId);
    } catch (e) {
      taskError = msg(e);
    }
  }

  async function loadCompletion() {
    completionError = '';
    try {
      completion = await collaboration.completionStatus(owner, repo, taskId);
    } catch (e) {
      completionError = msg(e);
    }
  }
  async function loadDeps() {
    depsError = '';
    try {
      deps = await collaboration.listDependencies(owner, repo, taskId);
    } catch (e) {
      depsError = msg(e);
    }
  }
  async function loadOverlaps(append = false) {
    overlapsLoading = true;
    overlapsError = '';
    try {
      const res = await collaboration.listOverlaps(owner, repo, taskId, {
        limit: 25,
        cursor: append ? overlapsCursor : null,
      });
      overlaps = append ? [...overlaps, ...res.items] : res.items;
      overlapsCursor = res.next_cursor;
    } catch (e) {
      overlapsError = msg(e);
    } finally {
      overlapsLoading = false;
    }
  }
  async function loadQuestions(append = false) {
    questionsLoading = true;
    questionsError = '';
    try {
      const res = await collaboration.listQuestions(owner, repo, taskId, {
        limit: 25,
        cursor: append ? questionsCursor : null,
      });
      questions = append ? [...questions, ...res.items] : res.items;
      questionsCursor = res.next_cursor;
    } catch (e) {
      questionsError = msg(e);
    } finally {
      questionsLoading = false;
    }
  }
  async function loadDecisions(append = false) {
    decisionsLoading = true;
    decisionsError = '';
    try {
      const res = await collaboration.listDecisions(owner, repo, taskId, {
        limit: 25,
        cursor: append ? decisionsCursor : null,
      });
      decisions = append ? [...decisions, ...res.items] : res.items;
      decisionsCursor = res.next_cursor;
    } catch (e) {
      decisionsError = msg(e);
    } finally {
      decisionsLoading = false;
    }
  }
  async function loadHandoffs(append = false) {
    handoffsLoading = true;
    handoffsError = '';
    try {
      const res = await collaboration.listHandoffs(owner, repo, taskId, {
        limit: 25,
        cursor: append ? handoffsCursor : null,
      });
      handoffs = append ? [...handoffs, ...res.items] : res.items;
      handoffsCursor = res.next_cursor;
    } catch (e) {
      handoffsError = msg(e);
    } finally {
      handoffsLoading = false;
    }
  }
  async function loadMessages(append = false) {
    messagesLoading = true;
    messagesError = '';
    try {
      const res = await collaboration.listMessages(owner, repo, taskId, {
        afterSeq: append ? messagesAfter : 0,
        limit: 50,
      });
      messages = append ? [...messages, ...res.items] : res.items;
      messagesAfter = res.next_after_seq;
      messagesMore = res.has_more;
    } catch (e) {
      messagesError = msg(e);
    } finally {
      messagesLoading = false;
    }
  }
  async function runDiagnostics() {
    diagnosisLoading = true;
    diagnosisError = '';
    try {
      diagnosis = await collaboration.diagnoseTask(owner, repo, taskId);
    } catch (e) {
      diagnosisError = msg(e);
    } finally {
      diagnosisLoading = false;
    }
  }

  function resetAndLoad() {
    task = completion = deps = diagnosis = null;
    overlaps = questions = decisions = handoffs = messages = [];
    openQuestionId = null;
    loadSessionMap(owner, repo).then((m) => (sessions = m)).catch(() => (sessions = null));
    loadTask().then(() => {
      if (!task) return;
      loadCompletion();
      loadDeps();
      loadOverlaps();
      loadQuestions();
      loadDecisions();
      loadHandoffs();
      loadMessages();
    });
  }

  $effect(() => {
    void owner;
    void repo;
    void taskId;
    resetAndLoad();
  });

  function sessionLabel(id: string | null): string {
    if (!id) return t('collab.unowned');
    const s = sessions?.get(id);
    return s ? `${s.display_name} (${s.client_kind})` : shortId(id);
  }
  const taskHref = (id: string) => `/${owner}/${repo}/collaboration/tasks/${id}`;

  function onQuestionChanged() {
    loadQuestions();
    loadCompletion();
  }
</script>

<svelte:head>
  <title>{task ? task.title : t('collab.task.title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="collaboration" />

  <p class="back"><a href={`/${owner}/${repo}/collaboration`}>← {t('collab.title')}</a></p>

  {#if taskLoading}
    <LoadingSpinner />
  {:else if notFound}
    <EmptyState title={t('collab.task.not_found')} />
  {:else if taskError}
    <ErrorBanner message={taskError} />
  {:else if task}
    <div class="page-header">
      <h1 class="text">{task.title}</h1>
      <div class="head-chips">
        <StatusBadge
          value={task.status}
          prefix="collab.task_status"
          known={KNOWN.taskStatus}
          good={['done']}
          warn={['waiting', 'blocked']}
          title={t('collab.declared_hint')}
        />
        <span class="muted">{t('collab.declared_label')}</span>
      </div>
    </div>
    <p class="intro">{t('collab.intro')}</p>

    <SectionCard title={t('collab.task.details')} note={t('collab.task.details_note')}>
      <dl class="facts">
        <dt>{t('collab.field.owner_session')}</dt>
        <dd>
          <span class="text">{sessionLabel(task.owner_session_id)}</span>
          {#if task.owner_session_id}
            {#if sessions?.get(task.owner_session_id)}
              <StatusBadge
                value={sessions.get(task.owner_session_id)!.state}
                prefix="collab.session_state"
                known={KNOWN.sessionState}
                good={['online']}
              />
              <span class="muted">{t('collab.self_reported')}</span>
            {:else}
              <span class="muted">{t('collab.session_state_unknown')}</span>
            {/if}
          {/if}
        </dd>
        <dt>{t('collab.field.branch')}</dt>
        <dd class="mono text">{task.branch ?? '—'}</dd>
        <dt>{t('collab.field.paths')}</dt>
        <dd>
          {#if task.paths.length === 0}
            —
          {:else}
            <ul class="paths">
              {#each task.paths as p}<li class="mono text">{p}</li>{/each}
            </ul>
          {/if}
          <span class="muted">
            {task.paths_declared_at
              ? t('collab.paths_declared_at', { time: formatTime(task.paths_declared_at) })
              : t('collab.paths_declared_unknown')}
          </span>
        </dd>
        <dt>{t('collab.field.links')}</dt>
        <dd>
          {#if task.issue_number != null}<a href={`/${owner}/${repo}/issues/${task.issue_number}`}>#{task.issue_number}</a>{/if}
          {#if task.pr_number != null}<a href={`/${owner}/${repo}/pulls/${task.pr_number}`}>PR #{task.pr_number}</a>{/if}
          {#if task.issue_number == null && task.pr_number == null}—{/if}
        </dd>
        <dt>{t('collab.field.version')}</dt>
        <dd>{task.version} · {t('collab.field.last_message_seq')} {task.last_message_seq}</dd>
        <dt>{t('collab.field.created')}</dt>
        <dd>{formatTime(task.created_at)}</dd>
        <dt>{t('collab.field.updated')}</dt>
        <dd>{formatTime(task.updated_at)}</dd>
        <dt>ID</dt>
        <dd class="mono">{task.id}</dd>
      </dl>
      {#if task.description}<div class="body">{task.description}</div>{/if}
    </SectionCard>

    <SectionCard title={t('collab_relations.reassign.title')} note={t('collab_relations.reassign.note')}>
      <ReassignForm {owner} {repo} {task} onChanged={refreshTask} />
    </SectionCard>

    <SectionCard title={t('collab_relations.takeovers.title')} note={t('collab_relations.takeovers.note')}>
      <TakeoversCard {owner} {repo} {taskId} refreshKey={task.version} {sessionLabel} />
    </SectionCard>

    <SectionCard title={t('collab.completion.title')} note={t('collab.completion.note')} error={completionError}>
      {#if completion}
        <p class="row">
          <StatusBadge
            value={completion.overall}
            prefix="collab.overall"
            known={KNOWN.overall}
            good={['all_satisfied']}
            warn={['incomplete']}
            bad={['unsatisfied']}
          />
          {#if completion.declared_done_without_evidence}
            <span class="warn">{t('collab.completion.done_without_evidence')}</span>
          {/if}
        </p>
        <ul class="list">
          {#each completion.conditions as c (c.id)}
            <li>
              <div class="row">
                <strong>{KNOWN.condition.includes(c.id as never) ? t(`collab.condition.${c.id}`) : c.id}</strong>
                <StatusBadge
                  value={c.status}
                  prefix="collab.condition_status"
                  known={KNOWN.conditionStatus}
                  good={['satisfied']}
                  warn={['stale', 'unknown']}
                  bad={['unsatisfied']}
                />
              </div>
              <p class="muted text">{c.detail}</p>
              {#each c.evidence as ev}
                <p class="muted mono">
                  {t('collab.completion.pipeline', { id: ev.pipeline_id })} · {ev.status ?? '—'}
                  {#if ev.commit_sha}· {shortId(ev.commit_sha)}{/if}
                </p>
              {/each}
            </li>
          {/each}
        </ul>
        {#if completion.observed_branch_head}
          <p class="muted">{t('collab.completion.head')}: <code>{shortId(completion.observed_branch_head)}</code></p>
        {/if}
      {/if}
    </SectionCard>

    <SectionCard title={t('collab.conditions.title')} note={t('collab.conditions.note')}>
      {#if completion}
        <ConditionsCard {owner} {repo} {taskId} defined={completion.defined} onChanged={loadCompletion} />
      {/if}
    </SectionCard>

    <SectionCard title={t('collab.deps.title')} note={t('collab.deps.note')} error={depsError}>
      {#if deps}
        <DependenciesEditor {owner} {repo} {taskId} {deps} onChanged={loadDeps} />
      {/if}
    </SectionCard>

    <SectionCard
      title={t('collab.overlaps.title')}
      note={t('collab.overlaps.note')}
      loading={overlapsLoading && overlaps.length === 0}
      error={overlapsError}
    >
      {#if overlaps.length === 0}
        <p class="muted">{t('collab.overlaps.none')}</p>
      {:else}
        <ul class="list">
          {#each overlaps as o (o.task_id)}
            <li>
              <div class="row">
                <a class="grow mono" href={taskHref(o.task_id)}>{shortId(o.task_id)}</a>
                <StatusBadge value={o.status} prefix="collab.task_status" known={KNOWN.taskStatus} />
                <StatusBadge
                  value={o.confidence}
                  prefix="collab.confidence"
                  known={KNOWN.confidence}
                  warn={['declaration_stale']}
                />
                {#if o.owner_state}
                  <StatusBadge value={o.owner_state} prefix="collab.session_state" known={KNOWN.sessionState} good={['online']} />
                {/if}
              </div>
              {#if o.branch}<p class="muted mono text">{o.branch}</p>{/if}
              <ul class="paths">
                {#each o.overlaps as p}
                  <li class="mono text">
                    {p.mine} ⟷ {p.theirs}
                    <span class="muted">({KNOWN.overlapReason.includes(p.reason as never) ? t(`collab.overlap_reason.${p.reason}`) : p.reason})</span>
                  </li>
                {/each}
              </ul>
              {#if o.overlaps_truncated}<p class="muted">{t('collab.truncated')}</p>{/if}
            </li>
          {/each}
        </ul>
      {/if}
      {#if overlapsCursor}
        <Button size="sm" disabled={overlapsLoading} onclick={() => loadOverlaps(true)}>{t('collab.load_more')}</Button>
      {/if}
    </SectionCard>

    <SectionCard
      title={t('collab.questions.title')}
      note={t('collab.questions.note')}
      loading={questionsLoading && questions.length === 0}
      error={questionsError}
      count={questions.length}
    >
      {#if questions.length === 0}
        <p class="muted">{t('collab.none')}</p>
      {:else}
        <ul class="list">
          {#each questions as q (q.id)}
            <li>
              <div class="row">
                <span class="grow text">{q.title}</span>
                <StatusBadge
                  value={q.status}
                  prefix="collab.question_status"
                  known={KNOWN.questionStatus}
                  good={['answered']}
                  warn={['open']}
                />
                <Button size="sm" onclick={() => (openQuestionId = openQuestionId === q.id ? null : q.id)}>
                  {openQuestionId === q.id ? t('collab.hide') : t('collab.show')}
                </Button>
              </div>
              {#if openQuestionId === q.id}
                <QuestionCard {owner} {repo} questionId={q.id} {userId} onChanged={onQuestionChanged} />
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
      {#if questionsCursor}
        <Button size="sm" disabled={questionsLoading} onclick={() => loadQuestions(true)}>{t('collab.load_more')}</Button>
      {/if}
    </SectionCard>

    <SectionCard
      title={t('collab.decisions.title')}
      note={t('collab.decisions.note')}
      loading={decisionsLoading && decisions.length === 0}
      error={decisionsError}
      count={decisions.length}
    >
      {#if decisions.length === 0}
        <p class="muted">{t('collab.none')}</p>
      {:else}
        <ul class="list">
          {#each decisions as d (d.id)}
            <li>
              <div class="row">
                <span class="muted">#{d.sequence}</span>
                <strong class="grow text">{d.title}</strong>
                <StatusBadge value={d.status} prefix="collab.decision_status" known={KNOWN.decisionStatus} good={['active']} bad={['revoked']} warn={['superseded']} />
                <StatusBadge value={d.context_freshness} prefix="collab.freshness" known={KNOWN.freshness} good={['fresh']} warn={['stale']} />
              </div>
              <div class="body">{d.rationale}</div>
              <p class="muted">
                {t('collab.decisions.by', { kind: d.actor_kind, id: shortId(d.actor_id) })} · {formatTime(d.created_at)}
              </p>
              {#if d.change}
                <p class="muted text">{d.change.kind}: {d.change.reason}</p>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
      {#if decisionsCursor}
        <Button size="sm" disabled={decisionsLoading} onclick={() => loadDecisions(true)}>{t('collab.load_more')}</Button>
      {/if}
    </SectionCard>

    <SectionCard
      title={t('collab.handoffs.title')}
      note={t('collab.handoffs.note')}
      loading={handoffsLoading && handoffs.length === 0}
      error={handoffsError}
      count={handoffs.length}
    >
      {#if handoffs.length === 0}
        <p class="muted">{t('collab.none')}</p>
      {:else}
        <ul class="list">
          {#each handoffs as h (h.id)}
            <li>
              <div class="row">
                <span class="grow text">{sessionLabel(h.from_session_id)} → {sessionLabel(h.to_session_id)}</span>
                <StatusBadge value={h.status} prefix="collab.handoff_status" known={KNOWN.handoffStatus} good={['accepted']} warn={['pending']} />
                <span class="when">{formatTime(h.created_at)}</span>
              </div>
              <div class="body">{h.summary}</div>
              {#if h.completed}<p class="sec"><strong>{t('collab.handoffs.completed')}</strong></p><div class="body">{h.completed}</div>{/if}
              {#if h.next_steps}<p class="sec"><strong>{t('collab.handoffs.next_steps')}</strong></p><div class="body">{h.next_steps}</div>{/if}
              {#if h.blockers}<p class="sec"><strong>{t('collab.handoffs.blockers')}</strong></p><div class="body">{h.blockers}</div>{/if}
              {#if h.commit_sha}<p class="muted">commit <code>{shortId(h.commit_sha)}</code></p>{/if}
            </li>
          {/each}
        </ul>
      {/if}
      {#if handoffsCursor}
        <Button size="sm" disabled={handoffsLoading} onclick={() => loadHandoffs(true)}>{t('collab.load_more')}</Button>
      {/if}
    </SectionCard>

    <SectionCard
      title={t('collab.messages.title')}
      note={t('collab.messages.note')}
      loading={messagesLoading && messages.length === 0}
      error={messagesError}
      count={messages.length}
    >
      {#if messages.length === 0}
        <p class="muted">{t('collab.none')}</p>
      {:else}
        <ol class="timeline">
          {#each messages as m (m.id)}
            <li>
              <div class="row">
                <span class="muted">#{m.seq}</span>
                <StatusBadge value={m.kind} prefix="collab.message_kind" known={KNOWN.messageKind} warn={['help']} bad={['blocker']} />
                <span class="text">{sessionLabel(m.author_session_id)}</span>
                <span class="when">{formatTime(m.created_at)}</span>
              </div>
              <div class="body">{m.body}</div>
              {#if m.commit_sha}<p class="muted">commit <code>{shortId(m.commit_sha)}</code></p>{/if}
            </li>
          {/each}
        </ol>
      {/if}
      {#if messagesMore}
        <Button size="sm" disabled={messagesLoading} onclick={() => loadMessages(true)}>{t('collab.load_more')}</Button>
      {/if}
    </SectionCard>

    <SectionCard title={t('collab_relations.budget.title')} note={t('collab_relations.budget.note')}>
      <BudgetCard {owner} {repo} {taskId} />
    </SectionCard>

    <SectionCard title={t('collab.diag.title')} note={t('collab.diag.note')} error={diagnosisError} loading={diagnosisLoading}>
      {#if !diagnosis}
        <Button size="sm" onclick={runDiagnostics}>{t('collab.diag.run')}</Button>
      {:else}
        {#if diagnosis.findings.length === 0}
          <p class="muted">{t('collab.diag.none')}</p>
        {/if}
        <ul class="list">
          {#each diagnosis.findings as f}
            <li>
              <div class="row">
                <StatusBadge value={f.severity} prefix="collab.severity" known={KNOWN.severity} warn={['attention']} />
                <StatusBadge value={f.confidence} prefix="collab.confidence" known={KNOWN.confidence} good={['established']} />
                <code>{f.code}</code>
              </div>
              <p class="text">{f.statement}</p>
            </li>
          {/each}
        </ul>
        <p class="muted">{t('collab.diag.not_covered')}</p>
        <ul class="paths">
          {#each diagnosis.not_covered as n}<li class="muted text">{n}</li>{/each}
        </ul>
        <Button size="sm" onclick={runDiagnostics}>{t('collab.diag.rerun')}</Button>
      {/if}
    </SectionCard>

    <SectionCard title={t('collab.notices.title')} note={t('collab.notices.note')}>
      <p class="muted">{t('collab.notices.unavailable')}</p>
    </SectionCard>
  {/if}
</div>

<style>
  .back { margin: 0 0 8px; font-size: 13px; }
  .intro { margin: -8px 0 20px; color: var(--text-secondary); font-size: 13px; max-width: 80ch; }
  h1 { margin: 0; overflow-wrap: anywhere; }
  .head-chips { display: flex; align-items: center; gap: 8px; }
  .muted { color: var(--text-muted); font-size: 12px; margin: 2px 0; }
  .warn { color: var(--yellow); font-size: 12px; }
  .text { overflow-wrap: anywhere; min-width: 0; }
  .mono { font-family: var(--font-mono); font-size: 12px; }
  .when { color: var(--text-muted); font-size: 12px; white-space: nowrap; }
  .grow { flex: 1 1 200px; min-width: 0; }
  .row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .list { list-style: none; margin: 0 0 8px; padding: 0; }
  .list > li { padding: 8px 0; border-top: 1px solid var(--border-light); }
  .list > li:first-child { border-top: none; }
  .facts { display: grid; grid-template-columns: max-content 1fr; gap: 6px 16px; margin: 0; font-size: 13px; }
  .facts dt { color: var(--text-secondary); }
  .facts dd { margin: 0; min-width: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .paths { margin: 2px 0; padding-left: 16px; width: 100%; }
  .body { white-space: pre-wrap; overflow-wrap: anywhere; margin: 6px 0; font-size: 14px; }
  .sec { margin: 6px 0 0; font-size: 12px; }
  .timeline { list-style: none; margin: 0 0 8px; padding: 0; }
  .timeline > li { padding: 8px 0 8px 12px; border-left: 2px solid var(--border); margin-left: 4px; }
  code { font-family: var(--font-mono); font-size: 12px; }
  @media (max-width: 600px) { .facts { grid-template-columns: 1fr; } }
</style>
