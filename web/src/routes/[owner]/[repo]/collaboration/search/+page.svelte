<script lang="ts">
  // Collaboration search: a literal substring, one kind of record at a time. The
  // server answers live and only says WHERE a match is (kind, id, task): no
  // snippets, no total. The server filters by permission, so nothing is hidden
  // or counted client-side; "no results" means none that this account may see.
  import { page } from '$app/stores';
  import Button from '$lib/components/Button.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingSpinner from '$lib/components/LoadingSpinner.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { ApiError } from '$lib/api/_base.svelte';
  import { collabOps, SEARCH_KINDS, type SearchHit } from '$lib/api/collabOps';
  import { formatTime, shortId } from '$lib/utils/collaboration';
  import CollabNav from '$lib/components/collaboration/CollabNav.svelte';

  const t = createT();
  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  let kind = $state<string>('task');
  let q = $state('');
  // What the shown results were searched for (the inputs may have been edited since).
  let searched = $state<{ kind: string; q: string } | null>(null);
  let hits = $state<SearchHit[]>([]);
  let nextCursor = $state<string | null>(null);
  let queriedAt = $state<string | null>(null);
  let loading = $state(false);
  let moreLoading = $state(false);
  let error = $state('');
  let accessDenied = $state(false);

  const kindLabel = (k: string) => (SEARCH_KINDS.some((x) => x === k) ? t(`collab_ops.search.kind.${k}`) : k);

  /** Mirrors the server: 1..256 bytes, no LIKE metacharacters. */
  let problem = $derived.by(() => {
    const text = q.trim();
    if (!text) return null;
    if (/[%_\\\0]/.test(text)) return t('collab_ops.search.bad_chars');
    if (new TextEncoder().encode(text).length > 256) return t('collab_ops.search.too_long');
    return null;
  });

  async function run(append: boolean) {
    const target = append ? searched : { kind, q: q.trim() };
    if (!target || !target.q || (!append && problem)) return;
    if (append) moreLoading = true;
    else loading = true;
    error = '';
    try {
      const res = await collabOps.search(owner, repo, {
        kind: target.kind,
        q: target.q,
        limit: 25,
        cursor: append ? nextCursor : null,
      });
      hits = append ? [...hits, ...res.items] : res.items;
      nextCursor = res.next_cursor;
      queriedAt = res.queried_at;
      searched = target;
    } catch (e) {
      if (e instanceof ApiError && (e.status === 403 || e.status === 401)) accessDenied = true;
      else error = toErrorMessage(e, t('common.unknown_error'));
      if (!append) {
        hits = [];
        nextCursor = null;
        searched = null;
      }
    } finally {
      loading = false;
      moreLoading = false;
    }
  }

  function submit(e: SubmitEvent) {
    e.preventDefault();
    void run(false);
  }

  const taskHref = (id: string) => `/${owner}/${repo}/collaboration/tasks/${id}`;
</script>

<svelte:head>
  <title>{t('collab_ops.search.title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<CollabNav {owner} {repo} current="search" />

<div class="page-header">
  <h1>{t('collab_ops.search.title')}</h1>
</div>
<p class="intro">{t('collab_ops.search.intro')}</p>

{#if accessDenied}
  <EmptyState icon="🔒" title={t('collab.access_denied')} description={t('collab.access_denied_hint')} />
{:else}
  <section class="card surface">
    <form class="bar" onsubmit={submit} role="search">
      <label class="f">
        <span>{t('collab_ops.search.kind_label')}</span>
        <select bind:value={kind} data-testid="search-kind">
          {#each SEARCH_KINDS as k}
            <option value={k}>{t(`collab_ops.search.kind.${k}`)}</option>
          {/each}
        </select>
      </label>
      <label class="f grow">
        <span>{t('collab_ops.search.query_label')}</span>
        <input type="search" bind:value={q} maxlength="256" placeholder={t('collab_ops.search.placeholder')} data-testid="search-q" />
      </label>
      <Button type="submit" variant="primary" disabled={loading || !q.trim() || problem !== null}>{t('collab_ops.search.go')}</Button>
    </form>
    {#if problem}<p class="problem" role="status">{problem}</p>{/if}
    <p class="muted">{t('collab_ops.search.scope_note')}</p>

    {#if error}<ErrorBanner message={error} />{/if}

    {#if loading}
      <LoadingSpinner />
    {:else if searched}
      <h2 class="sub">
        {t('collab_ops.search.results_for', { kind: kindLabel(searched.kind) })}
        <span class="q text">“{searched.q}”</span>
      </h2>
      {#if hits.length === 0}
        <EmptyState title={t('collab_ops.search.none')} description={t('collab_ops.search.none_hint')} />
      {:else}
        <ul class="list" data-testid="search-results">
          {#each hits as hit (hit.kind + hit.id)}
            <li>
              <div class="row">
                <span class="kind">{kindLabel(hit.kind)}</span>
                <span class="mono" title={hit.id}>{shortId(hit.id)}</span>
                <span class="when">{formatTime(hit.created_at)}</span>
                <a href={taskHref(hit.task_id)}>{t('collab.open_task')} <span class="mono">{shortId(hit.task_id)}</span></a>
              </div>
            </li>
          {/each}
        </ul>
        <p class="muted">{t('collab_ops.search.no_snippets')}</p>
        {#if nextCursor}
          <div class="more">
            <Button disabled={moreLoading} onclick={() => run(true)}>
              {moreLoading ? t('common.loading') : t('collab.load_more')}
            </Button>
          </div>
        {/if}
      {/if}
      {#if queriedAt}<p class="muted">{t('collab_ops.search.live_note', { time: formatTime(queriedAt) })}</p>{/if}
    {/if}
  </section>
{/if}

<style>
  .intro { margin: -8px 0 16px; color: var(--text-secondary); font-size: 13px; max-width: 80ch; }
  .card { padding: 16px 20px; margin-bottom: 20px; min-width: 0; }
  .bar { display: flex; gap: 12px; align-items: flex-end; flex-wrap: wrap; }
  .f { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--text-secondary); }
  .f.grow { flex: 1 1 240px; min-width: 0; }
  .f input { width: 100%; }
  .muted { color: var(--text-muted); font-size: 12px; margin: 8px 0; }
  .problem { color: var(--red); font-size: 12px; margin: 6px 0 0; }
  .sub { font-size: 14px; margin: 16px 0 8px; }
  .q { font-weight: 400; color: var(--text-secondary); }
  .text { overflow-wrap: anywhere; }
  .list { list-style: none; margin: 0; padding: 0; }
  .list > li { padding: 8px 0; border-top: 1px solid var(--border-light); }
  .list > li:first-child { border-top: none; }
  .row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .kind { padding: 0 8px; border-radius: 999px; font-size: 12px; background: var(--bg-tertiary); color: var(--text-secondary); font-weight: 600; }
  .mono { font-family: var(--font-mono); font-size: 12px; }
  .when { color: var(--text-muted); font-size: 12px; }
  .more { margin-top: 12px; text-align: center; }
</style>
