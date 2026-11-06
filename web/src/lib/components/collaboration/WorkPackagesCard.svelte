<script lang="ts">
  // Work packages of a task: numbered, immutable hand-over summaries. Read only.
  //
  // Two kinds of information are kept apart on purpose:
  //  - DECLARED: everything the author wrote (goal, "completed", evidence
  //    references…). A claim, shown as text. "Tests pass" in here is a claim.
  //  - VERIFIED: only what the platform itself looked up — whether a referenced
  //    CI pipeline exists in this repository and what status/commit it has.
  //    Commit, URL and note references are never checked.
  // Neither block is derived from the other.
  import { untrack } from 'svelte';
  import { collabDecisions, type WorkPackage } from '$lib/api/collabDecisions';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingSpinner from '$lib/components/LoadingSpinner.svelte';
  import Button from '$lib/components/Button.svelte';
  import StatusBadge from './StatusBadge.svelte';
  import { formatTime, knownLabelKey, shortId } from '$lib/utils/collaboration';
  import {
    KNOWN_EVIDENCE_KIND,
    KNOWN_MISSING,
    KNOWN_PIPELINE_STATUS,
    KNOWN_VERIFICATION,
    commitDiffers,
  } from '$lib/utils/collabDecisions';

  interface Props {
    owner: string;
    repo: string;
    taskId: string;
  }

  let { owner, repo, taskId }: Props = $props();
  const t = createT();
  const msg = (e: unknown) => toErrorMessage(e, t('common.unknown_error'));

  let items = $state<WorkPackage[]>([]);
  let cursor = $state<string | null>(null);
  let loading = $state(true);
  let error = $state('');

  let openId = $state<string | null>(null);
  let detail = $state<WorkPackage | null>(null);
  let detailLoading = $state(false);
  let detailError = $state('');

  async function load(append = false) {
    loading = true;
    error = '';
    try {
      const res = await collabDecisions.listWorkPackages(owner, repo, taskId, {
        limit: 25,
        cursor: append ? cursor : null,
      });
      items = append ? [...items, ...res.items] : res.items;
      cursor = res.next_cursor;
    } catch (e) {
      error = msg(e);
    } finally {
      loading = false;
    }
  }

  async function toggle(id: string) {
    if (openId === id) {
      openId = null;
      detail = null;
      return;
    }
    openId = id;
    detail = null;
    detailError = '';
    detailLoading = true;
    try {
      const got = await collabDecisions.getWorkPackage(owner, repo, id);
      if (openId === id) detail = got;
    } catch (e) {
      if (openId === id) detailError = msg(e);
    } finally {
      if (openId === id) detailLoading = false;
    }
  }

  $effect(() => {
    void owner;
    void repo;
    void taskId;
    openId = null;
    detail = null;
    untrack(() => void load());
  });

  const label = (prefix: string, value: string, known: readonly string[]) => {
    const key = knownLabelKey(prefix, value, known);
    return key ? t(key) : value;
  };
</script>

