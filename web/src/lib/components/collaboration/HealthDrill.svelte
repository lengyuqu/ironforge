<script lang="ts">
  // The tasks behind one health figure, with the numbers each contributes.
  // Ordered by task id (keyset paging), NOT by severity or duration: this is a
  // drill-down, not a ranking. Titles are client-written; statuses are
  // declarations.
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import Button from '$lib/components/Button.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingSpinner from '$lib/components/LoadingSpinner.svelte';
  import StatusBadge from './StatusBadge.svelte';
  import { collabHealth, type HealthDrillItem } from '$lib/api/collabHealth';
  import { KNOWN, formatTime } from '$lib/utils/collaboration';
  import { formatDuration, isSecondsKey } from '$lib/utils/collabHealth';

  interface Props {
    owner: string;
    repo: string;
    metric: string;
    /** The window of the health figure being drilled into. */
    from: string;
    to: string;
  }

  let { owner, repo, metric, from, to }: Props = $props();
  const t = createT();

  let items = $state<HealthDrillItem[]>([]);
  let nextCursor = $state<string | null>(null);
  let loading = $state(true);
  let moreLoading = $state(false);
  let error = $state('');

  async function load(append: boolean) {
    if (append) moreLoading = true;
    else loading = true;
    error = '';
    try {
      const res = await collabHealth.drill(owner, repo, {
        metric,
        from,
        to,
        limit: 25,
        cursor: append ? nextCursor : null,
      });
      items = append ? [...items, ...res.items] : res.items;
      nextCursor = res.next_cursor;
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      loading = false;
      moreLoading = false;
    }
  }

  $effect(() => {
    void metric;
    void from;
    void to;
    void load(false);
  });

  const taskHref = (id: string) => `/${owner}/${repo}/collaboration/tasks/${id}`;

  function basisText(basis: Record<string, unknown>): string {
    return Object.entries(basis ?? {})
      .map(([k, v]) => {
        if (typeof v === 'number' && isSecondsKey(k)) return `${k}: ${formatDuration(v)}`;
        if (typeof v === 'string' && /^\d{4}-\d\d-\d\dT/.test(v)) return `${k}: ${formatTime(v)}`;
        return `${k}: ${typeof v === 'object' ? JSON.stringify(v) : String(v)}`;
      })
      .join(' · ');
  }
</script>

<div data-testid="health-drill">
  {#if error}<ErrorBanner message={error} />{/if}
  {#if loading}
    <LoadingSpinner />
  {:else if items.length === 0}
    <p class="muted" data-testid="drill-empty">{t('collab_health.drill.empty')}</p>
  {:else}
    <ul class="list">
      {#each items as item (item.task_id)}
        <li data-testid="drill-item">
          <div class="row">
            <a class="text grow" href={taskHref(item.task_id)}>{item.title}</a>
            <StatusBadge
              value={item.status}
              prefix="collab.task_status"
              known={KNOWN.taskStatus}
              good={['done']}
              warn={['waiting', 'blocked']}
              title={t('collab.declared_hint')}
            />
          </div>
          <p class="muted mono text">{basisText(item.basis)}</p>
        </li>
      {/each}
    </ul>
    {#if nextCursor}
      <div class="more">
        <Button disabled={moreLoading} onclick={() => load(true)}>
          {moreLoading ? t('common.loading') : t('collab.load_more')}
        </Button>
      </div>
    {/if}
  {/if}
</div>

<style>
  .muted { color: var(--text-muted); font-size: 12px; margin: 4px 0; }
  .list { list-style: none; margin: 0; padding: 0; }
  .list > li { padding: 8px 0; border-top: 1px solid var(--border-light); }
  .list > li:first-child { border-top: none; }
  .row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .grow { flex: 1 1 200px; min-width: 0; }
  .text { overflow-wrap: anywhere; min-width: 0; }
  .mono { font-family: var(--font-mono); }
  .more { margin-top: 12px; text-align: center; }
</style>
