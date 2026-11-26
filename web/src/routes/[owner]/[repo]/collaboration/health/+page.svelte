<script lang="ts">
  // Collaboration health of this repository (repository admins only).
  //
  // What the numbers are, and are not:
  //  * They come from a platform event log that only starts at `coverage_start`.
  //    A value for time before that is partial (a lower bound, shown "≥ n") or
  //    unknown (shown as "unknown"); it is NEVER shown as 0.
  //  * Task status is whatever its client declared; only the time of each
  //    change is recorded by the platform. Both are labelled.
  //  * There is deliberately no ranking and no per-session / per-agent view.
  import { page } from '$app/stores';
  import CollabNav from '$lib/components/collaboration/CollabNav.svelte';
  import SectionCard from '$lib/components/collaboration/SectionCard.svelte';
  import StatusBadge from '$lib/components/collaboration/StatusBadge.svelte';
  import HealthDrill from '$lib/components/collaboration/HealthDrill.svelte';
  import Button from '$lib/components/Button.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import { confirmDialog } from '$lib/components/confirm.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { ApiError } from '$lib/api/_base.svelte';
  import { collabHealth, type BaselineReport, type Health, type HealthMetric } from '$lib/api/collabHealth';
  import { formatTime, knownLabelKey } from '$lib/utils/collaboration';
  import { KNOWN_HEALTH, NOTE_KEYS, WINDOW_DAYS, formatDuration, valueRows, windowFrom } from '$lib/utils/collabHealth';

  const t = createT();
  const msg = (e: unknown) => toErrorMessage(e, t('common.unknown_error'));

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  let days = $state<number>(7);
  let health = $state<Health | null>(null);
  let loading = $state(true);
  let error = $state('');
  let adminsOnly = $state(false);
  let signedOut = $state(false);
  let drillMetric = $state<string | null>(null);

  let baselineBusy = $state(false);
  let baselineError = $state('');
  let baselineReport = $state<BaselineReport | null>(null);

  async function load() {
    loading = true;
    error = '';
    adminsOnly = false;
    signedOut = false;
    drillMetric = null;
    try {
      health = await collabHealth.get(owner, repo, { from: windowFrom(days) });
    } catch (e) {
      health = null;
      if (e instanceof ApiError && e.status === 403) adminsOnly = true;
      else if (e instanceof ApiError && e.status === 401) signedOut = true;
      else error = msg(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void owner;
    void repo;
    baselineReport = null;
  });

  // Reload when the repository or the chosen window changes.
  $effect(() => {
    void owner;
    void repo;
    void days;
    void load();
  });

  async function takeBaseline() {
    if (baselineBusy) return;
    const ok = await confirmDialog({
      title: t('collab_health.baseline.confirm_title'),
      message: t('collab_health.baseline.confirm_message'),
      confirmLabel: t('collab_health.baseline.confirm_button'),
      variant: 'danger',
    });
    if (!ok) return;
    baselineBusy = true;
    baselineError = '';
    try {
      baselineReport = await collabHealth.takeBaseline(owner, repo);
      await load();
    } catch (e) {
      baselineError = msg(e);
    } finally {
      baselineBusy = false;
    }
  }

  const label = (prefix: string, value: string, known: readonly string[]) => {
    const key = knownLabelKey(prefix, value, known);
    return key ? t(key) : value;
  };

  function renderRow(metric: HealthMetric, row: ReturnType<typeof valueRows>[number]): string {
    // A partial metric is a lower bound; say so on every number. A partial
    // zero says nothing about the time before coverage, so it is not shown as 0.
    const partial = metric.status === 'partial';
    const bound = partial ? '≥ ' : '';
    if (partial && (row.kind === 'seconds' || row.kind === 'number') && row.value === 0) {
      return t('collab_health.none_recorded');
    }
    if (row.kind === 'seconds') return `${bound}${formatDuration(row.value as number)}`;
    if (row.kind === 'number') return `${bound}${row.value}`;
    if (row.kind === 'time') return formatTime(String(row.value));
    return String(row.value);
  }

  /** A server note in the UI language when it is a known sentence, else as sent. */
  const noteText = (note: string) => {
    const id = NOTE_KEYS[note];
    return id ? t(`collab_health.note.${id}`) : note;
  };

  const rowLabel = (key: string) => {
    const k = `collab_health.value.${key}`;
    const text = t(k);
    return text === k ? key : text;
  };

  let qualityRows = $derived(
    health
      ? Object.entries(health.data_quality).filter(([, n]) => typeof n === 'number' && n > 0)
      : [],
  );
</script>

<svelte:head>
  <title>{t('collab_health.title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<CollabNav {owner} {repo} current="health" />

<div class="page-header"><h1>{t('collab_health.title')}</h1></div>
<p class="intro">{t('collab_health.intro')}</p>

{#if adminsOnly}
  <EmptyState icon="🔒" title={t('collab_health.admins_only')} description={t('collab_health.admins_only_hint')} />
{:else if signedOut}
  <EmptyState icon="🔒" title={t('collab_health.signed_out')} description={t('collab_health.signed_out_hint')} />
{:else}
  <div class="toolbar">
    <label class="filter">
      <span>{t('collab_health.window.label')}</span>
      <select value={days} onchange={(e) => (days = Number(e.currentTarget.value))} data-testid="window-select">
        {#each WINDOW_DAYS as d}
          <option value={d}>{t('collab_health.window.days', { days: d })}</option>
        {/each}
      </select>
    </label>
  </div>
  {#if error}<ErrorBanner message={error} />{/if}

  <SectionCard
    title={t('collab_health.coverage.title')}
    note={t('collab_health.coverage.note')}
    {loading}
  >
    {#if health}
      <dl class="facts" data-testid="coverage">
        <dt>{t('collab_health.coverage.range')}</dt>
        <dd>{formatTime(health.range.from)} → {formatTime(health.range.to)} ({health.range.timezone}, {health.range.interval})</dd>
        <dt>{t('collab_health.coverage.start')}</dt>
        <dd>
          {#if health.coverage_start}{formatTime(health.coverage_start)}{:else}<em>{t('collab_health.coverage.none')}</em>{/if}
        </dd>
        <dt>{t('collab_health.coverage.baseline')}</dt>
        <dd>
          {#if health.baseline.taken_at}{formatTime(health.baseline.taken_at)}{:else}<em>{t('collab_health.coverage.baseline_none')}</em>{/if}
        </dd>
        <dt>{t('collab_health.coverage.computed')}</dt>
        <dd>{formatTime(health.computed_at)} · {t('collab_health.coverage.version', { version: health.metric_version })}</dd>
      </dl>
      {#if health.partial}
        <p class="banner" role="status" data-testid="partial-banner">{t('collab_health.coverage.partial_banner')}</p>
      {/if}
    {/if}
  </SectionCard>

  {#if health}
    <SectionCard title={t('collab_health.metrics.title')} note={t('collab_health.metrics.note')}>
      <div class="grid">
        {#each health.metrics as metric (metric.id)}
          <article class="metric" data-testid="metric" data-metric={metric.id}>
            <h3>{label('collab_health.metric', metric.id, KNOWN_HEALTH.metric)}</h3>
            <div class="chips">
              <StatusBadge
                value={metric.status}
                prefix="collab_health.status"
                known={KNOWN_HEALTH.status}
                good={['complete']}
                warn={['partial', 'unknown']}
              />
              <StatusBadge
                value={metric.source}
                prefix="collab_health.source"
                known={KNOWN_HEALTH.source}
                title={t('collab_health.source_hint')}
              />
            </div>
            {#if metric.status === 'unknown' || !metric.value}
              <p class="unknown" data-testid="metric-unknown">
                {t('collab_health.unknown')}
                {#if metric.reason}
                  — {label('collab_health.reason', metric.reason, KNOWN_HEALTH.reason)}
                {/if}
              </p>
            {:else}
              {#if metric.status === 'partial'}<p class="muted">{t('collab_health.lower_bound')}</p>{/if}
              <dl class="values">
                {#each valueRows(metric.value) as row (row.key)}
                  <dt>{rowLabel(row.key)}</dt>
                  <dd data-testid="value-{row.key}">{renderRow(metric, row)}</dd>
                {/each}
              </dl>
            {/if}
            {#each metric.notes as note}<p class="muted">{noteText(note)}</p>{/each}
            {#if metric.drill_down.length > 0}
              <div class="drills">
                {#each metric.drill_down as d (d.metric)}
                  <Button size="sm" onclick={() => (drillMetric = d.metric)}>
                    {label('collab_health.drill_metric', d.metric, KNOWN_HEALTH.drill)}
                  </Button>
                {/each}
              </div>
            {/if}
          </article>
        {/each}
      </div>
      <p class="muted">{t('collab_health.no_ranking')}</p>
    </SectionCard>

    {#if drillMetric}
      <SectionCard
        title={t('collab_health.drill.title', { metric: label('collab_health.drill_metric', drillMetric, KNOWN_HEALTH.drill) })}
        note={t('collab_health.drill.note')}
      >
        {#key drillMetric + health.computed_at}
          <HealthDrill {owner} {repo} metric={drillMetric} from={health.range.from} to={health.range.to} />
        {/key}
        <div class="more"><Button size="sm" variant="ghost" onclick={() => (drillMetric = null)}>{t('collab_health.drill.close')}</Button></div>
      </SectionCard>
    {/if}

    {#if qualityRows.length > 0}
      <SectionCard title={t('collab_health.quality.title')} note={t('collab_health.quality.note')}>
        <dl class="values" data-testid="data-quality">
          {#each qualityRows as [key, n] (key)}
            <dt>{label('collab_health.quality', key, KNOWN_HEALTH.quality)}</dt>
            <dd>{n}</dd>
          {/each}
        </dl>
      </SectionCard>
    {/if}
  {/if}

  <SectionCard title={t('collab_health.baseline.title')} note={t('collab_health.baseline.note')}>
    <p class="text">{t('collab_health.baseline.explain')}</p>
    {#if health?.baseline.taken_at}
      <p class="muted" data-testid="baseline-taken">
        {t('collab_health.baseline.already', { time: formatTime(health.baseline.taken_at) })}
      </p>
    {/if}
    {#if baselineError}<ErrorBanner message={baselineError} />{/if}
    <Button variant="danger" disabled={baselineBusy || loading} onclick={takeBaseline}>
      {baselineBusy ? t('common.loading') : t('collab_health.baseline.button')}
    </Button>
    {#if baselineReport}
      <div class="report" data-testid="baseline-report" role="status">
        <p>{t('collab_health.baseline.done', { time: formatTime(baselineReport.observed_at) })}</p>
        <p class="muted">
          {t('collab_health.baseline.added', {
            tasks: baselineReport.added.tasks,
            handoffs: baselineReport.added.handoffs,
            questions: baselineReport.added.questions,
          })}
          {#if baselineReport.existed_before}· {t('collab_health.baseline.existed')}{/if}
        </p>
        <p class="muted">{noteText(baselineReport.note)}</p>
      </div>
    {/if}
  </SectionCard>
{/if}

<style>
  .intro { margin: -8px 0 20px; color: var(--text-secondary); font-size: 13px; max-width: 80ch; }
  .toolbar { margin-bottom: 12px; }
  .filter { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; color: var(--text-secondary); }
  .muted { color: var(--text-muted); font-size: 12px; margin: 4px 0; overflow-wrap: anywhere; }
  .text { font-size: 13px; max-width: 80ch; }
  .facts, .values { display: grid; grid-template-columns: minmax(140px, max-content) 1fr; gap: 4px 16px; margin: 0; font-size: 13px; }
  dt { color: var(--text-secondary); }
  dd { margin: 0; overflow-wrap: anywhere; }
  .banner { margin: 12px 0 0; padding: 8px 10px; font-size: 13px; background: var(--yellow-subtle); border: 1px solid var(--yellow-dim); border-radius: 6px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 16px; }
  .metric { border: 1px solid var(--border-light); border-radius: 8px; padding: 12px 14px; min-width: 0; }
  .metric h3 { margin: 0 0 6px; font-size: 14px; }
  .chips { display: flex; gap: 6px; flex-wrap: wrap; margin-bottom: 8px; }
  .unknown { margin: 4px 0; font-size: 13px; color: var(--yellow); font-weight: 600; }
  .drills { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 10px; }
  .more { margin-top: 12px; }
  .report { margin-top: 12px; padding: 8px 10px; border: 1px solid var(--border); border-radius: 6px; background: var(--bg-tertiary); font-size: 13px; }
  .report p { margin: 2px 0; }
</style>