<div data-testid="work-packages">
  {#if error}<ErrorBanner message={error}><Button size="sm" onclick={() => load()}>{t('common.retry')}</Button></ErrorBanner>{/if}
  {#if loading && items.length === 0}
    <LoadingSpinner />
  {:else if items.length === 0 && !error}
    <p class="muted">{t('collab_decisions.wp.none')}</p>
  {:else}
    <ul class="list">
      {#each items as p (p.id)}
        <li data-testid="work-package">
          <div class="row">
            <strong>{t('collab_decisions.wp.revision', { n: p.revision })}</strong>
            <span class="muted">
              {t('collab_decisions.wp.author', { kind: p.author_kind, id: shortId(p.author_id) })} · {formatTime(p.created_at)}
            </span>
            {#if p.commit_sha}<code title={t('collab_decisions.wp.declared_commit')}>{shortId(p.commit_sha)}</code>{/if}
            <span class="grow"></span>
            <Button size="sm" onclick={() => toggle(p.id)}>
              {openId === p.id ? t('collab.hide') : t('collab.show')}
            </Button>
          </div>
          {#if p.goal}<p class="text goal">{p.goal}</p>{/if}
          {#if p.missing_fields.length > 0}
            <p class="muted">
              {t('collab_decisions.wp.left_out')}:
              {p.missing_fields.map((f) => label('collab_decisions.wp.missing', f, KNOWN_MISSING)).join(', ')}
            </p>
          {/if}

          {#if openId === p.id}
            {#if detailLoading}
              <LoadingSpinner />
            {:else if detailError}
              <ErrorBanner message={detailError} />
            {:else if detail}
              <div class="declared" data-testid="wp-declared">
                <h4>{t('collab_decisions.wp.declared_title')}</h4>
                <p class="muted">{t('collab_decisions.wp.declared_note')}</p>
                <dl>
                  <dt>{t('collab_decisions.wp.goal')}</dt>
                  <dd class="body">{detail.goal || '—'}</dd>
                  <dt>{t('collab_decisions.wp.scope')}</dt>
                  <dd class="body">{detail.scope || '—'}</dd>
                  {#each [['artifacts', detail.artifacts], ['completed', detail.completed], ['open_items', detail.open_items], ['next_steps', detail.next_steps]] as [field, list] (field)}
                    <dt>{t(`collab_decisions.wp.${field as string}`)}</dt>
                    <dd>
                      {#if (list as string[]).length === 0}—{:else}
                        <ul class="items">
                          {#each list as string[] as item}<li class="text">{item}</li>{/each}
                        </ul>
                      {/if}
                    </dd>
                  {/each}
                  <dt>{t('collab_decisions.wp.refs')}</dt>
                  <dd>
                    {#if detail.evidence_refs.length === 0}—{:else}
                      <ul class="items">
                        {#each detail.evidence_refs as r}
                          <li class="text">
                            <span class="muted">{label('collab_decisions.wp.ref_kind', r.kind, KNOWN_EVIDENCE_KIND)}</span>
                            <code>{r.reference}</code>
                          </li>
                        {/each}
                      </ul>
                    {/if}
                  </dd>
                  <dt>{t('collab_decisions.wp.task_version')}</dt>
                  <dd>{detail.task_version}</dd>
                </dl>
              </div>

              <div class="verified" data-testid="wp-verified">
                <h4>{t('collab_decisions.wp.verified_title')}</h4>
                <p class="muted">{t('collab_decisions.wp.verified_note')}</p>
                {#if !detail.evidence_resolution || detail.evidence_resolution.length === 0}
                  <p class="muted">{t('collab_decisions.wp.no_pipeline_refs')}</p>
                {:else}
                  <ul class="items">
                    {#each detail.evidence_resolution as ev (ev.pipeline_id)}
                      <li>
                        <div class="row">
                          <strong>{t('collab.completion.pipeline', { id: ev.pipeline_id })}</strong>
                          <StatusBadge
                            value={ev.verification}
                            prefix="collab_decisions.wp.verification"
                            known={KNOWN_VERIFICATION}
                            good={['verified_reference']}
                            warn={['unavailable']}
                          />
                          {#if ev.status}
                            <StatusBadge
                              value={ev.status}
                              prefix="collab_decisions.wp.pipeline_status"
                              known={KNOWN_PIPELINE_STATUS}
                              good={['success']}
                              bad={['failed']}
                              warn={['pending', 'running']}
                            />
                          {/if}
                        </div>
                        {#if ev.verification === 'unavailable'}
                          <p class="muted">{t('collab_decisions.wp.unavailable_note')}</p>
                        {:else}
                          <p class="muted mono">
                            {#if ev.ref_name}{ev.ref_name}{/if}
                            {#if ev.commit_sha}
                              · {t('collab_decisions.wp.ran_on')} <code>{shortId(ev.commit_sha)}</code>
                            {/if}
                            {#if ev.finished_at}· {formatTime(ev.finished_at)}{/if}
                          </p>
                          {#if commitDiffers(detail, ev)}
                            <p class="warn">
                              {t('collab_decisions.wp.commit_differs', {
                                declared: shortId(detail.commit_sha),
                                ran: shortId(ev.commit_sha),
                              })}
                            </p>
                          {/if}
                        {/if}
                        <p class="muted">{t('collab_decisions.wp.looked_up', { time: formatTime(ev.resolved_at) })}</p>
                      </li>
                    {/each}
                  </ul>
                {/if}
              </div>
            {/if}
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#if cursor}
    <Button size="sm" disabled={loading} onclick={() => load(true)}>{t('collab.load_more')}</Button>
  {/if}
</div>

<style>
  .list { list-style: none; margin: 0 0 8px; padding: 0; }
  .list > li { padding: 8px 0; border-top: 1px solid var(--border-light); }
  .list > li:first-child { border-top: none; }
  .row { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .grow { flex: 1 1 20px; }
  .muted { color: var(--text-muted); font-size: 12px; margin: 2px 0; }
  .warn { color: var(--yellow); font-size: 12px; margin: 4px 0; }
  .text { overflow-wrap: anywhere; min-width: 0; }
  .goal { margin: 4px 0; font-size: 14px; }
  .body { white-space: pre-wrap; overflow-wrap: anywhere; }
  .mono, code { font-family: var(--font-mono); font-size: 12px; }
  h4 { margin: 0 0 2px; font-size: 13px; }
  .declared, .verified { margin-top: 8px; padding: 8px 12px; border-radius: var(--radius); }
  .declared { border: 1px dashed var(--border); background: var(--bg-primary); }
  .verified { border: 1px solid var(--green-dim); background: var(--bg-primary); }
  dl { display: grid; grid-template-columns: fit-content(55%) minmax(0, 1fr); overflow-wrap: anywhere; gap: 4px 14px; margin: 6px 0 0; font-size: 13px; }
  dt { color: var(--text-secondary); }
  dd { margin: 0; min-width: 0; }
  .items { margin: 0; padding-left: 16px; list-style: disc; }
  .verified .items { list-style: none; padding-left: 0; }
  .verified .items > li { padding: 4px 0; }
  @media (max-width: 600px) { dl { grid-template-columns: 1fr; } }
</style>
