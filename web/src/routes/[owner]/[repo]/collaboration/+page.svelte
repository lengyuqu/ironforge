<script lang="ts">
  // Collaboration home: what waits for the signed-in person, an overview of
  // stuck / in-flight work, and the task list. All titles are client-written
  // text; statuses are declarations by whoever set them.
  import { page } from '$app/state';
  import Button from '$lib/components/Button.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { createLatest } from '$lib/utils/latest';
  import { ApiError } from '$lib/api/_base.svelte';
  import {
    collaboration,
    loadSessionMap,
    TASK_STATUSES,
    type CollabSession,
    type CollabTask,
    type Inbox,
    type Overview,
  } from '$lib/api/collaboration';
  import { getUser } from '$lib/stores/auth.svelte';
  import { KNOWN, formatTime, shortId } from '$lib/utils/collaboration';
  import SectionCard from '$lib/components/collaboration/SectionCard.svelte';
  import StatusBadge from '$lib/components/collaboration/StatusBadge.svelte';
  import QuestionCard from '$lib/components/collaboration/QuestionCard.svelte';
  import CollabNav from '$lib/components/collaboration/CollabNav.svelte';

  const t = createT();

  let owner = $derived(page.params.owner!);
  let repo = $derived(page.params.repo!);
  let userId = $derived(getUser()?.id ?? null);

  // ── Inbox (questions waiting for this person) ──
  let inbox = $state<Inbox | null>(null);
  let inboxLoading = $state(true);
  let inboxError = $state('');
  let inboxForbidden = $state(false);
  let openQuestionId = $state<string | null>(null);

  // ── Overview ──
  let overview = $state<Overview | null>(null);
  let overviewLoading = $state(true);
  let overviewError = $state('');

  // ── Tasks ──
  let tasks = $state<CollabTask[]>([]);
  let nextCursor = $state<string | null>(null);
  let tasksLoading = $state(true);
  let moreLoading = $state(false);
  let tasksError = $state('');
  let statusFilter = $state('');
  let sessions = $state<Map<string, CollabSession> | null>(null);
  let accessDenied = $state(false);

  const sessionsLatest = createLatest();
  const inboxLatest = createLatest();
  const overviewLatest = createLatest();
  const tasksLatest = createLatest();

  async function loadInbox() {
    const isCurrent = inboxLatest.begin();
    inboxLoading = true;
    inboxError = '';
    inboxForbidden = false;
    try {
      const res = await collaboration.userInbox(owner, repo, { limit: 50 });
      if (!isCurrent()) return;
      inbox = res;
    } catch (e) {
      if (!isCurrent()) return;
      inbox = null;
      if (e instanceof ApiError && e.status === 403) inboxForbidden = true;
      else inboxError = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      if (isCurrent()) inboxLoading = false;
    }
  }

  async function loadOverview() {
    const isCurrent = overviewLatest.begin();
    overviewLoading = true;
    overviewError = '';
    try {
      const res = await collaboration.overview(owner, repo, {
        sections: ['blocked_waiting', 'pending_handoffs', 'in_progress'],
        limit: 20,
      });
      if (!isCurrent()) return;
      overview = res;
    } catch (e) {
      if (!isCurrent()) return;
      overviewError = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      if (isCurrent()) overviewLoading = false;
    }
  }

  async function loadTasks(append = false) {
    const isCurrent = tasksLatest.begin();
    if (append) moreLoading = true;
    else tasksLoading = true;
    tasksError = '';
    try {
      const res = await collaboration.listTasks(owner, repo, {
        status: statusFilter || undefined,
        limit: 25,
        cursor: append ? nextCursor : null,
      });
      if (!isCurrent()) return;
      tasks = append ? [...tasks, ...res.items] : res.items;
      nextCursor = res.next_cursor;
    } catch (e) {
      if (!isCurrent()) return;
      if (e instanceof ApiError && (e.status === 403 || e.status === 401)) accessDenied = true;
      else tasksError = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      if (isCurrent()) {
        tasksLoading = false;
        moreLoading = false;
      }
    }
  }

  async function loadAll() {
    accessDenied = false;
    const isCurrentSessions = sessionsLatest.begin();
    // Sessions are optional context: an error must not break the page.
    loadSessionMap(owner, repo)
      .then((m) => {
        if (isCurrentSessions()) sessions = m;
      })
      .catch(() => {
        if (isCurrentSessions()) sessions = null;
      });
    await Promise.all([loadTasks(false), loadInbox(), loadOverview()]);
  }

  $effect(() => {
    void owner;
    void repo;
    loadAll();
  });

  function onFilterChange() {
    nextCursor = null;
    loadTasks(false);
  }

  function ownerLabel(id: string | null): string {
    if (!id) return t('collab.unowned');
    return sessions?.get(id)?.display_name || shortId(id);
  }

  /** Which enum an overview item's `state` belongs to, by the kind of record it describes. */
  function stateKind(objectType: string): { prefix: string; known: readonly string[] } {
    if (objectType === 'pending_handoff') return { prefix: 'collab.handoff_status', known: KNOWN.handoffStatus };
    if (objectType === 'open_question') return { prefix: 'collab.question_status', known: KNOWN.questionStatus };
    return { prefix: 'collab.task_status', known: KNOWN.taskStatus };
  }

  function taskHref(id: string) {
    return `/${owner}/${repo}/collaboration/tasks/${id}`;
  }

  let hiddenSections = $derived(overview?.sections.filter((s) => !s.available) ?? []);
</script>

<svelte:head>
  <title>{t('collab.title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<CollabNav {owner} {repo} current="home" />

<div class="page-header">
  <h1>{t('collab.title')}</h1>
</div>
<p class="intro">{t('collab.intro')}</p>

{#if accessDenied}
  <EmptyState icon="🔒" title={t('collab.access_denied')} description={t('collab.access_denied_hint')} />
{:else}
  <SectionCard
    title={t('collab.inbox.title')}
    note={t('collab.inbox.note')}
    loading={inboxLoading}
    error={inboxError}
    count={inbox ? inbox.items.length : null}
  >
    {#if inboxForbidden}
      <p class="muted">{t('collab.inbox.write_needed')}</p>
    {:else if !inbox || inbox.items.length === 0}
      <p class="muted">{t('collab.inbox.empty')}</p>
    {:else}
      <ul class="list">
        {#each inbox.items as item (item.item_key)}
          <li>
            <div class="row">
              <StatusBadge
                value={item.type}
                prefix="collab.inbox_type"
                known={KNOWN.inboxType}
                warn={['open_question']}
              />
              <span class="grow text">{item.title}</span>
              <span class="when">{formatTime(item.occurred_at)}</span>
              <a href={taskHref(item.task_id)}>{t('collab.open_task')}</a>
              {#if item.type === 'open_question' || item.type === 'question_answer'}
                <Button
                  size="sm"
                  onclick={() => (openQuestionId = openQuestionId === item.object_id ? null : item.object_id)}
                >
                  {openQuestionId === item.object_id ? t('collab.hide') : t('collab.inbox.review')}
                </Button>
              {/if}
            </div>
            {#if openQuestionId === item.object_id}
              <QuestionCard {owner} {repo} questionId={item.object_id} {userId} onChanged={loadInbox} />
            {/if}
          </li>
        {/each}
      </ul>
      {#if inbox.truncated}<p class="muted">{t('collab.truncated')}</p>{/if}
    {/if}
  </SectionCard>

  <SectionCard
    title={t('collab.overview.title')}
    note={t('collab.overview.note')}
    loading={overviewLoading}
    error={overviewError}
  >
    {#if overview}
      <div class="grid">
        {#each overview.sections.filter((s) => s.available) as section (section.name)}
          <div class="ov">
            <h3>
              {t(`collab.section.${section.name}`)}
              <span class="badge-count">{section.items.length}{section.truncated ? '+' : ''}</span>
            </h3>
            {#if section.items.length === 0}
              <p class="muted">{t('collab.none')}</p>
            {:else}
              <ul class="list tight">
                {#each section.items as item (item.object_type + item.object_id)}
                  <li>
                    <a href={taskHref(item.task_id)} class="text">{item.title}</a>
                    <div class="sub">
                      <StatusBadge value={item.state} {...stateKind(item.object_type)} />
                      {#each item.flags as flag}
                        <StatusBadge value={flag} prefix="collab.flag" known={KNOWN.flag} warn={['unowned', 'session_not_online']} />
                      {/each}
                    </div>
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
        {/each}
      </div>
      {#each hiddenSections as s}
        <p class="muted">{t(`collab.section.${s.name}`)}: {s.reason ?? t('collab.unavailable')}</p>
      {/each}
    {/if}
  </SectionCard>

  <SectionCard
    title={t('collab.tasks.title')}
    note={t('collab.tasks.note')}
    loading={tasksLoading}
    error={tasksError}
    count={tasksLoading ? null : tasks.length}
  >
    {#snippet actions()}
      <label class="filter">
        <span>{t('collab.tasks.filter_status')}</span>
        <select bind:value={statusFilter} onchange={onFilterChange}>
          <option value="">{t('collab.tasks.all')}</option>
          {#each TASK_STATUSES as s}
            <option value={s}>{t(`collab.task_status.${s}`)}</option>
          {/each}
        </select>
      </label>
    {/snippet}
    {#if tasks.length === 0}
      <EmptyState title={t('collab.tasks.empty')} description={t('collab.tasks.empty_hint')} />
    {:else}
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th>{t('collab.field.title')}</th>
              <th>{t('collab.field.status_declared')}</th>
              <th>{t('collab.field.owner_session')}</th>
              <th>{t('collab.field.branch')}</th>
              <th>{t('collab.field.updated')}</th>
            </tr>
          </thead>
          <tbody>
            {#each tasks as task (task.id)}
              <tr>
                <td class="text"><a href={taskHref(task.id)}>{task.title}</a></td>
                <td>
                  <StatusBadge
                    value={task.status}
                    prefix="collab.task_status"
                    known={KNOWN.taskStatus}
                    good={['done']}
                    warn={['waiting', 'blocked']}
                    title={t('collab.declared_hint')}
                  />
                </td>
                <td>
                  <span class="text">{ownerLabel(task.owner_session_id)}</span>
                  {#if task.owner_session_id && sessions?.get(task.owner_session_id)}
                    <StatusBadge
                      value={sessions.get(task.owner_session_id)!.state}
                      prefix="collab.session_state"
                      known={KNOWN.sessionState}
                      good={['online']}
                    />
                  {/if}
                </td>
                <td class="mono text">{task.branch ?? '—'}</td>
                <td class="when">{formatTime(task.updated_at)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {#if nextCursor}
        <div class="more">
          <Button disabled={moreLoading} onclick={() => loadTasks(true)}>
            {moreLoading ? t('common.loading') : t('collab.load_more')}
          </Button>
        </div>
      {/if}
    {/if}
    {#if sessions === null}<p class="muted">{t('collab.sessions_unavailable')}</p>{/if}
  </SectionCard>
{/if}

<style>
  .intro { margin: -8px 0 20px; color: var(--text-secondary); font-size: 13px; max-width: 80ch; }
  .muted { color: var(--text-muted); font-size: 13px; margin: 4px 0; }
  .list { list-style: none; margin: 0; padding: 0; }
  .list > li { padding: 8px 0; border-top: 1px solid var(--border-light); }
  .list > li:first-child { border-top: none; }
  .list.tight > li { padding: 6px 0; }
  .row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .grow { flex: 1 1 200px; min-width: 0; }
  .text { overflow-wrap: anywhere; min-width: 0; }
  .when { color: var(--text-muted); font-size: 12px; white-space: nowrap; }
  .sub { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 4px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: 20px; }
  .ov h3 { margin: 0 0 6px; font-size: 14px; }
  .badge-count { margin-left: 6px; font-size: 12px; color: var(--text-muted); }
  .filter { display: flex; align-items: center; gap: 6px; font-size: 13px; color: var(--text-secondary); }
  .table-wrap { overflow-x: auto; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th { text-align: left; font-weight: 600; color: var(--text-secondary); padding: 6px 8px; border-bottom: 1px solid var(--border); white-space: nowrap; }
  td { padding: 8px; border-bottom: 1px solid var(--border-light); vertical-align: top; }
  td.mono { font-family: var(--font-mono); font-size: 12px; }
  .more { margin-top: 12px; text-align: center; }
</style>
